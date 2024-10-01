use futures::{future, future::BoxFuture, future::FutureExt, stream, stream::TryStreamExt, Stream};
// --- Start Kallichore ---
/*
use hyper::header::{
    HeaderName, HeaderValue, CONNECTION, CONTENT_TYPE, SEC_WEBSOCKET_KEY, UPGRADE,
};
*/
use hyper::header::{HeaderName, HeaderValue, CONTENT_TYPE};
// --- End Kallichore ---
use hyper::{Body, HeaderMap, Request, Response, StatusCode};
use log::warn;
#[allow(unused_imports)]
use std::convert::{TryFrom, TryInto};
use std::error::Error;
use std::future::Future;
use std::marker::PhantomData;
use std::task::{Context, Poll};
pub use swagger::auth::Authorization;
use swagger::auth::Scopes;
use swagger::{ApiError, BodyExt, Has, RequestParser, XSpanIdString};
use url::form_urlencoded;

use crate::header;
#[allow(unused_imports)]
use crate::models;

pub use crate::context;

type ServiceFuture = BoxFuture<'static, Result<Response<Body>, crate::ServiceError>>;

use crate::{
    Api, ChannelsWebsocketResponse, DeleteSessionResponse, GetSessionResponse,
    InterruptSessionResponse, KillSessionResponse, ListSessionsResponse, NewSessionResponse,
    RestartSessionResponse, ShutdownServerResponse, StartSessionResponse,
};

mod paths {
    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref GLOBAL_REGEX_SET: regex::RegexSet = regex::RegexSet::new(vec![
            r"^/sessions$",
            r"^/sessions/(?P<session_id>[^/?#]*)$",
            r"^/sessions/(?P<session_id>[^/?#]*)/channels$",
            r"^/sessions/(?P<session_id>[^/?#]*)/interrupt$",
            r"^/sessions/(?P<session_id>[^/?#]*)/kill$",
            r"^/sessions/(?P<session_id>[^/?#]*)/restart$",
            r"^/sessions/(?P<session_id>[^/?#]*)/start$",
            r"^/shutdown$"
        ])
        .expect("Unable to create global regex set");
    }
    pub(crate) static ID_SESSIONS: usize = 0;
    pub(crate) static ID_SESSIONS_SESSION_ID: usize = 1;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_CHANNELS: usize = 2;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_CHANNELS: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/channels$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_CHANNELS");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_INTERRUPT: usize = 3;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_INTERRUPT: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/interrupt$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_INTERRUPT");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_KILL: usize = 4;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_KILL: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/kill$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_KILL");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_RESTART: usize = 5;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_RESTART: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/restart$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_RESTART");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_START: usize = 6;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_START: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/start$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_START");
    }
    pub(crate) static ID_SHUTDOWN: usize = 7;
}

pub struct MakeService<T, C>
where
    T: Api<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    api_impl: T,
    marker: PhantomData<C>,
}

impl<T, C> MakeService<T, C>
where
    T: Api<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    pub fn new(api_impl: T) -> Self {
        MakeService {
            api_impl,
            marker: PhantomData,
        }
    }
}

impl<T, C, Target> hyper::service::Service<Target> for MakeService<T, C>
where
    T: Api<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    type Response = Service<T, C>;
    type Error = crate::ServiceError;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, target: Target) -> Self::Future {
        future::ok(Service::new(self.api_impl.clone()))
    }
}

fn method_not_allowed() -> Result<Response<Body>, crate::ServiceError> {
    Ok(Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Body::empty())
        .expect("Unable to create Method Not Allowed response"))
}

pub struct Service<T, C>
where
    T: Api<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    api_impl: T,
    marker: PhantomData<C>,
}

impl<T, C> Service<T, C>
where
    T: Api<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    pub fn new(api_impl: T) -> Self {
        Service {
            api_impl,
            marker: PhantomData,
        }
    }
}

impl<T, C> Clone for Service<T, C>
where
    T: Api<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Service {
            api_impl: self.api_impl.clone(),
            marker: self.marker,
        }
    }
}

impl<T, C> hyper::service::Service<(Request<Body>, C)> for Service<T, C>
where
    T: Api<C> + Clone + Send + Sync + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = crate::ServiceError;
    type Future = ServiceFuture;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.api_impl.poll_ready(cx)
    }

    fn call(&mut self, req: (Request<Body>, C)) -> Self::Future {
        async fn run<T, C>(
            mut api_impl: T,
            req: (Request<Body>, C),
        ) -> Result<Response<Body>, crate::ServiceError>
        where
            T: Api<C> + Clone + Send + 'static,
            C: Has<XSpanIdString> + Send + Sync + 'static,
        {
            let (request, context) = req;
            // --- Start Kallichore ---
            /*
            let (parts, body) = request.into_parts();
            let (method, uri, headers) = (parts.method, parts.uri, parts.headers);
            */
            let method = request.method().clone();
            let uri = request.uri().clone();
            let headers = request.headers().clone();
            // --- End Kallichore ---
            let path = paths::GLOBAL_REGEX_SET.matches(uri.path());

            match method {
                // ChannelsWebsocket - GET /sessions/{session_id}/channels
                hyper::Method::GET if path.matched(paths::ID_SESSIONS_SESSION_ID_CHANNELS) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID_CHANNELS
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID_CHANNELS in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID_CHANNELS.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't parse path parameter session_id: {}", e)))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    // --- Start Kallichore ---
                    log::debug!(
                        "channels_websocket_request for session '{}'",
                        param_session_id
                    );
                    let response = api_impl
                        .channels_websocket_request(request, param_session_id, &context)
                        .await
                        .unwrap();

                    /*
                    let result = api_impl
                        .channels_websocket(param_session_id, &context)
                        .await;

                    match result {
                        Ok(rsp) => match rsp {
                            ChannelsWebsocketResponse::UpgradeConnectionToAWebsocket => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                            }
                            ChannelsWebsocketResponse::InvalidRequest(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for CHANNELS_WEBSOCKET_INVALID_REQUEST"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            ChannelsWebsocketResponse::AccessTokenIsMissingOrInvalid => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                            ChannelsWebsocketResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    } */

                    Ok(response)
                }

                // DeleteSession - DELETE /sessions/{session_id}
                hyper::Method::DELETE if path.matched(paths::ID_SESSIONS_SESSION_ID) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't parse path parameter session_id: {}", e)))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.delete_session(param_session_id, &context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            DeleteSessionResponse::SessionDeleted(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for DELETE_SESSION_SESSION_DELETED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            DeleteSessionResponse::FailedToDeleteSession(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for DELETE_SESSION_FAILED_TO_DELETE_SESSION"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            DeleteSessionResponse::AccessTokenIsMissingOrInvalid => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                            DeleteSessionResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // GetSession - GET /sessions/{session_id}
                hyper::Method::GET if path.matched(paths::ID_SESSIONS_SESSION_ID) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't parse path parameter session_id: {}", e)))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.get_session(param_session_id, &context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            GetSessionResponse::SessionDetails(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for GET_SESSION_SESSION_DETAILS"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            GetSessionResponse::FailedToGetSession(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for GET_SESSION_FAILED_TO_GET_SESSION"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            GetSessionResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // InterruptSession - POST /sessions/{session_id}/interrupt
                hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_INTERRUPT) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID_INTERRUPT
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID_INTERRUPT in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID_INTERRUPT.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't parse path parameter session_id: {}", e)))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.interrupt_session(param_session_id, &context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            InterruptSessionResponse::Interrupted(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for INTERRUPT_SESSION_INTERRUPTED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            InterruptSessionResponse::InterruptFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for INTERRUPT_SESSION_INTERRUPT_FAILED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            InterruptSessionResponse::AccessTokenIsMissingOrInvalid => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                            InterruptSessionResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // KillSession - POST /sessions/{session_id}/kill
                hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_KILL) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID_KILL
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID_KILL in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID_KILL.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't parse path parameter session_id: {}", e)))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.kill_session(param_session_id, &context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            KillSessionResponse::Killed(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for KILL_SESSION_KILLED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            KillSessionResponse::KillFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for KILL_SESSION_KILL_FAILED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            KillSessionResponse::AccessTokenIsMissingOrInvalid => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                            KillSessionResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // ListSessions - GET /sessions
                hyper::Method::GET if path.matched(paths::ID_SESSIONS) => {
                    let result = api_impl.list_sessions(&context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            ListSessionsResponse::ListOfActiveSessions(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for LIST_SESSIONS_LIST_OF_ACTIVE_SESSIONS"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // NewSession - PUT /sessions
                hyper::Method::PUT if path.matched(paths::ID_SESSIONS) => {
                    // Body parameters (note that non-required body parameters will ignore garbage
                    // values, rather than causing a 400 response). Produce warning header and logs for
                    // any unused fields.
                    // --- Start Kallichore ---
                    let body = request.into_body();
                    // --- End Kallichore ---
                    let result = body.into_raw().await;
                    match result {
                            Ok(body) => {
                                let mut unused_elements = Vec::new();
                                let param_new_session: Option<models::NewSession> = if !body.is_empty() {
                                    let deserializer = &mut serde_json::Deserializer::from_slice(&body);
                                    let handle_unknown_field = |path: serde_ignored::Path<'_>| {
                                        warn!("Ignoring unknown field in body: {}", path);
                                        unused_elements.push(path.to_string());
                                    };
                                    match serde_ignored::deserialize(deserializer, handle_unknown_field) {
                                        Ok(param_new_session) => param_new_session,
                                        Err(e) => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(Body::from(format!("Couldn't parse body parameter NewSession - doesn't match schema: {}", e)))
                                                        .expect("Unable to create Bad Request response for invalid body parameter NewSession due to schema")),
                                    }
                                } else {
                                    None
                                };
                                let param_new_session = match param_new_session {
                                    Some(param_new_session) => param_new_session,
                                    None => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(Body::from("Missing required body parameter NewSession"))
                                                        .expect("Unable to create Bad Request response for missing body parameter NewSession")),
                                };

                                let result = api_impl.new_session(
                                            param_new_session,
                                        &context
                                    ).await;
                                let mut response = Response::new(Body::empty());
                                response.headers_mut().insert(
                                            HeaderName::from_static("x-span-id"),
                                            HeaderValue::from_str((&context as &dyn Has<XSpanIdString>).get().0.clone().as_str())
                                                .expect("Unable to create X-Span-ID header value"));

                                        if !unused_elements.is_empty() {
                                            response.headers_mut().insert(
                                                HeaderName::from_static("warning"),
                                                HeaderValue::from_str(format!("Ignoring unknown fields in body: {:?}", unused_elements).as_str())
                                                    .expect("Unable to create Warning header value"));
                                        }

                                        match result {
                                            Ok(rsp) => match rsp {
                                                NewSessionResponse::TheSessionID
                                                    (body)
                                                => {
                                                    *response.status_mut() = StatusCode::from_u16(200).expect("Unable to turn 200 into a StatusCode");
                                                    response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for NEW_SESSION_THE_SESSION_ID"));
                                                    let body_content = serde_json::to_string(&body).expect("impossible to fail to serialize");
                                                    *response.body_mut() = Body::from(body_content);
                                                },
                                                NewSessionResponse::InvalidRequest
                                                    (body)
                                                => {
                                                    *response.status_mut() = StatusCode::from_u16(400).expect("Unable to turn 400 into a StatusCode");
                                                    response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for NEW_SESSION_INVALID_REQUEST"));
                                                    let body_content = serde_json::to_string(&body).expect("impossible to fail to serialize");
                                                    *response.body_mut() = Body::from(body_content);
                                                },
                                                NewSessionResponse::AccessTokenIsMissingOrInvalid
                                                => {
                                                    *response.status_mut() = StatusCode::from_u16(401).expect("Unable to turn 401 into a StatusCode");
                                                },
                                            },
                                            Err(_) => {
                                                // Application code returned an error. This should not happen, as the implementation should
                                                // return a valid response.
                                                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                                *response.body_mut() = Body::from("An internal error occurred");
                                            },
                                        }

                                        Ok(response)
                            },
                            Err(e) => Ok(Response::builder()
                                                .status(StatusCode::BAD_REQUEST)
                                                .body(Body::from(format!("Couldn't read body parameter NewSession: {}", e)))
                                                .expect("Unable to create Bad Request response due to unable to read body parameter NewSession")),
                        }
                }

                // RestartSession - POST /sessions/{session_id}/restart
                hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_RESTART) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID_RESTART
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID_RESTART in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID_RESTART.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't parse path parameter session_id: {}", e)))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.restart_session(param_session_id, &context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            RestartSessionResponse::Restarted(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for RESTART_SESSION_RESTARTED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            RestartSessionResponse::RestartFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for RESTART_SESSION_RESTART_FAILED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            RestartSessionResponse::AccessTokenIsMissingOrInvalid => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                            RestartSessionResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // ShutdownServer - POST /shutdown
                hyper::Method::POST if path.matched(paths::ID_SHUTDOWN) => {
                    let result = api_impl.shutdown_server(&context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            ShutdownServerResponse::ShuttingDown(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for SHUTDOWN_SERVER_SHUTTING_DOWN"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            ShutdownServerResponse::ShutdownFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for SHUTDOWN_SERVER_SHUTDOWN_FAILED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            ShutdownServerResponse::AccessTokenIsMissingOrInvalid => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // StartSession - POST /sessions/{session_id}/start
                hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_START) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID_START
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID_START in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID_START.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't parse path parameter session_id: {}", e)))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(Body::from(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.start_session(param_session_id, &context).await;
                    let mut response = Response::new(Body::empty());
                    response.headers_mut().insert(
                        HeaderName::from_static("x-span-id"),
                        HeaderValue::from_str(
                            (&context as &dyn Has<XSpanIdString>)
                                .get()
                                .0
                                .clone()
                                .as_str(),
                        )
                        .expect("Unable to create X-Span-ID header value"),
                    );

                    match result {
                        Ok(rsp) => match rsp {
                            StartSessionResponse::Started(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for START_SESSION_STARTED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            StartSessionResponse::StartFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                                        CONTENT_TYPE,
                                                        HeaderValue::from_str("application/json")
                                                            .expect("Unable to create Content-Type header for START_SESSION_START_FAILED"));
                                let body_content = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = Body::from(body_content);
                            }
                            StartSessionResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                            StartSessionResponse::AccessTokenIsMissingOrInvalid => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = Body::from("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                _ if path.matched(paths::ID_SESSIONS) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_CHANNELS) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_INTERRUPT) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_KILL) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_RESTART) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_START) => method_not_allowed(),
                _ if path.matched(paths::ID_SHUTDOWN) => method_not_allowed(),
                _ => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .expect("Unable to create Not Found response")),
            }
        }
        Box::pin(run(self.api_impl.clone(), req))
    }
}

/// Request parser for `Api`.
pub struct ApiRequestParser;
impl<T> RequestParser<T> for ApiRequestParser {
    fn parse_operation_id(request: &Request<T>) -> Option<&'static str> {
        let path = paths::GLOBAL_REGEX_SET.matches(request.uri().path());
        match *request.method() {
            // ChannelsWebsocket - GET /sessions/{session_id}/channels
            hyper::Method::GET if path.matched(paths::ID_SESSIONS_SESSION_ID_CHANNELS) => {
                Some("ChannelsWebsocket")
            }
            // DeleteSession - DELETE /sessions/{session_id}
            hyper::Method::DELETE if path.matched(paths::ID_SESSIONS_SESSION_ID) => {
                Some("DeleteSession")
            }
            // GetSession - GET /sessions/{session_id}
            hyper::Method::GET if path.matched(paths::ID_SESSIONS_SESSION_ID) => Some("GetSession"),
            // InterruptSession - POST /sessions/{session_id}/interrupt
            hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_INTERRUPT) => {
                Some("InterruptSession")
            }
            // KillSession - POST /sessions/{session_id}/kill
            hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_KILL) => {
                Some("KillSession")
            }
            // ListSessions - GET /sessions
            hyper::Method::GET if path.matched(paths::ID_SESSIONS) => Some("ListSessions"),
            // NewSession - PUT /sessions
            hyper::Method::PUT if path.matched(paths::ID_SESSIONS) => Some("NewSession"),
            // RestartSession - POST /sessions/{session_id}/restart
            hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_RESTART) => {
                Some("RestartSession")
            }
            // ShutdownServer - POST /shutdown
            hyper::Method::POST if path.matched(paths::ID_SHUTDOWN) => Some("ShutdownServer"),
            // StartSession - POST /sessions/{session_id}/start
            hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_START) => {
                Some("StartSession")
            }
            _ => None,
        }
    }
}
