//
// listener.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! The loopback HTTP listener that serves the MCP endpoint.
//!
//! This is a separate TCP listener from the supervisor's main API transport,
//! which defaults to a Unix socket or named pipe and must stay that way: agents
//! speak plain HTTP over a URL.
//!
//! Every registered frontend has an endpoint of its own under `/mcp/w/`. The
//! bearer token still decides which frontend a request belongs to; naming it in
//! the path as well means an agent configured with one window's URL and another
//! window's token is refused loudly instead of quietly working on the wrong
//! window.

use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HttpBuilder;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower::Service as _;

use super::auth::{check_request, AuthRejection};
use super::McpState;

/// The path prefix under which each frontend's endpoint lives; the frontend ID
/// follows.
const MCP_PATH_PREFIX: &str = "/mcp/w/";

/// The path rmcp's service expects to see once the frontend has been resolved.
const MCP_PATH: &str = "/mcp";

/// The response body type shared with rmcp's Streamable HTTP service.
type McpBody = BoxBody<Bytes, Infallible>;

/// Bind the MCP listener on loopback.
///
/// Tries `preferred_port` first so an agent's saved URL keeps working across
/// supervisor restarts, and falls back to an OS-assigned port.
pub async fn bind(preferred_port: Option<u16>) -> std::io::Result<(TcpListener, u16)> {
    if let Some(port) = preferred_port.filter(|p| *p != 0) {
        match TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await {
            Ok(listener) => {
                let port = listener.local_addr()?.port();
                return Ok((listener, port));
            }
            Err(e) => log::info!(
                "MCP listener could not use preferred port {} ({}); asking the OS for one",
                port,
                e
            ),
        }
    }

    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Serve MCP requests on an already-bound listener until `cancel` fires.
pub fn serve(listener: TcpListener, state: Arc<McpState>, cancel: CancellationToken) {
    tokio::spawn(async move {
        loop {
            let (stream, peer) = tokio::select! {
                _ = cancel.cancelled() => break,
                accepted = listener.accept() => match accepted {
                    Ok(accepted) => accepted,
                    Err(e) => {
                        log::error!("MCP listener failed to accept a connection: {}", e);
                        continue;
                    }
                },
            };

            let io = TokioIo::new(stream);
            let service = McpConnectionService {
                state: state.clone(),
            };
            let connection_cancel = cancel.clone();
            tokio::spawn(async move {
                let builder = HttpBuilder::new(TokioExecutor::new());
                let serve = builder.serve_connection(io, service);
                tokio::select! {
                    _ = connection_cancel.cancelled() => {}
                    result = serve => {
                        if let Err(e) = result {
                            log::debug!("MCP connection ended ({}): {}", peer, e);
                        }
                    }
                }
            });
        }
    });
}

/// Routes and authorizes one connection's requests before handing them to the
/// protocol layer.
#[derive(Clone)]
struct McpConnectionService {
    state: Arc<McpState>,
}

impl hyper::service::Service<Request<Incoming>> for McpConnectionService {
    type Response = Response<McpBody>;
    type Error = Infallible;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let state = self.state.clone();

        Box::pin(async move {
            log::debug!("MCP {} {}", request.method(), request.uri().path());

            let Some(addressed) = request
                .uri()
                .path()
                .strip_prefix(MCP_PATH_PREFIX)
                .map(|id| id.trim_end_matches('/'))
                .filter(|id| !id.is_empty())
            else {
                log::warn!(
                    "Rejecting MCP request: no endpoint at {}",
                    request.uri().path()
                );
                return Ok(status_response(
                    StatusCode::NOT_FOUND,
                    "Not found; a window's MCP endpoint is at /mcp/w/<frontend-id>",
                ));
            };
            let addressed = addressed.to_string();

            let token = match check_request(request.headers()) {
                Ok(token) => token.to_string(),
                Err(AuthRejection::Forbidden(reason)) => {
                    log::warn!("Rejecting MCP request: {}", reason);
                    return Ok(status_response(StatusCode::FORBIDDEN, &reason));
                }
                Err(AuthRejection::Unauthorized(reason)) => {
                    log::warn!("Rejecting MCP request: {}", reason);
                    // Deliberately no WWW-Authenticate challenge: some agents
                    // read one as an invitation to start an OAuth flow.
                    return Ok(status_response(StatusCode::UNAUTHORIZED, &reason));
                }
            };

            let Some(frontend_id) = state.registry.frontend_for_token(&token).await else {
                log::warn!("Rejecting MCP request: invalid bearer token");
                return Ok(status_response(
                    StatusCode::UNAUTHORIZED,
                    "Invalid bearer token",
                ));
            };

            if frontend_id != addressed {
                log::warn!(
                    "Rejecting MCP request for frontend '{}': the token belongs to '{}'",
                    addressed,
                    frontend_id
                );
                return Ok(status_response(
                    StatusCode::FORBIDDEN,
                    "This token belongs to a different Positron window. Use the URL and token \
                     from the same window, which its integrated terminals publish as \
                     POSITRON_MCP_URL and POSITRON_MCP_TOKEN.",
                ));
            }

            let Some(mut service) = state.service_for(&frontend_id).await else {
                log::warn!("Rejecting MCP request: the listener is shutting down");
                return Ok(status_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The MCP server is shutting down",
                ));
            };

            // rmcp serves one endpoint, so the frontend is addressed by the
            // service handling the request rather than by the path.
            let mut request = request;
            let mut parts = request.uri().clone().into_parts();
            parts.path_and_query = Some(MCP_PATH.parse().expect("MCP path is a valid path"));
            *request.uri_mut() = hyper::Uri::from_parts(parts).expect("Rewritten MCP URI is valid");

            match service.call(request).await {
                Ok(response) => Ok(response),
                Err(never) => match never {},
            }
        })
    }
}

/// Build a plain-text response with the given status.
fn status_response(status: StatusCode, message: &str) -> Response<McpBody> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .body(Full::new(Bytes::from(message.to_string())).boxed())
        .expect("Unable to build MCP status response")
}
