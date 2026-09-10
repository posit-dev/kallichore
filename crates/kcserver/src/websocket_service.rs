//
// websocket_service.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Custom service wrapper that intercepts websocket requests to provide raw HTTP request access.

use futures::future::BoxFuture;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::Service as HyperService;
use hyper::{Request, Response};
use swagger::{ApiError, Authorization, Has, XSpanIdString};

use kallichore_api::Api;

/// A request path this service handles itself rather than passing to the
/// generated API service, because completing it needs the raw HTTP request.
enum WebsocketRoute {
    /// `/sessions/{session_id}/channels`
    SessionChannels(String),

    /// `/mcp/frontends/{frontend_id}/channel`
    McpFrontendChannel(String),
}

/// Extension trait to provide access to the custom websocket request handlers.
/// These need the raw HTTP request, which the generated API service does not
/// hand to its operations.
pub trait ApiWebsocketExt<C>
where
    C: Send + Sync + 'static,
{
    fn channels_websocket_request(
        &self,
        request: Request<Incoming>,
        session_id: String,
        context: &C,
    ) -> BoxFuture<'static, Result<Response<BoxBody<bytes::Bytes, std::io::Error>>, ApiError>>;

    fn mcp_frontend_channel_request(
        &self,
        request: Request<Incoming>,
        frontend_id: String,
        context: &C,
    ) -> BoxFuture<'static, Result<Response<BoxBody<bytes::Bytes, std::io::Error>>, ApiError>>;
}

/// A service wrapper that intercepts websocket channel requests to provide raw HTTP request access.
/// This allows the websocket upgrade to work properly while maintaining compatibility with the
/// auto-generated API service for all other routes.
pub struct WebsocketInterceptorService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    api_impl: T,
    inner_service: kallichore_api::server::Service<T, C>,
    /// Whether session channels upgrade in place. Only TCP does; the domain
    /// socket and named pipe transports hand out a per-session endpoint
    /// instead, so their session channel requests must reach the generated
    /// service. The MCP frontend channel always upgrades in place.
    intercept_session_channels: bool,
}

impl<T, C> WebsocketInterceptorService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    pub fn new(api_impl: T, intercept_session_channels: bool) -> Self {
        let inner_service = kallichore_api::server::Service::new(api_impl.clone());
        Self {
            api_impl,
            inner_service,
            intercept_session_channels,
        }
    }
}

impl<T, C> Clone for WebsocketInterceptorService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            api_impl: self.api_impl.clone(),
            inner_service: self.inner_service.clone(),
            intercept_session_channels: self.intercept_session_channels,
        }
    }
}

impl<T, C> hyper::service::Service<(Request<Incoming>, C)> for WebsocketInterceptorService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + Sync + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    type Response = Response<BoxBody<bytes::Bytes, std::io::Error>>;
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: (Request<Incoming>, C)) -> Self::Future {
        let (request, context) = req;
        let route = if request.method() == hyper::Method::GET {
            match websocket_route(request.uri().path()) {
                Some(WebsocketRoute::SessionChannels(_)) if !self.intercept_session_channels => {
                    None
                }
                route => route,
            }
        } else {
            None
        };

        let Some(route) = route else {
            // For all other requests, delegate to the generated service
            let inner_service = self.inner_service.clone();
            return Box::pin(async move {
                HyperService::call(&inner_service, (request, context))
                    .await
                    .map(|response| {
                        response.map(|body| {
                            body.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                                .boxed()
                        })
                    })
                    .map_err(|e| e.into())
            });
        };

        let api_impl = self.api_impl.clone();
        Box::pin(async move {
            let handled = match route {
                WebsocketRoute::SessionChannels(session_id) => {
                    api_impl
                        .channels_websocket_request(request, session_id, &context)
                        .await
                }
                WebsocketRoute::McpFrontendChannel(frontend_id) => {
                    api_impl
                        .mcp_frontend_channel_request(request, frontend_id, &context)
                        .await
                }
            };
            match handled {
                Ok(response) => Ok(response),
                Err(e) => {
                    log::error!("Websocket request handler error: {:?}", e);
                    let response = Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .body(
                            Full::new(bytes::Bytes::from(
                                "Internal server error during websocket upgrade",
                            ))
                            .map_err(|_| {
                                std::io::Error::new(std::io::ErrorKind::Other, "infallible")
                            })
                            .boxed(),
                        )
                        .expect("Unable to create error response");
                    Ok(response)
                }
            }
        })
    }
}

/// Match a request path against the routes this service handles itself,
/// returning the ID captured from the path.
fn websocket_route(path: &str) -> Option<WebsocketRoute> {
    use regex::Regex;
    use std::sync::OnceLock;

    static SESSION_CHANNELS: OnceLock<Regex> = OnceLock::new();
    static MCP_FRONTEND_CHANNEL: OnceLock<Regex> = OnceLock::new();

    let sessions = SESSION_CHANNELS
        .get_or_init(|| Regex::new(r"^/sessions/([^/?#]+)/channels$").expect("Invalid regex"));
    if let Some(id) = capture_id(sessions, path) {
        return Some(WebsocketRoute::SessionChannels(id));
    }

    let frontends = MCP_FRONTEND_CHANNEL
        .get_or_init(|| Regex::new(r"^/mcp/frontends/([^/?#]+)/channel$").expect("Invalid regex"));
    capture_id(frontends, path).map(WebsocketRoute::McpFrontendChannel)
}

/// Extract and URL-decode the first capture group of a path pattern.
fn capture_id(regex: &regex::Regex, path: &str) -> Option<String> {
    let captured = regex.captures(path)?.get(1)?;
    percent_encoding::percent_decode(captured.as_str().as_bytes())
        .decode_utf8()
        .ok()
        .map(|s| s.to_string())
}

/// Custom MakeService that creates our websocket interceptor service
pub struct WebsocketInterceptorMakeService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    api_impl: T,
    intercept_session_channels: bool,
    _marker: std::marker::PhantomData<C>,
}

impl<T, C> WebsocketInterceptorMakeService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    pub fn new(api_impl: T, intercept_session_channels: bool) -> Self {
        Self {
            api_impl,
            intercept_session_channels,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, C, Target> hyper::service::Service<Target> for WebsocketInterceptorMakeService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    type Response = WebsocketInterceptorService<T, C>;
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
    type Future = futures::future::Ready<Result<Self::Response, Self::Error>>;

    fn call(&self, _target: Target) -> Self::Future {
        let service = WebsocketInterceptorService::new(
            self.api_impl.clone(),
            self.intercept_session_channels,
        );
        futures::future::ready(Ok(service))
    }
}
