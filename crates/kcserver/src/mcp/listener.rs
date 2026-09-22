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
//! Every registered workspace has an endpoint of its own under `/mcp/w/`. The
//! bearer token still decides which workspace a request belongs to; naming it
//! in the path as well means an agent configured with one workspace's URL and
//! another's token is refused loudly instead of quietly working on the wrong
//! sessions.
//!
//! A workspace's endpoint may carry a further `/s/<session-id>`, which is how a
//! client running inside one of the workspace's kernels says which session it
//! is. That is a hint the caller volunteers about itself, not a permission: the
//! token still decides the workspace, and naming a session can only narrow what
//! the caller may do.
//!
//! Each endpoint also serves its [server card](super::card) at
//! `/server-card`, the one request that needs no token.

use std::collections::hash_map::DefaultHasher;
use std::convert::Infallible;
use std::hash::{Hash, Hasher};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HttpBuilder;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower::Service as _;

use super::auth::{check_loopback, check_request, AuthRejection};
use super::card::{server_card, CARD_MEDIA_TYPE, CARD_PATH_SUFFIX};
use super::McpState;

/// The path prefix under which each workspace's endpoint lives; the workspace
/// ID follows.
pub(crate) const MCP_PATH_PREFIX: &str = "/mcp/w/";

/// The path rmcp's service expects to see once the workspace has been resolved.
const MCP_PATH: &str = "/mcp";

/// The path segment, following a workspace ID, under which a caller names the
/// session it is running in.
pub(crate) const CALLER_PATH_SEGMENT: &str = "/s/";

/// The response body type shared with rmcp's Streamable HTTP service.
type McpBody = BoxBody<Bytes, Infallible>;

/// The URL of a workspace's endpoint: what agents are configured with, and what
/// its server card advertises.
pub fn endpoint_url(port: u16, workspace_id: &str) -> String {
    format!(
        "http://127.0.0.1:{}{}{}",
        port, MCP_PATH_PREFIX, workspace_id
    )
}

/// The URL handed to a kernel: the workspace's endpoint, with the kernel's own
/// session named.
///
/// A client inside a kernel reaches the same workspace with the same token as
/// any other agent. What the extra segment buys is that the server can tell
/// when such a client asks to run code in the session it is itself running in,
/// which would queue behind the call still waiting for an answer.
pub fn session_endpoint_url(port: u16, workspace_id: &str, session_id: &str) -> String {
    format!(
        "{}{}{}",
        endpoint_url(port, workspace_id),
        CALLER_PATH_SEGMENT,
        session_id
    )
}

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

            let Some(endpoint) = request
                .uri()
                .path()
                .strip_prefix(MCP_PATH_PREFIX)
                .map(|rest| rest.trim_end_matches('/'))
                .filter(|rest| !rest.is_empty())
            else {
                log::warn!(
                    "Rejecting MCP request: no endpoint at {}",
                    request.uri().path()
                );
                return Ok(status_response(
                    StatusCode::NOT_FOUND,
                    "Not found; a workspace's MCP endpoint is at /mcp/w/<workspace-id>",
                ));
            };

            // A card is read at whichever endpoint the reader holds, so its
            // suffix comes off before the endpoint itself is taken apart.
            let (endpoint, card) = match endpoint.strip_suffix(CARD_PATH_SUFFIX) {
                Some(endpoint) => (endpoint, true),
                None => (endpoint, false),
            };

            // A caller may name the session it is running in after the
            // workspace; see [`session_endpoint_url`].
            let (addressed, caller) = match endpoint.split_once(CALLER_PATH_SEGMENT) {
                Some((workspace, session)) if !session.is_empty() => {
                    (workspace, Some(session.to_string()))
                }
                Some((workspace, _)) => (workspace, None),
                None => (endpoint, None),
            };

            // The card is read before a client has a token, so it is routed
            // ahead of the bearer check.
            if card {
                return Ok(card_response(&state, &request, addressed, caller.as_deref()).await);
            }
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

            let Some(workspace_id) = state.registry.workspace_for_token(&token).await else {
                log::warn!("Rejecting MCP request: invalid bearer token");
                return Ok(status_response(
                    StatusCode::UNAUTHORIZED,
                    "Invalid bearer token",
                ));
            };

            if workspace_id != addressed {
                log::warn!(
                    "Rejecting MCP request for workspace '{}': the token belongs to '{}'",
                    addressed,
                    workspace_id
                );
                return Ok(status_response(
                    StatusCode::FORBIDDEN,
                    "This token belongs to a different Positron workspace. Use the URL and token \
                     from the same window, which its integrated terminals publish as \
                     POSITRON_MCP_URL and POSITRON_MCP_TOKEN.",
                ));
            }

            let Some(mut service) = state.service_for(&workspace_id, caller.as_deref()).await
            else {
                log::warn!("Rejecting MCP request: the listener is shutting down");
                return Ok(status_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The MCP server is shutting down",
                ));
            };

            // rmcp serves one endpoint, so the workspace is addressed by the
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

/// Serve a workspace's server card.
///
/// Unauthenticated, but still behind the loopback guards: a page in the user's
/// browser has no business reading it, and for the same reason the response
/// carries no CORS headers, which the card specification asks for on the public
/// endpoints it was written for. Nor does it invite a shared cache to keep a
/// document naming the user's workspace; an entity tag is enough to save the
/// transfer.
async fn card_response(
    state: &Arc<McpState>,
    request: &Request<Incoming>,
    workspace_id: &str,
    caller: Option<&str>,
) -> Response<McpBody> {
    if request.method() != Method::GET {
        return status_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "A server card is read with GET",
        );
    }

    if let Err(AuthRejection::Forbidden(reason) | AuthRejection::Unauthorized(reason)) =
        check_loopback(request.headers())
    {
        log::warn!("Rejecting server card request: {}", reason);
        return status_response(StatusCode::FORBIDDEN, &reason);
    }

    let Some(display_name) = state.registry.display_name(workspace_id).await else {
        return status_response(
            StatusCode::NOT_FOUND,
            "No such workspace; its window may have closed",
        );
    };
    let Some(port) = state.port().await else {
        return status_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "The MCP server is shutting down",
        );
    };

    let card = server_card(port, workspace_id, &display_name, caller).to_string();
    let etag = entity_tag(&card);
    let unchanged = request
        .headers()
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|tag| tag.trim() == etag));

    let builder = Response::builder()
        .header(CACHE_CONTROL, "no-cache")
        .header(ETAG, &etag);
    let response = if unchanged {
        builder
            .status(StatusCode::NOT_MODIFIED)
            .body(Full::new(Bytes::new()).boxed())
    } else {
        builder
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, CARD_MEDIA_TYPE)
            .body(Full::new(Bytes::from(card)).boxed())
    };
    response.expect("Unable to build server card response")
}

/// An opaque validator for a card, so an unchanged one need not be sent twice.
fn entity_tag(card: &str) -> String {
    let mut hasher = DefaultHasher::new();
    card.hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}

/// Build a plain-text response with the given status.
fn status_response(status: StatusCode, message: &str) -> Response<McpBody> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain")
        .body(Full::new(Bytes::from(message.to_string())).boxed())
        .expect("Unable to build MCP status response")
}
