use async_trait::async_trait;
use bytes::Bytes;
use futures::{
    future, future::BoxFuture, future::FutureExt, future::TryFutureExt, stream, stream::StreamExt,
    Stream,
};
use http_body_util::{combinators::BoxBody, Full};
use hyper::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use hyper::{
    body::{Body, Incoming},
    service::Service,
    Request, Response, Uri,
};
use percent_encoding::{utf8_percent_encode, AsciiSet};
use std::borrow::Cow;
use std::convert::{Infallible, TryInto};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::io::{ErrorKind, Read};
use std::marker::PhantomData;
use std::path::Path;
use std::str;
use std::str::FromStr;
use std::string::ToString;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use swagger::{ApiError, AuthData, BodyExt, Connector, DropContextService, Has, XSpanIdString};
use tower_service::Service as _;
use url::form_urlencoded;

use crate::header;
use crate::models;

/// https://url.spec.whatwg.org/#fragment-percent-encode-set
#[allow(dead_code)]
const FRAGMENT_ENCODE_SET: &AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

/// This encode set is used for object IDs
///
/// Aside from the special characters defined in the `PATH_SEGMENT_ENCODE_SET`,
/// the vertical bar (|) is encoded.
#[allow(dead_code)]
const ID_ENCODE_SET: &AsciiSet = &FRAGMENT_ENCODE_SET.add(b'|');

use crate::{
    AdoptSessionResponse, Api, ChannelsUpgradeResponse, ClientHeartbeatResponse,
    ConnectionInfoResponse, DeleteSessionResponse, GetServerConfigurationResponse,
    GetSessionResponse, InterruptSessionResponse, KillSessionResponse, ListSessionsResponse,
    NewSessionResponse, RestartSessionResponse, ServerStatusResponse,
    SetServerConfigurationResponse, ShutdownServerResponse, StartSessionResponse,
};

/// Convert input into a base path, e.g. "http://example:123". Also checks the scheme as it goes.
fn into_base_path(
    input: impl TryInto<Uri, Error = hyper::http::uri::InvalidUri>,
    correct_scheme: Option<&'static str>,
) -> Result<String, ClientInitError> {
    // First convert to Uri, since a base path is a subset of Uri.
    let uri = input.try_into()?;

    let scheme = uri.scheme_str().ok_or(ClientInitError::InvalidScheme)?;

    // Check the scheme if necessary
    if let Some(correct_scheme) = correct_scheme {
        if scheme != correct_scheme {
            return Err(ClientInitError::InvalidScheme);
        }
    }

    let host = uri.host().ok_or(ClientInitError::MissingHost)?;
    let port = uri.port_u16().map(|x| format!(":{x}")).unwrap_or_default();
    Ok(format!(
        "{scheme}://{host}{port}{}",
        uri.path().trim_end_matches('/')
    ))
}

/// A client that implements the API by making HTTP calls out to a server.
pub struct Client<S, C>
where
    S: Service<(Request<BoxBody<Bytes, Infallible>>, C)> + Clone + Sync + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<crate::ServiceError> + fmt::Display,
    C: Clone + Send + Sync + 'static,
{
    /// Inner service
    client_service: S,

    /// Base path of the API
    base_path: String,

    /// Marker
    marker: PhantomData<fn(C)>,
}

impl<S, C> fmt::Debug for Client<S, C>
where
    S: Service<(Request<BoxBody<Bytes, Infallible>>, C)> + Clone + Sync + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<crate::ServiceError> + fmt::Display,
    C: Clone + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Client {{ base_path: {} }}", self.base_path)
    }
}

impl<S, C> Clone for Client<S, C>
where
    S: Service<(Request<BoxBody<Bytes, Infallible>>, C)> + Clone + Sync + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<crate::ServiceError> + fmt::Display,
    C: Clone + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            client_service: self.client_service.clone(),
            base_path: self.base_path.clone(),
            marker: PhantomData,
        }
    }
}

impl<Connector, C>
    Client<
        DropContextService<
            hyper_util::service::TowerToHyperService<
                hyper_util::client::legacy::Client<Connector, BoxBody<Bytes, Infallible>>,
            >,
            C,
        >,
        C,
    >
where
    Connector: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
    C: Clone + Send + Sync + 'static,
{
    /// Create a client with a custom implementation of hyper::client::Connect.
    ///
    /// Intended for use with custom implementations of connect for e.g. protocol logging
    /// or similar functionality which requires wrapping the transport layer. When wrapping a TCP connection,
    /// this function should be used in conjunction with `swagger::Connector::builder()`.
    ///
    /// For ordinary tcp connections, prefer the use of `try_new_http`, `try_new_https`
    /// and `try_new_https_mutual`, to avoid introducing a dependency on the underlying transport layer.
    ///
    /// # Arguments
    ///
    /// * `base_path` - base path of the client API, i.e. "<http://www.my-api-implementation.com>"
    /// * `protocol` - Which protocol to use when constructing the request url, e.g. `Some("http")`
    /// * `connector` - Implementation of `hyper::client::Connect` to use for the client
    pub fn try_new_with_connector(
        base_path: &str,
        protocol: Option<&'static str>,
        connector: Connector,
    ) -> Result<Self, ClientInitError> {
        let client_service =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(connector);
        let client_service = DropContextService::new(
            hyper_util::service::TowerToHyperService::new(client_service),
        );

        Ok(Self {
            client_service,
            base_path: into_base_path(base_path, protocol)?,
            marker: PhantomData,
        })
    }
}

#[derive(Debug, Clone)]
pub enum HyperClient {
    Http(
        hyper_util::client::legacy::Client<
            hyper_util::client::legacy::connect::HttpConnector,
            BoxBody<Bytes, Infallible>,
        >,
    ),
}

impl Service<Request<BoxBody<Bytes, Infallible>>> for HyperClient {
    type Response = Response<Incoming>;
    type Error = hyper_util::client::legacy::Error;
    type Future = hyper_util::client::legacy::ResponseFuture;

    fn call(&self, req: Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        match self {
            HyperClient::Http(client) => client.request(req),
        }
    }
}

impl<C> Client<DropContextService<HyperClient, C>, C>
where
    C: Clone + Send + Sync + 'static,
{
    /// Create an HTTP client.
    ///
    /// # Arguments
    /// * `base_path` - base path of the client API, i.e. "<http://www.my-api-implementation.com>"
    pub fn try_new(base_path: &str) -> Result<Self, ClientInitError> {
        let uri = Uri::from_str(base_path)?;

        let scheme = uri.scheme_str().ok_or(ClientInitError::InvalidScheme)?;
        let scheme = scheme.to_ascii_lowercase();

        let connector = Connector::builder();

        let client_service = match scheme.as_str() {
            "http" => HyperClient::Http(
                hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                    .build(connector.build()),
            ),
            // "https" => { // HTTPS support removed
            //                 let connector = connector.https()
            //                    .build()
            //                    .map_err(ClientInitError::SslError)?;
            //             },
            _ => {
                return Err(ClientInitError::InvalidScheme);
            }
        };

        let client_service = DropContextService::new(client_service);

        Ok(Self {
            client_service,
            base_path: into_base_path(base_path, None)?,
            marker: PhantomData,
        })
    }
}

impl<C>
    Client<
        DropContextService<
            hyper_util::service::TowerToHyperService<
                hyper_util::client::legacy::Client<
                    hyper_util::client::legacy::connect::HttpConnector,
                    BoxBody<Bytes, Infallible>,
                >,
            >,
            C,
        >,
        C,
    >
where
    C: Clone + Send + Sync + 'static,
{
    /// Create an HTTP client.
    ///
    /// # Arguments
    /// * `base_path` - base path of the client API, i.e. "<http://www.my-api-implementation.com>"
    pub fn try_new_http(base_path: &str) -> Result<Self, ClientInitError> {
        let http_connector = Connector::builder().build();

        Self::try_new_with_connector(base_path, Some("http"), http_connector)
    }
}

impl<S, C> Client<S, C>
where
    S: Service<(Request<BoxBody<Bytes, Infallible>>, C)> + Clone + Sync + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<crate::ServiceError> + fmt::Display,
    C: Clone + Send + Sync + 'static,
{
    /// Constructor for creating a `Client` by passing in a pre-made `hyper::service::Service` /
    /// `tower::Service`
    ///
    /// This allows adding custom wrappers around the underlying transport, for example for logging.
    pub fn try_new_with_client_service(
        client_service: S,
        base_path: &str,
    ) -> Result<Self, ClientInitError> {
        Ok(Self {
            client_service,
            base_path: into_base_path(base_path, None)?,
            marker: PhantomData,
        })
    }
}

/// Error type failing to create a Client
#[derive(Debug)]
pub enum ClientInitError {
    /// Invalid URL Scheme
    InvalidScheme,

    /// Invalid URI
    InvalidUri(hyper::http::uri::InvalidUri),

    /// Missing Hostname
    MissingHost,
}

impl From<hyper::http::uri::InvalidUri> for ClientInitError {
    fn from(err: hyper::http::uri::InvalidUri) -> ClientInitError {
        ClientInitError::InvalidUri(err)
    }
}

impl fmt::Display for ClientInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s: &dyn fmt::Debug = self;
        s.fmt(f)
    }
}

impl Error for ClientInitError {
    fn description(&self) -> &str {
        "Failed to produce a hyper client."
    }
}

#[allow(dead_code)]
fn body_from_string(s: String) -> BoxBody<Bytes, Infallible> {
    BoxBody::new(Full::new(Bytes::from(s)))
}

#[async_trait]
impl<S, C, B> Api<C> for Client<S, C>
where
    S: Service<(Request<BoxBody<Bytes, Infallible>>, C), Response = Response<B>>
        + Clone
        + Sync
        + Send
        + 'static,
    S::Future: Send + 'static,
    S::Error: Into<crate::ServiceError> + fmt::Display,
    C: Has<XSpanIdString> + Has<Option<AuthData>> + Clone + Send + Sync + 'static,
    B: hyper::body::Body + Send + 'static + Unpin,
    B::Data: Send,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
{
    #[allow(clippy::vec_init_then_push)]
    async fn client_heartbeat(
        &self,
        param_client_heartbeat: models::ClientHeartbeat,
        context: &C,
    ) -> Result<ClientHeartbeatResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!("{}/client_heartbeat", self.base_path);

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("POST")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        // Consumes basic body
        // Body parameter
        let body = serde_json::to_string(&param_client_heartbeat)
            .expect("impossible to fail to serialize");
        *request.body_mut() = body_from_string(body);

        let header = "application/json";
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(header));

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ClientHeartbeatResponse::HeartbeatReceived(body))
            }
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn get_server_configuration(
        &self,
        context: &C,
    ) -> Result<GetServerConfigurationResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!("{}/server_configuration", self.base_path);

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("GET")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body =
                    serde_json::from_str::<models::ServerConfiguration>(body).map_err(|e| {
                        ApiError(format!("Response body did not match the schema: {e}"))
                    })?;

                Ok(GetServerConfigurationResponse::TheCurrentServerConfiguration(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(GetServerConfigurationResponse::FailedToGetConfiguration(
                    body,
                ))
            }
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!("{}/sessions", self.base_path);

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("GET")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::SessionList>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ListSessionsResponse::ListOfActiveSessions(body))
            }
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn new_session(
        &self,
        param_new_session: models::NewSession,
        context: &C,
    ) -> Result<NewSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!("{}/sessions", self.base_path);

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("PUT")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        // Consumes basic body
        // Body parameter
        let body =
            serde_json::to_string(&param_new_session).expect("impossible to fail to serialize");
        *request.body_mut() = body_from_string(body);

        let header = "application/json";
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(header));

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body =
                    serde_json::from_str::<models::NewSession200Response>(body).map_err(|e| {
                        ApiError(format!("Response body did not match the schema: {e}"))
                    })?;

                Ok(NewSessionResponse::TheSessionID(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(NewSessionResponse::InvalidRequest(body))
            }
            401 => Ok(NewSessionResponse::Unauthorized),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn server_status(&self, context: &C) -> Result<ServerStatusResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!("{}/status", self.base_path);

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("GET")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::ServerStatus>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ServerStatusResponse::ServerStatusAndInformation(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ServerStatusResponse::Error(body))
            }
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn set_server_configuration(
        &self,
        param_server_configuration: models::ServerConfiguration,
        context: &C,
    ) -> Result<SetServerConfigurationResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!("{}/server_configuration", self.base_path);

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("POST")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        // Consumes basic body
        // Body parameter
        let body = serde_json::to_string(&param_server_configuration)
            .expect("impossible to fail to serialize");
        *request.body_mut() = body_from_string(body);

        let header = "application/json";
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(header));

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(SetServerConfigurationResponse::ConfigurationUpdated(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(SetServerConfigurationResponse::Error(body))
            }
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn shutdown_server(&self, context: &C) -> Result<ShutdownServerResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!("{}/shutdown", self.base_path);

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("POST")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ShutdownServerResponse::ShuttingDown(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ShutdownServerResponse::ShutdownFailed(body))
            }
            401 => Ok(ShutdownServerResponse::Unauthorized),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn adopt_session(
        &self,
        param_session_id: String,
        param_connection_info: models::ConnectionInfo,
        context: &C,
    ) -> Result<AdoptSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}/adopt",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("PUT")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        // Consumes basic body
        // Body parameter
        let body =
            serde_json::to_string(&param_connection_info).expect("impossible to fail to serialize");
        *request.body_mut() = body_from_string(body);

        let header = "application/json";
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(header));

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(AdoptSessionResponse::Adopted(body))
            }
            500 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(AdoptSessionResponse::AdoptionFailed(body))
            }
            404 => Ok(AdoptSessionResponse::SessionNotFound),
            401 => Ok(AdoptSessionResponse::Unauthorized),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn channels_upgrade(
        &self,
        param_session_id: String,
        context: &C,
    ) -> Result<ChannelsUpgradeResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}/channels",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("GET")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<String>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ChannelsUpgradeResponse::UpgradedConnection(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ChannelsUpgradeResponse::InvalidRequest(body))
            }
            401 => Ok(ChannelsUpgradeResponse::Unauthorized),
            404 => Ok(ChannelsUpgradeResponse::SessionNotFound),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn connection_info(
        &self,
        param_session_id: String,
        context: &C,
    ) -> Result<ConnectionInfoResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}/connection_info",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("GET")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::ConnectionInfo>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ConnectionInfoResponse::ConnectionInfo(body))
            }
            500 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(ConnectionInfoResponse::Failed(body))
            }
            401 => Ok(ConnectionInfoResponse::Unauthorized),
            404 => Ok(ConnectionInfoResponse::SessionNotFound),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn delete_session(
        &self,
        param_session_id: String,
        context: &C,
    ) -> Result<DeleteSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(DeleteSessionResponse::SessionDeleted(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(DeleteSessionResponse::FailedToDeleteSession(body))
            }
            401 => Ok(DeleteSessionResponse::Unauthorized),
            404 => Ok(DeleteSessionResponse::SessionNotFound),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn get_session(
        &self,
        param_session_id: String,
        context: &C,
    ) -> Result<GetSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("GET")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::ActiveSession>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(GetSessionResponse::SessionDetails(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(GetSessionResponse::FailedToGetSession(body))
            }
            404 => Ok(GetSessionResponse::SessionNotFound),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn interrupt_session(
        &self,
        param_session_id: String,
        context: &C,
    ) -> Result<InterruptSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}/interrupt",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("POST")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(InterruptSessionResponse::Interrupted(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(InterruptSessionResponse::InterruptFailed(body))
            }
            401 => Ok(InterruptSessionResponse::Unauthorized),
            404 => Ok(InterruptSessionResponse::SessionNotFound),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn kill_session(
        &self,
        param_session_id: String,
        context: &C,
    ) -> Result<KillSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}/kill",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("POST")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(KillSessionResponse::Killed(body))
            }
            400 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::Error>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(KillSessionResponse::KillFailed(body))
            }
            401 => Ok(KillSessionResponse::Unauthorized),
            404 => Ok(KillSessionResponse::SessionNotFound),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn restart_session(
        &self,
        param_session_id: String,
        param_restart_session: Option<models::RestartSession>,
        context: &C,
    ) -> Result<RestartSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}/restart",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("POST")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        // Consumes basic body
        // Body parameter
        if let Some(param_restart_session) = param_restart_session {
            let body = serde_json::to_string(&param_restart_session)
                .expect("impossible to fail to serialize");
            *request.body_mut() = body_from_string(body);
        }

        let header = "application/json";
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(header));

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(RestartSessionResponse::Restarted(body))
            }
            500 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::StartupError>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(RestartSessionResponse::RestartFailed(body))
            }
            401 => Ok(RestartSessionResponse::Unauthorized),
            404 => Ok(RestartSessionResponse::SessionNotFound),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }

    #[allow(clippy::vec_init_then_push)]
    async fn start_session(
        &self,
        param_session_id: String,
        context: &C,
    ) -> Result<StartSessionResponse, ApiError> {
        let mut client_service = self.client_service.clone();
        #[allow(clippy::uninlined_format_args)]
        let mut uri = format!(
            "{}/sessions/{session_id}/start",
            self.base_path,
            session_id = utf8_percent_encode(&param_session_id.to_string(), ID_ENCODE_SET)
        );

        // Query parameters
        let query_string = {
            let mut query_string = form_urlencoded::Serializer::new("".to_owned());
            query_string.finish()
        };
        if !query_string.is_empty() {
            uri += "?";
            uri += &query_string;
        }

        let uri = match Uri::from_str(&uri) {
            Ok(uri) => uri,
            Err(err) => return Err(ApiError(format!("Unable to build URI: {err}"))),
        };

        let mut request = match Request::builder()
            .method("POST")
            .uri(uri)
            .body(BoxBody::new(http_body_util::Empty::new()))
        {
            Ok(req) => req,
            Err(e) => return Err(ApiError(format!("Unable to create request: {e}"))),
        };

        let header = HeaderValue::from_str(Has::<XSpanIdString>::get(context).0.as_str());
        request.headers_mut().insert(
            HeaderName::from_static("x-span-id"),
            match header {
                Ok(h) => h,
                Err(e) => {
                    return Err(ApiError(format!(
                        "Unable to create X-Span ID header value: {e}"
                    )))
                }
            },
        );

        let response = client_service
            .call((request, context.clone()))
            .map_err(|e| ApiError(format!("No response received: {e}")))
            .await?;

        match response.status().as_u16() {
            200 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(StartSessionResponse::Started(body))
            }
            500 => {
                let body = response.into_body();
                let body = http_body_util::BodyExt::collect(body)
                    .await
                    .map(|f| f.to_bytes().to_vec())
                    .map_err(|e| ApiError(format!("Failed to read response: {}", e.into())))?;

                let body = str::from_utf8(&body)
                    .map_err(|e| ApiError(format!("Response was not valid UTF8: {e}")))?;
                let body = serde_json::from_str::<models::StartupError>(body).map_err(|e| {
                    ApiError(format!("Response body did not match the schema: {e}"))
                })?;

                Ok(StartSessionResponse::StartFailed(body))
            }
            404 => Ok(StartSessionResponse::SessionNotFound),
            401 => Ok(StartSessionResponse::Unauthorized),
            code => {
                let headers = response.headers().clone();
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .map(|f| f.to_bytes().to_vec());
                Err(ApiError(format!(
                    "Unexpected response code {code}:\n{headers:?}\n\n{}",
                    match body {
                        Ok(body) => match String::from_utf8(body) {
                            Ok(body) => body,
                            Err(e) => format!("<Body was not UTF8: {e:?}>"),
                        },
                        Err(e) => format!(
                            "<Failed to read body: {}>",
                            Into::<crate::ServiceError>::into(e)
                        ),
                    }
                )))
            }
        }
    }
}
