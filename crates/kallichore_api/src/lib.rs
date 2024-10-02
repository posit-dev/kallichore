#![allow(
    missing_docs,
    trivial_casts,
    unused_variables,
    unused_mut,
    unused_imports,
    unused_extern_crates,
    non_camel_case_types
)]
#![allow(unused_imports, unused_attributes)]
#![allow(
    clippy::derive_partial_eq_without_eq,
    clippy::disallowed_names,
    clippy::too_many_arguments
)]

use async_trait::async_trait;
use futures::Stream;
// --- Start Kallichore ---
use hyper::{Body, Request, Response};
// --- End Kallichore ---
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::task::{Context, Poll};
use swagger::{ApiError, ContextWrapper};

type ServiceError = Box<dyn Error + Send + Sync + 'static>;

pub const BASE_PATH: &str = "";
pub const API_VERSION: &str = "1.0.0";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum ChannelsWebsocketResponse {
    /// Upgrade connection to a websocket
    UpgradeConnectionToAWebsocket,
    /// Invalid request
    InvalidRequest(models::Error),
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
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
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
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
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
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
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
    /// Session not found
    SessionNotFound,
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
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum RestartSessionResponse {
    /// Restarted
    Restarted(serde_json::Value),
    /// Restart failed
    RestartFailed(models::Error),
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
    /// Session not found
    SessionNotFound,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum ShutdownServerResponse {
    /// Shutting down
    ShuttingDown(serde_json::Value),
    /// Shutdown failed
    ShutdownFailed(models::Error),
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum StartSessionResponse {
    /// Started
    Started(serde_json::Value),
    /// Start failed
    StartFailed(models::Error),
    /// Session not found
    SessionNotFound,
    /// Access token is missing or invalid
    AccessTokenIsMissingOrInvalid,
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

    /// Upgrade to a WebSocket for channel communication
    async fn channels_websocket(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<ChannelsWebsocketResponse, ApiError>;

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

    /// List active sessions
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError>;

    /// Create a new session
    async fn new_session(
        &self,
        new_session: models::NewSession,
        context: &C,
    ) -> Result<NewSessionResponse, ApiError>;

    /// Restart a session
    async fn restart_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<RestartSessionResponse, ApiError>;

    ///
    async fn shutdown_server(&self, context: &C) -> Result<ShutdownServerResponse, ApiError>;

    /// Start a session
    async fn start_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<StartSessionResponse, ApiError>;

    // --- Start Kallichore ---
    /// Upgrade a websocket request for channel communication
    async fn channels_websocket_request(
        &self,
        request: Request<Body>,
        session_id: String,
        context: &C,
    ) -> Result<Response<Body>, ApiError>;
    // --- End Kallichore ---
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

    /// Upgrade to a WebSocket for channel communication
    async fn channels_websocket(
        &self,
        session_id: String,
    ) -> Result<ChannelsWebsocketResponse, ApiError>;

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

    /// List active sessions
    async fn list_sessions(&self) -> Result<ListSessionsResponse, ApiError>;

    /// Create a new session
    async fn new_session(
        &self,
        new_session: models::NewSession,
    ) -> Result<NewSessionResponse, ApiError>;

    /// Restart a session
    async fn restart_session(&self, session_id: String)
        -> Result<RestartSessionResponse, ApiError>;

    ///
    async fn shutdown_server(&self) -> Result<ShutdownServerResponse, ApiError>;

    /// Start a session
    async fn start_session(&self, session_id: String) -> Result<StartSessionResponse, ApiError>;

    // --- Start Kallichore ---
    /// Upgrade a websocket request for channel communication
    async fn channels_websocket_request(
        &self,
        request: Request<Body>,
        session_id: String,
    ) -> Result<Response<Body>, ApiError>;
    // --- End Kallichore ---
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

    /// Upgrade to a WebSocket for channel communication
    async fn channels_websocket(
        &self,
        session_id: String,
    ) -> Result<ChannelsWebsocketResponse, ApiError> {
        let context = self.context().clone();
        self.api().channels_websocket(session_id, &context).await
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

    /// Restart a session
    async fn restart_session(
        &self,
        session_id: String,
    ) -> Result<RestartSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().restart_session(session_id, &context).await
    }

    ///
    async fn shutdown_server(&self) -> Result<ShutdownServerResponse, ApiError> {
        let context = self.context().clone();
        self.api().shutdown_server(&context).await
    }

    /// Start a session
    async fn start_session(&self, session_id: String) -> Result<StartSessionResponse, ApiError> {
        let context = self.context().clone();
        self.api().start_session(session_id, &context).await
    }

    // --- Start Kallichore ---
    /// Upgrade a websocket request for channel communication
    async fn channels_websocket_request(
        &self,
        request: Request<Body>,
        session_id: String,
    ) -> Result<Response<Body>, ApiError> {
        let context = self.context().clone();
        self.api()
            .channels_websocket_request(request, session_id, &context)
            .await
    }
    // --- End Kallichore ---
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
