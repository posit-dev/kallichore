#![allow(
    missing_docs,
    trivial_casts,
    unused_variables,
    unused_mut,
    unused_imports,
    unused_extern_crates,
    unused_attributes,
    non_camel_case_types
)]
#![allow(clippy::derive_partial_eq_without_eq, clippy::disallowed_names)]

use crate::server::Authorization;
use async_trait::async_trait;
use futures::Stream;
use hyper;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::error::Error;
use std::task::{Context, Poll};
use swagger::{ApiError, ContextWrapper};

type ServiceError = Box<dyn Error + Send + Sync + 'static>;

pub const BASE_PATH: &str = "";
pub const API_VERSION: &str = "1.0.0";

mod auth;
pub use auth::{AuthenticationApi, Claims};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientHeartbeatResponse {
    /// Heartbeat received
    HeartbeatReceived(serde_json::Value),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum GetServerConfigurationResponse {
    /// The current server configuration
    TheCurrentServerConfiguration(models::ServerConfiguration),
    /// Failed to get configuration
    FailedToGetConfiguration(models::Error),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum ListSessionsResponse {
    /// List of active sessions
    ListOfActiveSessions(models::SessionList),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum NewSessionResponse {
    /// The session ID
    TheSessionID(models::NewSession200Response),
    /// Invalid request
    InvalidRequest(models::Error),
    /// Unauthorized
    Unauthorized,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum ServerStatusResponse {
    /// Server status and information
    ServerStatusAndInformation(models::ServerStatus),
    /// Error
    Error(models::Error),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum SetServerConfigurationResponse {
    /// Configuration updated
    ConfigurationUpdated(serde_json::Value),
    /// Error
    Error(models::Error),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum ShutdownServerResponse {
    /// Shutting down
    ShuttingDown(serde_json::Value),
    /// Shutdown failed
    ShutdownFailed(models::Error),
    /// Unauthorized
    Unauthorized,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum AdoptSessionResponse {
    /// Adopted
    Adopted(serde_json::Value),
    /// Adoption failed
    AdoptionFailed(models::Error),
    /// Session not found
    SessionNotFound,
    /// Unauthorized
    Unauthorized,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum ChannelsWebsocketResponse {
    /// Upgrade connection to a websocket
    UpgradeConnectionToAWebsocket,
    /// Invalid request
    InvalidRequest(models::Error),
    /// Unauthorized
    Unauthorized,
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum ConnectionInfoResponse {
    /// Connection Info
    ConnectionInfo(models::ConnectionInfo),
    /// Failed
    Failed(models::Error),
    /// Unauthorized
    Unauthorized,
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum DeleteSessionResponse {
    /// Session deleted
    SessionDeleted(serde_json::Value),
    /// Failed to delete session
    FailedToDeleteSession(models::Error),
    /// Unauthorized
    Unauthorized,
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum GetSessionResponse {
    /// Session details
    SessionDetails(models::ActiveSession),
    /// Failed to get session
    FailedToGetSession(models::Error),
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum InterruptSessionResponse {
    /// Interrupted
    Interrupted(serde_json::Value),
    /// Interrupt failed
    InterruptFailed(models::Error),
    /// Unauthorized
    Unauthorized,
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum KillSessionResponse {
    /// Killed
    Killed(serde_json::Value),
    /// Kill failed
    KillFailed(models::Error),
    /// Unauthorized
    Unauthorized,
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum RestartSessionResponse {
    /// Restarted
    Restarted(serde_json::Value),
    /// Restart failed
    RestartFailed(models::StartupError),
    /// Unauthorized
    Unauthorized,
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum StartSessionResponse {
    /// Started
    Started(serde_json::Value),
    /// Start failed
    StartFailed(models::StartupError),
    /// Session not found
    SessionNotFound,
    /// Unauthorized
    Unauthorized,
}

/// API
#[async_trait]
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
pub trait Api<C: Send + Sync> {
    fn poll_ready(
        &self,
        _cx: &mut Context,
    ) -> Poll<Result<(), Box<dyn Error + Send + Sync + 'static>>> {
        Poll::Ready(Ok(()))
    }

    /// Notify the server that a client is connected
    async fn client_heartbeat(
        &self,
        client_heartbeat: models::ClientHeartbeat,
        context: &C,
    ) -> Result<ClientHeartbeatResponse, ApiError>;

    /// Get the server configuration
    async fn get_server_configuration(
        &self,
        context: &C,
    ) -> Result<GetServerConfigurationResponse, ApiError>;

    /// List active sessions
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError>;

    /// Create a new session
    async fn new_session(
        &self,
        new_session: models::NewSession,
        context: &C,
    ) -> Result<NewSessionResponse, ApiError>;

    /// Get server status and information
    async fn server_status(&self, context: &C) -> Result<ServerStatusResponse, ApiError>;

    /// Change the server configuration
    async fn set_server_configuration(
        &self,
        server_configuration: models::ServerConfiguration,
        context: &C,
    ) -> Result<SetServerConfigurationResponse, ApiError>;

    /// Shut down all sessions and the server itself
    async fn shutdown_server(&self, context: &C) -> Result<ShutdownServerResponse, ApiError>;

    /// Adopt an existing session
    async fn adopt_session(
        &self,
        session_id: String,
        connection_info: models::ConnectionInfo,
        context: &C,
    ) -> Result<AdoptSessionResponse, ApiError>;

    /// Upgrade to a WebSocket for channel communication
    async fn channels_websocket(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<ChannelsWebsocketResponse, ApiError>;

    /// Handle raw WebSocket upgrade request (optional override for websocket implementations)
    async fn channels_websocket_request(
        &self,
        request: hyper::Request<hyper::Body>,
        session_id: String,
        context: &C,
    ) -> Result<hyper::Response<hyper::Body>, ApiError> {
        // Default implementation: just return 501 Not Implemented
        Ok(hyper::Response::builder()
            .status(hyper::StatusCode::NOT_IMPLEMENTED)
            .body(hyper::Body::from("WebSocket upgrade not implemented"))
            .unwrap())
    }

    /// Get Jupyter connection information for the session
    async fn connection_info(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<ConnectionInfoResponse, ApiError>;

    /// Delete session
    async fn delete_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<DeleteSessionResponse, ApiError>;

    /// Get session details
    async fn get_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<GetSessionResponse, ApiError>;

    /// Interrupt session
    async fn interrupt_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<InterruptSessionResponse, ApiError>;

    /// Force quit session
    async fn kill_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<KillSessionResponse, ApiError>;

    /// Restart a session
    async fn restart_session(
        &self,
        session_id: String,
        restart_session: Option<models::RestartSession>,
        context: &C,
    ) -> Result<RestartSessionResponse, ApiError>;

    /// Start a session
    async fn start_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<StartSessionResponse, ApiError>;
}

/// API where `Context` isn't passed on every API call
#[async_trait]
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
pub trait ApiNoContext<C: Send + Sync> {
    fn poll_ready(
        &self,
        _cx: &mut Context,
    ) -> Poll<Result<(), Box<dyn Error + Send + Sync + 'static>>>;

    fn context(&self) -> &C;

    /// Notify the server that a client is connected
    async fn client_heartbeat(
        &self,
        client_heartbeat: models::ClientHeartbeat,
    ) -> Result<ClientHeartbeatResponse, ApiError>;

    /// Get the server configuration
    async fn get_server_configuration(&self) -> Result<GetServerConfigurationResponse, ApiError>;

    /// List active sessions
    async fn list_sessions(&self) -> Result<ListSessionsResponse, ApiError>;

    /// Create a new session
    async fn new_session(
        &self,
        new_session: models::NewSession,
    ) -> Result<NewSessionResponse, ApiError>;

    /// Get server status and information
    async fn server_status(&self) -> Result<ServerStatusResponse, ApiError>;

    /// Change the server configuration
    async fn set_server_configuration(
        &self,
        server_configuration: models::ServerConfiguration,
    ) -> Result<SetServerConfigurationResponse, ApiError>;

    /// Shut down all sessions and the server itself
    async fn shutdown_server(&self) -> Result<ShutdownServerResponse, ApiError>;

    /// Adopt an existing session
    async fn adopt_session(
        &self,
        session_id: String,
        connection_info: models::ConnectionInfo,
    ) -> Result<AdoptSessionResponse, ApiError>;

    /// Upgrade to a WebSocket for channel communication
    async fn channels_websocket(
        &self,
        session_id: String,
    ) -> Result<ChannelsWebsocketResponse, ApiError>;

    /// Get Jupyter connection information for the session
    async fn connection_info(&self, session_id: String)
        -> Result<ConnectionInfoResponse, ApiError>;

    /// Delete session
    async fn delete_session(&self, session_id: String) -> Result<DeleteSessionResponse, ApiError>;

    /// Get session details
    async fn get_session(&self, session_id: String) -> Result<GetSessionResponse, ApiError>;

    /// Interrupt session
    async fn interrupt_session(
        &self,
        session_id: String,
    ) -> Result<InterruptSessionResponse, ApiError>;

    /// Force quit session
    async fn kill_session(&self, session_id: String) -> Result<KillSessionResponse, ApiError>;

    /// Restart a session
    async fn restart_session(
        &self,
        session_id: String,
        restart_session: Option<models::RestartSession>,
    ) -> Result<RestartSessionResponse, ApiError>;

    /// Start a session
    async fn start_session(&self, session_id: String) -> Result<StartSessionResponse, ApiError>;
}

/// Trait to extend an API to make it easy to bind it to a context.
pub trait ContextWrapperExt<C: Send + Sync>
where
    Self: Sized,
{
    /// Binds this API to a context.
    fn with_context(self, context: C) -> ContextWrapper<Self, C>;
}

impl<T: Api<C> + Send + Sync, C: Clone + Send + Sync> ContextWrapperExt<C> for T {
    fn with_context(self: T, context: C) -> ContextWrapper<T, C> {
        ContextWrapper::<T, C>::new(self, context)
    }
}

#[async_trait]
impl<T: Api<C> + Send + Sync, C: Clone + Send + Sync> ApiNoContext<C> for ContextWrapper<T, C> {
    fn poll_ready(&self, cx: &mut Context) -> Poll<Result<(), ServiceError>> {
        self.api().poll_ready(cx)
    }

    fn context(&self) -> &C {
        ContextWrapper::context(self)
    }

    /// Notify the server that a client is connected
    async fn client_heartbeat(
        &self,
        client_heartbeat: models::ClientHeartbeat,
    ) -> Result<ClientHeartbeatResponse, ApiError> {
        let context = self.context().clone();
        self.api()
            .client_heartbeat(client_heartbeat, &context)
            .await
    }

    /// Get the server configuration
    async fn get_server_configuration(&self) -> Result<GetServerConfigurationResponse, ApiError> {
        let context = self.context().clone();
        self.api().get_server_configuration(&context).await
    }

    /// List active sessions
    async fn list_sessions(&self) -> Result<ListSessionsResponse, ApiError> {
        let context = self.context().clone();
        self.api().list_sessions(&context).await
    }

    /// Create a new session
    async fn new_session(
        &self,
        new_session: models::NewSession,
    ) -> Result<NewSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().new_session(new_session, &context).await
    }

    /// Get server status and information
    async fn server_status(&self) -> Result<ServerStatusResponse, ApiError> {
        let context = self.context().clone();
        self.api().server_status(&context).await
    }

    /// Change the server configuration
    async fn set_server_configuration(
        &self,
        server_configuration: models::ServerConfiguration,
    ) -> Result<SetServerConfigurationResponse, ApiError> {
        let context = self.context().clone();
        self.api()
            .set_server_configuration(server_configuration, &context)
            .await
    }

    /// Shut down all sessions and the server itself
    async fn shutdown_server(&self) -> Result<ShutdownServerResponse, ApiError> {
        let context = self.context().clone();
        self.api().shutdown_server(&context).await
    }

    /// Adopt an existing session
    async fn adopt_session(
        &self,
        session_id: String,
        connection_info: models::ConnectionInfo,
    ) -> Result<AdoptSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api()
            .adopt_session(session_id, connection_info, &context)
            .await
    }

    /// Upgrade to a WebSocket for channel communication
    async fn channels_websocket(
        &self,
        session_id: String,
    ) -> Result<ChannelsWebsocketResponse, ApiError> {
        let context = self.context().clone();
        self.api().channels_websocket(session_id, &context).await
    }

    /// Get Jupyter connection information for the session
    async fn connection_info(
        &self,
        session_id: String,
    ) -> Result<ConnectionInfoResponse, ApiError> {
        let context = self.context().clone();
        self.api().connection_info(session_id, &context).await
    }

    /// Delete session
    async fn delete_session(&self, session_id: String) -> Result<DeleteSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().delete_session(session_id, &context).await
    }

    /// Get session details
    async fn get_session(&self, session_id: String) -> Result<GetSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().get_session(session_id, &context).await
    }

    /// Interrupt session
    async fn interrupt_session(
        &self,
        session_id: String,
    ) -> Result<InterruptSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().interrupt_session(session_id, &context).await
    }

    /// Force quit session
    async fn kill_session(&self, session_id: String) -> Result<KillSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().kill_session(session_id, &context).await
    }

    /// Restart a session
    async fn restart_session(
        &self,
        session_id: String,
        restart_session: Option<models::RestartSession>,
    ) -> Result<RestartSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api()
            .restart_session(session_id, restart_session, &context)
            .await
    }

    /// Start a session
    async fn start_session(&self, session_id: String) -> Result<StartSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().start_session(session_id, &context).await
    }
}

#[cfg(feature = "client")]
pub mod client;

// Re-export Client as a top-level name
#[cfg(feature = "client")]
pub use client::Client;

#[cfg(feature = "server")]
pub mod server;

// Re-export router() as a top-level name
#[cfg(feature = "server")]
pub use self::server::Service;

#[cfg(feature = "server")]
pub mod context;

pub mod models;

#[cfg(any(feature = "client", feature = "server"))]
pub(crate) mod header;
