//
// websocket_service.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

//! Custom service wrapper that intercepts websocket requests to provide raw HTTP request access.

use std::task::{Context, Poll};

use futures::future::BoxFuture;
use hyper::{Body, Request, Response};
use swagger::{ApiError, Authorization, Has, XSpanIdString};

use kallichore_api::Api;

/// Extension trait to provide access to the custom websocket request handler
/// This allows us to access the `channels_websocket_request` method that handles raw HTTP requests
pub trait ApiWebsocketExt<C>
where
    C: Send + Sync + 'static,
{
    fn channels_websocket_request(
        &self,
        request: Request<Body>,
        session_id: String,
        context: &C,
    ) -> BoxFuture<'static, Result<Response<Body>, ApiError>>;
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
}

impl<T, C> WebsocketInterceptorService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    pub fn new(api_impl: T) -> Self {
        let inner_service = kallichore_api::server::Service::new(api_impl.clone());
        Self {
            api_impl,
            inner_service,
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
        }
    }
}

impl<T, C> hyper::service::Service<(Request<Body>, C)> for WebsocketInterceptorService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + Sync + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner_service.poll_ready(cx)
    }

    fn call(&mut self, req: (Request<Body>, C)) -> Self::Future {
        let (request, context) = req;
        let method = request.method().clone();
        let path = request.uri().path().to_string();

        // Check if this is a websocket channels request
        if method == hyper::Method::GET && is_websocket_channels_path(&path) {
            // Extract session_id from path
            if let Some(session_id) = extract_session_id_from_path(&path) {
                let api_impl = self.api_impl.clone();

                Box::pin(async move {
                    // Call our custom websocket request handler with raw request access
                    match api_impl
                        .channels_websocket_request(request, session_id, &context)
                        .await
                    {
                        Ok(response) => Ok(response),
                        Err(e) => {
                            log::error!("Websocket request handler error: {:?}", e);
                            let response = Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("Internal server error during websocket upgrade"))
                                .expect("Unable to create error response");
                            Ok(response)
                        }
                    }
                })
            } else {
                // If we can't extract session_id, fall back to the normal service
                let mut inner_service = self.inner_service.clone();
                Box::pin(async move { inner_service.call((request, context)).await })
            }
        } else {
            // For all other requests, delegate to the generated service
            let mut inner_service = self.inner_service.clone();
            Box::pin(async move { inner_service.call((request, context)).await })
        }
    }
}

/// Check if the path matches the websocket channels pattern
fn is_websocket_channels_path(path: &str) -> bool {
    // Use regex to match /sessions/{session_id}/channels
    use regex::Regex;
    use std::sync::OnceLock;

    static WEBSOCKET_REGEX: OnceLock<Regex> = OnceLock::new();
    let regex = WEBSOCKET_REGEX
        .get_or_init(|| Regex::new(r"^/sessions/[^/?#]+/channels$").expect("Invalid regex"));

    regex.is_match(path)
}

/// Extract session_id from the websocket channels path
fn extract_session_id_from_path(path: &str) -> Option<String> {
    use regex::Regex;
    use std::sync::OnceLock;

    static EXTRACT_REGEX: OnceLock<Regex> = OnceLock::new();
    let regex = EXTRACT_REGEX
        .get_or_init(|| Regex::new(r"^/sessions/([^/?#]+)/channels$").expect("Invalid regex"));

    regex
        .captures(path)
        .and_then(|caps| caps.get(1))
        .map(|m| {
            // URL decode the session_id
            percent_encoding::percent_decode(m.as_str().as_bytes())
                .decode_utf8()
                .ok()
                .map(|s| s.to_string())
        })
        .flatten()
}

/// Custom MakeService that creates our websocket interceptor service
pub struct WebsocketInterceptorMakeService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    api_impl: T,
    _marker: std::marker::PhantomData<C>,
}

impl<T, C> WebsocketInterceptorMakeService<T, C>
where
    T: Api<C> + ApiWebsocketExt<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    pub fn new(api_impl: T) -> Self {
        Self {
            api_impl,
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

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: Target) -> Self::Future {
        let service = WebsocketInterceptorService::new(self.api_impl.clone());
        futures::future::ready(Ok(service))
    }
}
