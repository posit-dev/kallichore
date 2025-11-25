use bytes::Bytes;
use futures::{future, future::BoxFuture, future::FutureExt, stream, stream::TryStreamExt, Stream};
use http_body_util::{combinators::BoxBody, Full};
use hyper::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use hyper::{
    body::{Body, Incoming},
    HeaderMap, Request, Response, StatusCode,
};
use log::warn;
#[allow(unused_imports)]
use std::convert::{TryFrom, TryInto};
use std::future::Future;
use std::marker::PhantomData;
use std::task::{Context, Poll};
use std::{convert::Infallible, error::Error};
pub use swagger::auth::Authorization;
use swagger::auth::Scopes;
use swagger::{ApiError, BodyExt, Has, RequestParser, XSpanIdString};
use url::form_urlencoded;

#[allow(unused_imports)]
use crate::{header, models, AuthenticationApi};

pub use crate::context;

type ServiceFuture =
    BoxFuture<'static, Result<Response<BoxBody<Bytes, Infallible>>, crate::ServiceError>>;

use crate::{
    AdoptSessionResponse, Api, ChannelsUpgradeResponse, ClientHeartbeatResponse,
    ConnectionInfoResponse, DeleteSessionResponse, GetServerConfigurationResponse,
    GetSessionResponse, InterruptSessionResponse, KillSessionResponse, ListSessionsResponse,
    NewSessionResponse, RestartSessionResponse, ServerStatusResponse,
    SetServerConfigurationResponse, ShutdownServerResponse, StartSessionResponse,
};

mod server_auth;

mod paths {
    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref GLOBAL_REGEX_SET: regex::RegexSet = regex::RegexSet::new(vec![
            r"^/client_heartbeat$",
            r"^/server_configuration$",
            r"^/sessions$",
            r"^/sessions/(?P<session_id>[^/?#]*)$",
            r"^/sessions/(?P<session_id>[^/?#]*)/adopt$",
            r"^/sessions/(?P<session_id>[^/?#]*)/channels$",
            r"^/sessions/(?P<session_id>[^/?#]*)/connection_info$",
            r"^/sessions/(?P<session_id>[^/?#]*)/interrupt$",
            r"^/sessions/(?P<session_id>[^/?#]*)/kill$",
            r"^/sessions/(?P<session_id>[^/?#]*)/restart$",
            r"^/sessions/(?P<session_id>[^/?#]*)/start$",
            r"^/shutdown$",
            r"^/status$"
        ])
        .expect("Unable to create global regex set");
    }
    pub(crate) static ID_CLIENT_HEARTBEAT: usize = 0;
    pub(crate) static ID_SERVER_CONFIGURATION: usize = 1;
    pub(crate) static ID_SESSIONS: usize = 2;
    pub(crate) static ID_SESSIONS_SESSION_ID: usize = 3;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_ADOPT: usize = 4;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_ADOPT: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/adopt$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_ADOPT");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_CHANNELS: usize = 5;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_CHANNELS: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/channels$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_CHANNELS");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_CONNECTION_INFO: usize = 6;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_CONNECTION_INFO: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/connection_info$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_CONNECTION_INFO");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_INTERRUPT: usize = 7;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_INTERRUPT: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/interrupt$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_INTERRUPT");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_KILL: usize = 8;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_KILL: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/kill$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_KILL");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_RESTART: usize = 9;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_RESTART: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/restart$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_RESTART");
    }
    pub(crate) static ID_SESSIONS_SESSION_ID_START: usize = 10;
    lazy_static! {
        pub static ref REGEX_SESSIONS_SESSION_ID_START: regex::Regex =
            #[allow(clippy::invalid_regex)]
            regex::Regex::new(r"^/sessions/(?P<session_id>[^/?#]*)/start$")
                .expect("Unable to create regex for SESSIONS_SESSION_ID_START");
    }
    pub(crate) static ID_SHUTDOWN: usize = 11;
    pub(crate) static ID_STATUS: usize = 12;
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

impl<T, C> Clone for MakeService<T, C>
where
    T: Api<C> + Clone + Send + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            api_impl: self.api_impl.clone(),
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

    fn call(&self, target: Target) -> Self::Future {
        let service = Service::new(self.api_impl.clone());

        future::ok(service)
    }
}

fn method_not_allowed() -> Result<Response<BoxBody<Bytes, Infallible>>, crate::ServiceError> {
    Ok(Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(BoxBody::new(http_body_util::Empty::new()))
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

#[allow(dead_code)]
fn body_from_string(s: String) -> BoxBody<Bytes, Infallible> {
    BoxBody::new(Full::new(Bytes::from(s)))
}

fn body_from_str(s: &str) -> BoxBody<Bytes, Infallible> {
    BoxBody::new(Full::new(Bytes::copy_from_slice(s.as_bytes())))
}

impl<T, C, ReqBody> hyper::service::Service<(Request<ReqBody>, C)> for Service<T, C>
where
    T: Api<C> + Clone + Send + Sync + 'static,
    C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
    ReqBody: Body + Send + 'static,
    ReqBody::Error: Into<Box<dyn Error + Send + Sync>> + Send,
    ReqBody::Data: Send,
{
    type Response = Response<BoxBody<Bytes, Infallible>>;
    type Error = crate::ServiceError;
    type Future = ServiceFuture;

    fn call(&self, req: (Request<ReqBody>, C)) -> Self::Future {
        async fn run<T, C, ReqBody>(
            mut api_impl: T,
            req: (Request<ReqBody>, C),
        ) -> Result<Response<BoxBody<Bytes, Infallible>>, crate::ServiceError>
        where
            T: Api<C> + Clone + Send + 'static,
            C: Has<XSpanIdString> + Has<Option<Authorization>> + Send + Sync + 'static,
            ReqBody: Body + Send + 'static,
            ReqBody::Error: Into<Box<dyn Error + Send + Sync>> + Send,
            ReqBody::Data: Send,
        {
            let (request, context) = req;
            let (parts, body) = request.into_parts();
            let (method, uri, headers) = (parts.method, parts.uri, parts.headers);
            let path = paths::GLOBAL_REGEX_SET.matches(uri.path());

            match method {
                // ClientHeartbeat - POST /client_heartbeat
                hyper::Method::POST if path.matched(paths::ID_CLIENT_HEARTBEAT) => {
                    // Handle body parameters (note that non-required body parameters will ignore garbage
                    // values, rather than causing a 400 response). Produce warning header and logs for
                    // any unused fields.
                    let result = http_body_util::BodyExt::collect(body)
                        .await
                        .map(|f| f.to_bytes().to_vec());
                    match result {
                        Ok(body) => {
                            let mut unused_elements: Vec<String> = vec![];
                            let param_client_heartbeat: Option<models::ClientHeartbeat> = if !body
                                .is_empty()
                            {
                                let deserializer = &mut serde_json::Deserializer::from_slice(&body);
                                match serde_ignored::deserialize(deserializer, |path| {
                                            warn!("Ignoring unknown field in body: {path}");
                                            unused_elements.push(path.to_string());
                                    }) {
                                        Ok(param_client_heartbeat) => param_client_heartbeat,
                                        Err(e) => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new(format!("Couldn't parse body parameter ClientHeartbeat - doesn't match schema: {e}")))
                                                        .expect("Unable to create Bad Request response for invalid body parameter ClientHeartbeat due to schema")),
                                    }
                            } else {
                                None
                            };
                            let param_client_heartbeat = match param_client_heartbeat {
                                    Some(param_client_heartbeat) => param_client_heartbeat,
                                    None => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new("Missing required body parameter ClientHeartbeat".to_string()))
                                                        .expect("Unable to create Bad Request response for missing body parameter ClientHeartbeat")),
                                };

                            let result = api_impl
                                .client_heartbeat(param_client_heartbeat, &context)
                                .await;
                            let mut response =
                                Response::new(BoxBody::new(http_body_util::Empty::new()));
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

                            if !unused_elements.is_empty() {
                                response.headers_mut().insert(
                                    HeaderName::from_static("warning"),
                                    HeaderValue::from_str(
                                        format!(
                                            "Ignoring unknown fields in body: {unused_elements:?}"
                                        )
                                        .as_str(),
                                    )
                                    .expect("Unable to create Warning header value"),
                                );
                            }
                            match result {
                                Ok(rsp) => match rsp {
                                    ClientHeartbeatResponse::HeartbeatReceived(body) => {
                                        *response.status_mut() = StatusCode::from_u16(200)
                                            .expect("Unable to turn 200 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                },
                                Err(_) => {
                                    // Application code returned an error. This should not happen, as the implementation should
                                    // return a valid response.
                                    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                    *response.body_mut() =
                                        body_from_str("An internal error occurred");
                                }
                            }

                            Ok(response)
                        }
                        Err(e) => Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(body_from_string(format!(
                                "Unable to read body: {}",
                                e.into()
                            )))
                            .expect(
                                "Unable to create Bad Request response due to unable to read body",
                            )),
                    }
                }

                // GetServerConfiguration - GET /server_configuration
                hyper::Method::GET if path.matched(paths::ID_SERVER_CONFIGURATION) => {
                    let result = api_impl.get_server_configuration(&context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                            GetServerConfigurationResponse::TheCurrentServerConfiguration(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            GetServerConfigurationResponse::FailedToGetConfiguration(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // ListSessions - GET /sessions
                hyper::Method::GET if path.matched(paths::ID_SESSIONS) => {
                    let result = api_impl.list_sessions(&context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // NewSession - PUT /sessions
                hyper::Method::PUT if path.matched(paths::ID_SESSIONS) => {
                    // Handle body parameters (note that non-required body parameters will ignore garbage
                    // values, rather than causing a 400 response). Produce warning header and logs for
                    // any unused fields.
                    let result = http_body_util::BodyExt::collect(body)
                        .await
                        .map(|f| f.to_bytes().to_vec());
                    match result {
                        Ok(body) => {
                            let mut unused_elements: Vec<String> = vec![];
                            let param_new_session: Option<models::NewSession> = if !body.is_empty()
                            {
                                let deserializer = &mut serde_json::Deserializer::from_slice(&body);
                                match serde_ignored::deserialize(deserializer, |path| {
                                            warn!("Ignoring unknown field in body: {path}");
                                            unused_elements.push(path.to_string());
                                    }) {
                                        Ok(param_new_session) => param_new_session,
                                        Err(e) => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new(format!("Couldn't parse body parameter NewSession - doesn't match schema: {e}")))
                                                        .expect("Unable to create Bad Request response for invalid body parameter NewSession due to schema")),
                                    }
                            } else {
                                None
                            };
                            let param_new_session = match param_new_session {
                                    Some(param_new_session) => param_new_session,
                                    None => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new("Missing required body parameter NewSession".to_string()))
                                                        .expect("Unable to create Bad Request response for missing body parameter NewSession")),
                                };

                            let result = api_impl.new_session(param_new_session, &context).await;
                            let mut response =
                                Response::new(BoxBody::new(http_body_util::Empty::new()));
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

                            if !unused_elements.is_empty() {
                                response.headers_mut().insert(
                                    HeaderName::from_static("warning"),
                                    HeaderValue::from_str(
                                        format!(
                                            "Ignoring unknown fields in body: {unused_elements:?}"
                                        )
                                        .as_str(),
                                    )
                                    .expect("Unable to create Warning header value"),
                                );
                            }
                            match result {
                                Ok(rsp) => match rsp {
                                    NewSessionResponse::TheSessionID(body) => {
                                        *response.status_mut() = StatusCode::from_u16(200)
                                            .expect("Unable to turn 200 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                    NewSessionResponse::InvalidRequest(body) => {
                                        *response.status_mut() = StatusCode::from_u16(400)
                                            .expect("Unable to turn 400 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                    NewSessionResponse::Unauthorized => {
                                        *response.status_mut() = StatusCode::from_u16(401)
                                            .expect("Unable to turn 401 into a StatusCode");
                                    }
                                },
                                Err(_) => {
                                    // Application code returned an error. This should not happen, as the implementation should
                                    // return a valid response.
                                    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                    *response.body_mut() =
                                        body_from_str("An internal error occurred");
                                }
                            }

                            Ok(response)
                        }
                        Err(e) => Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(body_from_string(format!(
                                "Unable to read body: {}",
                                e.into()
                            )))
                            .expect(
                                "Unable to create Bad Request response due to unable to read body",
                            )),
                    }
                }

                // ServerStatus - GET /status
                hyper::Method::GET if path.matched(paths::ID_STATUS) => {
                    let result = api_impl.server_status(&context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                            ServerStatusResponse::ServerStatusAndInformation(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            ServerStatusResponse::Error(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // SetServerConfiguration - POST /server_configuration
                hyper::Method::POST if path.matched(paths::ID_SERVER_CONFIGURATION) => {
                    // Handle body parameters (note that non-required body parameters will ignore garbage
                    // values, rather than causing a 400 response). Produce warning header and logs for
                    // any unused fields.
                    let result = http_body_util::BodyExt::collect(body)
                        .await
                        .map(|f| f.to_bytes().to_vec());
                    match result {
                        Ok(body) => {
                            let mut unused_elements: Vec<String> = vec![];
                            let param_server_configuration: Option<models::ServerConfiguration> =
                                if !body.is_empty() {
                                    let deserializer =
                                        &mut serde_json::Deserializer::from_slice(&body);
                                    match serde_ignored::deserialize(deserializer, |path| {
                                            warn!("Ignoring unknown field in body: {path}");
                                            unused_elements.push(path.to_string());
                                    }) {
                                        Ok(param_server_configuration) => param_server_configuration,
                                        Err(e) => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new(format!("Couldn't parse body parameter ServerConfiguration - doesn't match schema: {e}")))
                                                        .expect("Unable to create Bad Request response for invalid body parameter ServerConfiguration due to schema")),
                                    }
                                } else {
                                    None
                                };
                            let param_server_configuration = match param_server_configuration {
                                    Some(param_server_configuration) => param_server_configuration,
                                    None => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new("Missing required body parameter ServerConfiguration".to_string()))
                                                        .expect("Unable to create Bad Request response for missing body parameter ServerConfiguration")),
                                };

                            let result = api_impl
                                .set_server_configuration(param_server_configuration, &context)
                                .await;
                            let mut response =
                                Response::new(BoxBody::new(http_body_util::Empty::new()));
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

                            if !unused_elements.is_empty() {
                                response.headers_mut().insert(
                                    HeaderName::from_static("warning"),
                                    HeaderValue::from_str(
                                        format!(
                                            "Ignoring unknown fields in body: {unused_elements:?}"
                                        )
                                        .as_str(),
                                    )
                                    .expect("Unable to create Warning header value"),
                                );
                            }
                            match result {
                                Ok(rsp) => match rsp {
                                    SetServerConfigurationResponse::ConfigurationUpdated(body) => {
                                        *response.status_mut() = StatusCode::from_u16(200)
                                            .expect("Unable to turn 200 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                    SetServerConfigurationResponse::Error(body) => {
                                        *response.status_mut() = StatusCode::from_u16(400)
                                            .expect("Unable to turn 400 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                },
                                Err(_) => {
                                    // Application code returned an error. This should not happen, as the implementation should
                                    // return a valid response.
                                    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                    *response.body_mut() =
                                        body_from_str("An internal error occurred");
                                }
                            }

                            Ok(response)
                        }
                        Err(e) => Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(body_from_string(format!(
                                "Unable to read body: {}",
                                e.into()
                            )))
                            .expect(
                                "Unable to create Bad Request response due to unable to read body",
                            )),
                    }
                }

                // ShutdownServer - POST /shutdown
                hyper::Method::POST if path.matched(paths::ID_SHUTDOWN) => {
                    let result = api_impl.shutdown_server(&context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            ShutdownServerResponse::ShutdownFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            ShutdownServerResponse::Unauthorized => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // AdoptSession - PUT /sessions/{session_id}/adopt
                hyper::Method::PUT if path.matched(paths::ID_SESSIONS_SESSION_ID_ADOPT) => {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID_ADOPT
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID_ADOPT in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID_ADOPT.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    // Handle body parameters (note that non-required body parameters will ignore garbage
                    // values, rather than causing a 400 response). Produce warning header and logs for
                    // any unused fields.
                    let result = http_body_util::BodyExt::collect(body)
                        .await
                        .map(|f| f.to_bytes().to_vec());
                    match result {
                        Ok(body) => {
                            let mut unused_elements: Vec<String> = vec![];
                            let param_connection_info: Option<models::ConnectionInfo> = if !body
                                .is_empty()
                            {
                                let deserializer = &mut serde_json::Deserializer::from_slice(&body);
                                match serde_ignored::deserialize(deserializer, |path| {
                                            warn!("Ignoring unknown field in body: {path}");
                                            unused_elements.push(path.to_string());
                                    }) {
                                        Ok(param_connection_info) => param_connection_info,
                                        Err(e) => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new(format!("Couldn't parse body parameter ConnectionInfo - doesn't match schema: {e}")))
                                                        .expect("Unable to create Bad Request response for invalid body parameter ConnectionInfo due to schema")),
                                    }
                            } else {
                                None
                            };
                            let param_connection_info = match param_connection_info {
                                    Some(param_connection_info) => param_connection_info,
                                    None => return Ok(Response::builder()
                                                        .status(StatusCode::BAD_REQUEST)
                                                        .body(BoxBody::new("Missing required body parameter ConnectionInfo".to_string()))
                                                        .expect("Unable to create Bad Request response for missing body parameter ConnectionInfo")),
                                };

                            let result = api_impl
                                .adopt_session(param_session_id, param_connection_info, &context)
                                .await;
                            let mut response =
                                Response::new(BoxBody::new(http_body_util::Empty::new()));
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

                            if !unused_elements.is_empty() {
                                response.headers_mut().insert(
                                    HeaderName::from_static("warning"),
                                    HeaderValue::from_str(
                                        format!(
                                            "Ignoring unknown fields in body: {unused_elements:?}"
                                        )
                                        .as_str(),
                                    )
                                    .expect("Unable to create Warning header value"),
                                );
                            }
                            match result {
                                Ok(rsp) => match rsp {
                                    AdoptSessionResponse::Adopted(body) => {
                                        *response.status_mut() = StatusCode::from_u16(200)
                                            .expect("Unable to turn 200 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                    AdoptSessionResponse::AdoptionFailed(body) => {
                                        *response.status_mut() = StatusCode::from_u16(500)
                                            .expect("Unable to turn 500 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                    AdoptSessionResponse::SessionNotFound => {
                                        *response.status_mut() = StatusCode::from_u16(404)
                                            .expect("Unable to turn 404 into a StatusCode");
                                    }
                                    AdoptSessionResponse::Unauthorized => {
                                        *response.status_mut() = StatusCode::from_u16(401)
                                            .expect("Unable to turn 401 into a StatusCode");
                                    }
                                },
                                Err(_) => {
                                    // Application code returned an error. This should not happen, as the implementation should
                                    // return a valid response.
                                    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                    *response.body_mut() =
                                        body_from_str("An internal error occurred");
                                }
                            }

                            Ok(response)
                        }
                        Err(e) => Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(body_from_string(format!(
                                "Unable to read body: {}",
                                e.into()
                            )))
                            .expect(
                                "Unable to create Bad Request response due to unable to read body",
                            )),
                    }
                }

                // ChannelsUpgrade - GET /sessions/{session_id}/channels
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
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.channels_upgrade(param_session_id, &context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                            ChannelsUpgradeResponse::UpgradedConnection(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            ChannelsUpgradeResponse::InvalidRequest(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            ChannelsUpgradeResponse::Unauthorized => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                            ChannelsUpgradeResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                // ConnectionInfo - GET /sessions/{session_id}/connection_info
                hyper::Method::GET
                    if path.matched(paths::ID_SESSIONS_SESSION_ID_CONNECTION_INFO) =>
                {
                    // Path parameters
                    let path: &str = uri.path();
                    let path_params =
                    paths::REGEX_SESSIONS_SESSION_ID_CONNECTION_INFO
                    .captures(path)
                    .unwrap_or_else(||
                        panic!("Path {} matched RE SESSIONS_SESSION_ID_CONNECTION_INFO in set but failed match against \"{}\"", path, paths::REGEX_SESSIONS_SESSION_ID_CONNECTION_INFO.as_str())
                    );

                    let param_session_id = match percent_encoding::percent_decode(path_params["session_id"].as_bytes()).decode_utf8() {
                    Ok(param_session_id) => match param_session_id.parse::<String>() {
                        Ok(param_session_id) => param_session_id,
                        Err(e) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.connection_info(param_session_id, &context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                            ConnectionInfoResponse::ConnectionInfo(body) => {
                                *response.status_mut() = StatusCode::from_u16(200)
                                    .expect("Unable to turn 200 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            ConnectionInfoResponse::Failed(body) => {
                                *response.status_mut() = StatusCode::from_u16(500)
                                    .expect("Unable to turn 500 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            ConnectionInfoResponse::Unauthorized => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                            ConnectionInfoResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

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
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.delete_session(param_session_id, &context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            DeleteSessionResponse::FailedToDeleteSession(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            DeleteSessionResponse::Unauthorized => {
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
                            *response.body_mut() = body_from_str("An internal error occurred");
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
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.get_session(param_session_id, &context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            GetSessionResponse::FailedToGetSession(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
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
                            *response.body_mut() = body_from_str("An internal error occurred");
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
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.interrupt_session(param_session_id, &context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            InterruptSessionResponse::InterruptFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            InterruptSessionResponse::Unauthorized => {
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
                            *response.body_mut() = body_from_str("An internal error occurred");
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
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.kill_session(param_session_id, &context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            KillSessionResponse::KillFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(400)
                                    .expect("Unable to turn 400 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            KillSessionResponse::Unauthorized => {
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
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

                    Ok(response)
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
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    // Handle body parameters (note that non-required body parameters will ignore garbage
                    // values, rather than causing a 400 response). Produce warning header and logs for
                    // any unused fields.
                    let result = http_body_util::BodyExt::collect(body)
                        .await
                        .map(|f| f.to_bytes().to_vec());
                    match result {
                        Ok(body) => {
                            let mut unused_elements: Vec<String> = vec![];
                            let param_restart_session: Option<models::RestartSession> = if !body
                                .is_empty()
                            {
                                let deserializer = &mut serde_json::Deserializer::from_slice(&body);
                                serde_ignored::deserialize(deserializer, |path| {
                                    warn!("Ignoring unknown field in body: {path}");
                                    unused_elements.push(path.to_string());
                                })
                                .unwrap_or_default()
                            } else {
                                None
                            };

                            let result = api_impl
                                .restart_session(param_session_id, param_restart_session, &context)
                                .await;
                            let mut response =
                                Response::new(BoxBody::new(http_body_util::Empty::new()));
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

                            if !unused_elements.is_empty() {
                                response.headers_mut().insert(
                                    HeaderName::from_static("warning"),
                                    HeaderValue::from_str(
                                        format!(
                                            "Ignoring unknown fields in body: {unused_elements:?}"
                                        )
                                        .as_str(),
                                    )
                                    .expect("Unable to create Warning header value"),
                                );
                            }
                            match result {
                                Ok(rsp) => match rsp {
                                    RestartSessionResponse::Restarted(body) => {
                                        *response.status_mut() = StatusCode::from_u16(200)
                                            .expect("Unable to turn 200 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                    RestartSessionResponse::RestartFailed(body) => {
                                        *response.status_mut() = StatusCode::from_u16(500)
                                            .expect("Unable to turn 500 into a StatusCode");
                                        response.headers_mut().insert(
                                            CONTENT_TYPE,
                                            HeaderValue::from_static("application/json"),
                                        );
                                        // JSON Body
                                        let body = serde_json::to_string(&body)
                                            .expect("impossible to fail to serialize");
                                        *response.body_mut() = body_from_string(body);
                                    }
                                    RestartSessionResponse::Unauthorized => {
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
                                    *response.body_mut() =
                                        body_from_str("An internal error occurred");
                                }
                            }

                            Ok(response)
                        }
                        Err(e) => Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(body_from_string(format!(
                                "Unable to read body: {}",
                                e.into()
                            )))
                            .expect(
                                "Unable to create Bad Request response due to unable to read body",
                            )),
                    }
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
                                        .body(body_from_string(format!("Couldn't parse path parameter session_id: {e}")))
                                        .expect("Unable to create Bad Request response for invalid path parameter")),
                    },
                    Err(_) => return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_string(format!("Couldn't percent-decode path parameter as UTF-8: {}", &path_params["session_id"])))
                                        .expect("Unable to create Bad Request response for invalid percent decode"))
                };

                    let result = api_impl.start_session(param_session_id, &context).await;
                    let mut response = Response::new(BoxBody::new(http_body_util::Empty::new()));
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
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            StartSessionResponse::StartFailed(body) => {
                                *response.status_mut() = StatusCode::from_u16(500)
                                    .expect("Unable to turn 500 into a StatusCode");
                                response.headers_mut().insert(
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                // JSON Body
                                let body = serde_json::to_string(&body)
                                    .expect("impossible to fail to serialize");
                                *response.body_mut() = body_from_string(body);
                            }
                            StartSessionResponse::SessionNotFound => {
                                *response.status_mut() = StatusCode::from_u16(404)
                                    .expect("Unable to turn 404 into a StatusCode");
                            }
                            StartSessionResponse::Unauthorized => {
                                *response.status_mut() = StatusCode::from_u16(401)
                                    .expect("Unable to turn 401 into a StatusCode");
                            }
                        },
                        Err(_) => {
                            // Application code returned an error. This should not happen, as the implementation should
                            // return a valid response.
                            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                            *response.body_mut() = body_from_str("An internal error occurred");
                        }
                    }

                    Ok(response)
                }

                _ if path.matched(paths::ID_CLIENT_HEARTBEAT) => method_not_allowed(),
                _ if path.matched(paths::ID_SERVER_CONFIGURATION) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_ADOPT) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_CHANNELS) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_CONNECTION_INFO) => {
                    method_not_allowed()
                }
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_INTERRUPT) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_KILL) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_RESTART) => method_not_allowed(),
                _ if path.matched(paths::ID_SESSIONS_SESSION_ID_START) => method_not_allowed(),
                _ if path.matched(paths::ID_SHUTDOWN) => method_not_allowed(),
                _ if path.matched(paths::ID_STATUS) => method_not_allowed(),
                _ => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(BoxBody::new(http_body_util::Empty::new()))
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
            // ClientHeartbeat - POST /client_heartbeat
            hyper::Method::POST if path.matched(paths::ID_CLIENT_HEARTBEAT) => {
                Some("ClientHeartbeat")
            }
            // GetServerConfiguration - GET /server_configuration
            hyper::Method::GET if path.matched(paths::ID_SERVER_CONFIGURATION) => {
                Some("GetServerConfiguration")
            }
            // ListSessions - GET /sessions
            hyper::Method::GET if path.matched(paths::ID_SESSIONS) => Some("ListSessions"),
            // NewSession - PUT /sessions
            hyper::Method::PUT if path.matched(paths::ID_SESSIONS) => Some("NewSession"),
            // ServerStatus - GET /status
            hyper::Method::GET if path.matched(paths::ID_STATUS) => Some("ServerStatus"),
            // SetServerConfiguration - POST /server_configuration
            hyper::Method::POST if path.matched(paths::ID_SERVER_CONFIGURATION) => {
                Some("SetServerConfiguration")
            }
            // ShutdownServer - POST /shutdown
            hyper::Method::POST if path.matched(paths::ID_SHUTDOWN) => Some("ShutdownServer"),
            // AdoptSession - PUT /sessions/{session_id}/adopt
            hyper::Method::PUT if path.matched(paths::ID_SESSIONS_SESSION_ID_ADOPT) => {
                Some("AdoptSession")
            }
            // ChannelsUpgrade - GET /sessions/{session_id}/channels
            hyper::Method::GET if path.matched(paths::ID_SESSIONS_SESSION_ID_CHANNELS) => {
                Some("ChannelsUpgrade")
            }
            // ConnectionInfo - GET /sessions/{session_id}/connection_info
            hyper::Method::GET if path.matched(paths::ID_SESSIONS_SESSION_ID_CONNECTION_INFO) => {
                Some("ConnectionInfo")
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
            // RestartSession - POST /sessions/{session_id}/restart
            hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_RESTART) => {
                Some("RestartSession")
            }
            // StartSession - POST /sessions/{session_id}/start
            hyper::Method::POST if path.matched(paths::ID_SESSIONS_SESSION_ID_START) => {
                Some("StartSession")
            }
            _ => None,
        }
    }
}
