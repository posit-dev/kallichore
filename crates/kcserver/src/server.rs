//
// server.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

//! Main library entry point for kallichore_api implementation.

#![allow(unused_imports)]

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{future, SinkExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use hyper::client::connect;
use hyper::header::{HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE};
use hyper::server::conn::Http;
use hyper::service::Service;
use hyper::upgrade::Upgraded;
use hyper::{Body, Response, StatusCode};
use hyper_util::rt::TokioIo;
use kallichore_api::models::{NewSession200Response, ServerStatus};
use log::info;
use serde_json::json;
use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::{any, env, usize};
use swagger::auth::MakeAllowAllAuthenticator;
use swagger::{AuthData, ContextBuilder, EmptyContext};
use swagger::{Authorization, Push};
use swagger::{Has, XSpanIdString};
use sysinfo::{Pid, System};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, Sender};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use kallichore_api::models::ConnectionInfo;
use kallichore_api::server::MakeService;
use kallichore_api::{Api, ClientHeartbeatResponse, ListSessionsResponse};
use std::error::Error;
use swagger::ApiError;

use crate::client_session::ClientSession;
use crate::connection_file::{self, ConnectionFile};
use crate::error::KSError;
use crate::kernel_session::{self, KernelSession};
use crate::registration_file::RegistrationFile;
use crate::registration_socket::HandshakeResult;
use crate::working_dir;
use crate::zmq_ws_proxy::{self, ZmqWsProxy};
use kallichore_api::{
    models, AdoptSessionResponse, ChannelsWebsocketResponse, ConnectionInfoResponse,
    DeleteSessionResponse, GetSessionResponse, InterruptSessionResponse, KillSessionResponse,
    NewSessionResponse, RestartSessionResponse, ShutdownServerResponse, StartSessionResponse,
};
use kcshared::{
    handshake_protocol::{HandshakeStatus, HandshakeVersion},
    kernel_message::KernelMessage,
    websocket_message::WebsocketMessage,
};
use tokio::sync::broadcast;

pub async fn create(
    addr: &str,
    token: Option<String>,
    idle_shutdown_hours: Option<u16>,
    log_level: Option<String>,
) {
    let addr = addr.parse().expect("Failed to parse bind address");

    // Get the log level from the provided parameter or environment variable if not provided
    let effective_log_level = match log_level {
        Some(level) => Some(level),
        None => match env::var("RUST_LOG") {
            Ok(level) => Some(level),
            Err(_) => None,
        },
    };

    let server = Server::new(token, idle_shutdown_hours, effective_log_level);

    let service = MakeService::new(server);

    let service = MakeAllowAllAuthenticator::new(service, "cosmo");

    #[allow(unused_mut)]
    let mut service =
        kallichore_api::server::context::MakeAddContext::<_, EmptyContext>::new(service);

    // Using HTTP
    hyper::server::Server::bind(&addr)
        .serve(service)
        .await
        .unwrap()
}

#[derive(Clone)]
pub struct Server<C> {
    marker: PhantomData<C>,
    #[allow(dead_code)]
    token: Option<String>,
    started_time: std::time::Instant,
    kernel_sessions: Arc<RwLock<Vec<KernelSession>>>,
    client_sessions: Arc<RwLock<Vec<ClientSession>>>,
    idle_nudge_tx: Sender<()>,
    reserved_ports: Arc<RwLock<Vec<i32>>>,
    // Shared, thread-safe storage for the idle_shutdown_hours setting
    idle_shutdown_hours: Arc<RwLock<Option<u16>>>,
    #[allow(dead_code)]
    log_level: Option<String>,
    // Channel to signal changes to idle configuration
    idle_config_update_tx: Sender<Option<u16>>,
}

impl<C> Server<C> {
    pub fn new(
        token: Option<String>,
        idle_shutdown_hours: Option<u16>,
        log_level: Option<String>,
    ) -> Self {
        // Create the list of kernel sessions we'll use throughout the server lifetime
        let kernel_sessions = Arc::new(RwLock::new(vec![]));

        // Create a shared, thread-safe storage for the idle_shutdown_hours setting
        let shared_idle_hours = Arc::new(RwLock::new(idle_shutdown_hours));

        // Create channels for idle nudging and configuration updates
        let (idle_nudge_tx, idle_nudge_rx) = mpsc::channel(256);
        let (idle_config_update_tx, idle_config_update_rx) = mpsc::channel(16);

        // Start the idle poll task
        Self::idle_poll_task(
            kernel_sessions.clone(),
            idle_nudge_rx,
            shared_idle_hours.clone(),
            idle_config_update_rx,
        );

        Server {
            token,
            started_time: std::time::Instant::now(),
            marker: PhantomData,
            kernel_sessions,
            client_sessions: Arc::new(RwLock::new(vec![])),
            reserved_ports: Arc::new(RwLock::new(vec![])),
            idle_nudge_tx,
            idle_shutdown_hours: shared_idle_hours,
            log_level,
            idle_config_update_tx,
        }
    }

    /// Starts the idle poll task.
    ///
    /// This task runs in the background and periodically checks for idle
    /// sessions at a configurable interval.
    ///
    /// Note that we run this task even if the user has not specified an idle
    /// shutdown time; we do this to simplify the logic around idle nudges.
    ///
    /// # Arguments
    ///
    /// - `all_sessions`: A reference to the list of all kernel sessions.
    /// - `idle_nudge_rx`: A receiver for idle nudges.
    /// - `idle_shutdown_hours`: The number of hours of inactivity before the
    ///    server shuts down. If None, the server will not shut down due to
    ///    inactivity.
    /// - `idle_config_update_rx`: A receiver for idle configuration updates.
    fn idle_poll_task(
        all_sessions: Arc<RwLock<Vec<KernelSession>>>,
        mut idle_nudge_rx: mpsc::Receiver<()>,
        idle_shutdown_hours: Arc<RwLock<Option<u16>>>,
        mut idle_config_update_rx: mpsc::Receiver<Option<u16>>,
    ) {
        tokio::spawn(async move {
            // Mark the start time of the idle poll task
            let start_time = std::time::Instant::now();

            // Get the initial value of idle_shutdown_hours
            let mut current_idle_shutdown_hours = { *idle_shutdown_hours.read().unwrap() };

            // Create the interval for the idle poll task.
            let mut duration = match current_idle_shutdown_hours {
                Some(hours) => match hours {
                    0 => {
                        // Zero hours would create a busy loop, so if 0 is
                        // specified, check every 30 seconds
                        log::info!("Server set to shut down after 30 seconds of inactivity");
                        tokio::time::Duration::from_secs(30)
                    }
                    _ => {
                        // Set an interval for the specified number of hours.
                        log::info!(
                            "Server set to shut down after {} hours of inactivity",
                            hours
                        );
                        tokio::time::Duration::from_secs((hours as u64) * 3600)
                    }
                },
                None => {
                    // If no idle shutdown hours are specified, default to 24 hours
                    log::debug!("idle_shutdown_hours not specified; the server will not shut down due to inactivity");
                    tokio::time::Duration::from_secs(24 * 3600)
                }
            };
            let mut interval = tokio::time::interval(duration);

            // Consume the first tick immediately (tokio intervals start at 0)
            interval.tick().await;

            loop {
                // Wait for either the interval to expire, an idle nudge, or a configuration update
                tokio::select! {
                    _ = interval.tick() => {
                        // If no idle shutdown hours are specified, skip the check
                        if current_idle_shutdown_hours.is_none() {
                            continue;
                        }

                        // Check wall clock time; if we haven't yet passed the duration of the idle
                        // timeout, skip the check. This should not be necessary since we consumed
                        // the first tick already, but we've seen some cases where the interval
                        // ticks _twice_ after being created. If this happens the server will shut
                        // down immediately after starting since it has no sessions running.
                        let elapsed = std::time::Instant::now().duration_since(start_time);
                        if elapsed < duration {
                            log::trace!("Skipping idle check; elapsed time is {:?} but idle timeout is {:?}", elapsed, duration);
                            continue;
                        }

                        // The interval has expired; check for idle sessions
                        let sessions = {
                            all_sessions.read().unwrap().clone()
                        };
                        Server::<C>::check_idle_sessions(&sessions).await;
                    }
                    _ = idle_nudge_rx.recv() => {
                        log::trace!("Received an idle nudge; resetting interval");
                        // Received an idle nudge; reset the interval
                        interval.reset();
                    }
                    Some(new_idle_hours) = idle_config_update_rx.recv() => {
                        // Received a configuration update
                        log::info!("Updating idle shutdown hours to {:?}", new_idle_hours);
                        current_idle_shutdown_hours = new_idle_hours;

                        // Update the interval duration
                        duration = match current_idle_shutdown_hours {
                            Some(hours) => match hours {
                                0 => tokio::time::Duration::from_secs(30),
                                _ => tokio::time::Duration::from_secs((hours as u64) * 3600),
                            },
                            None => tokio::time::Duration::from_secs(24 * 3600),
                        };

                        // Create a new interval with the updated duration
                        interval = tokio::time::interval(duration);

                        // Consume the first tick immediately
                        interval.tick().await;
                    }
                }
            }
        });
    }

    /// Check for idle sessions.
    ///
    /// This function checks all sessions to see if they are idle and
    /// disconnected. If all sessions are idle and disconnected, the server will
    /// shut down.
    async fn check_idle_sessions(sessions: &Vec<KernelSession>) {
        let mut running_sessions = Vec::new();
        log::debug!(
            "Idle timeout: checking for idle sessions ({} to check)",
            sessions.len()
        );

        // Check to see if all sessions are idle and disconnected.
        for session in sessions.iter() {
            let state = session.state.read().await;
            if state.status == models::Status::Busy {
                log::debug!(
                    "Session {} is in busy state; server is not idle",
                    session.connection.session_id
                );
                return;
            }
            if state.connected {
                log::debug!(
                    "Session {} has an active connection; server is not idle",
                    session.connection.session_id
                );
                return;
            }

            // This session may need to be shut down
            if state.status != models::Status::Exited
                && state.status != models::Status::Uninitialized
            {
                running_sessions.push(session.clone());
            }
        }

        // If we got here, all sessions are idle and disconnected (or otherwise
        // not busy, e.g. they're exited). If we have sessions to shut down, log
        // a message.
        if !running_sessions.is_empty() {
            log::info!(
                "All sessions are idle and disconnected; shutting down {} sessions",
                running_sessions.len()
            );
        }

        // Shut down all the running sessions and exit the server
        if let Err(err) = Server::<C>::shutdown_sessions_and_exit(&running_sessions).await {
            err.log();
        }
    }

    /// Get the list of running sessions.
    async fn running_sessions(sessions: &Vec<KernelSession>) -> Vec<KernelSession> {
        let mut running_sessions = Vec::new();
        for session in sessions.iter() {
            let status = session.state.read().await.status;
            if status != models::Status::Exited && status != models::Status::Uninitialized {
                running_sessions.push(session.clone());
            }
        }
        running_sessions
    }

    /// Shut down all running sessions and exit the server.
    async fn shutdown_sessions_and_exit(
        running_sessions: &Vec<KernelSession>,
    ) -> Result<(), KSError> {
        // Early exit if all sessions are already stopped
        if running_sessions.is_empty() {
            tokio::spawn(async move {
                // Wait 1 second before exiting
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                log::info!("All sessions have exited; exiting Kallichore server.");
                std::process::exit(0);
            });
            return Ok(());
        }

        // Spawn a task to wait for all sessions to exit
        let exiting_sessions = running_sessions.clone();
        tokio::spawn(async move {
            // Create exit event listeners for each running session
            let mut exit_listeners = Vec::new();
            for session in &exiting_sessions {
                let exit_listener = session.exit_event.listen();
                exit_listeners.push(exit_listener);
            }

            // Wait for all sessions to exit.
            //
            // Consider: we could add a timeout here to prevent the server from
            // hanging indefinitely if a session does not exit even after being
            // asked to do so.
            loop {
                // Wait for any of the exit events to be triggered.
                let (_, _, remaining) = future::select_all(exit_listeners).await;
                exit_listeners = remaining;
                if exit_listeners.is_empty() {
                    break;
                } else {
                    log::info!("Session has exited; {} remaining)", exit_listeners.len());
                }
            }

            log::info!("All sessions have exited; exiting Kallichore server.");
            std::process::exit(0);
        });

        log::info!("Shutting down {} running sessions", running_sessions.len());

        // Send a shutdown signal to each running session
        for session in running_sessions {
            let session_id = session.connection.session_id.clone();
            match session.shutdown().await {
                Ok(_) => {
                    log::info!("Session {} has been asked to shut down", session_id);
                }
                Err(e) => {
                    let error = KSError::SessionInterruptFailed(session_id, e);
                    error.log();
                    return Err(error);
                }
            }
        }

        Ok(())
    }

    /// Look up a kernel session by its session ID.
    ///
    /// # Arguments
    ///
    /// - `session_id`: The session ID to look up.
    ///
    /// # Returns
    ///
    /// The kernel session with the given session ID, or None if no such session
    /// exists.
    fn find_session(&self, session_id: String) -> Option<KernelSession> {
        let kernel_sessions = self.kernel_sessions.read().unwrap();
        let kernel_session = match kernel_sessions
            .iter()
            .find(|s| s.connection.session_id == session_id)
        {
            Some(s) => s,
            None => return None,
        };
        Some(kernel_session.clone())
    }

    /// Validate the bearer token in the context.
    ///
    /// # Arguments
    ///
    /// - `context`: The context to validate.
    ///
    /// # Returns
    ///
    /// True if the token is valid or not required, false otherwise. The server
    /// can be run in no-auth mode by using the `--token none` option.
    fn validate_token<D: Has<Option<AuthData>>>(&self, context: &D) -> bool {
        // Validate the bearer token
        if let Some(token) = &self.token {
            let auth_data = context.get();
            let data = match auth_data {
                Some(data) => data,
                None => {
                    log::warn!(
                        "Rejecting request with no authentication data; expected Bearer token"
                    );
                    return false;
                }
            };
            match data {
                AuthData::Bearer(bearer) => {
                    if bearer.token != *token {
                        log::warn!(
                            "Rejecting request; invalid Bearer token '{}' supplied",
                            bearer.token
                        );
                        return false;
                    }
                }
                _ => {
                    log::warn!("Rejecting request; expected Bearer token, got {:?}", data);
                    return false;
                }
            }
        }

        // If we got here, the token is valid or not required
        return true;
    }
}

#[async_trait]
impl<C> Api<C> for Server<C>
where
    C: Has<XSpanIdString> + Has<Option<AuthData>> + Send + Sync,
{
    /// Get session details
    async fn get_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<GetSessionResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "get_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );
        let session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => {
                return Ok(GetSessionResponse::SessionNotFound);
            }
        };
        return Ok(GetSessionResponse::SessionDetails(
            session.as_active_session().await,
        ));
    }

    /// List active sessions
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!("list_sessions - X-Span-ID: {:?}", ctx_span.get().0.clone(),);

        // Make a copy of the active session list to avoid holding the lock
        let sessions = {
            let sessions = self.kernel_sessions.read().unwrap();
            sessions.clone()
        };

        // Create a list of session metadata
        let mut result: Vec<models::ActiveSession> = Vec::new();
        for s in sessions.iter() {
            result.push(s.as_active_session().await);
        }
        let session_list = models::SessionList {
            total: result.len() as i32,
            sessions: result.clone(),
        };
        Ok(ListSessionsResponse::ListOfActiveSessions(session_list))
    }

    /// Create a new session

    async fn new_session(
        &self,
        session: models::NewSession,
        context: &C,
    ) -> Result<NewSessionResponse, ApiError> {
        {
            let ctx_span: &dyn Has<XSpanIdString> = context;
            info!(
                "new_session(\"{}\") - X-Span-ID: {:?}",
                session.session_id,
                ctx_span.get().0.clone(),
            );

            // Token validation
            if !self.validate_token(context) {
                return Ok(NewSessionResponse::Unauthorized);
            }

            // Check to see if the session already exists, dropping the read
            // lock afterwards.
            let sessions = self.kernel_sessions.read().unwrap();
            for s in sessions.iter() {
                if s.connection.session_id == session.session_id {
                    let error = KSError::SessionExists(session.session_id.clone());
                    error.log();
                    return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
                }
            }
        }

        let new_session_id = session.session_id.clone();
        let session_id = NewSession200Response {
            session_id: new_session_id.clone(),
        };

        // Generate a key for the session
        let key_bytes = rand::Rng::gen::<[u8; 16]>(&mut rand::thread_rng());
        let key = hex::encode(key_bytes);

        // Create log file path which may be needed in argv
        let temp_dir = env::temp_dir();
        let mut log_file_name = std::ffi::OsString::from("kernel_log_");
        log_file_name.push(new_session_id.clone());
        log_file_name.push(".txt");
        let log_path: PathBuf = temp_dir.join(log_file_name);

        log::debug!(
            "Created log file for session {} at {:?}",
            new_session_id.clone(),
            log_path
        );

        let session = models::NewSession {
            session_id: session_id.session_id.clone(),
            argv: session.argv,
            display_name: session.display_name.clone(),
            language: session.language.clone(),
            working_directory: session.working_directory.clone(),
            input_prompt: session.input_prompt.clone(),
            continuation_prompt: session.continuation_prompt.clone(),
            username: session.username.clone(),
            env: session.env.clone(),
            interrupt_mode: session.interrupt_mode.clone(),
            connection_timeout: session.connection_timeout.clone(),
            protocol_version: session.protocol_version.clone(),
        };

        let sessions = self.kernel_sessions.clone();

        let kernel_session = match KernelSession::new(
            session,
            key,
            self.idle_nudge_tx.clone(),
            self.reserved_ports.clone(),
        )
        .await
        {
            Ok(kernel_session) => kernel_session,
            Err(e) => {
                let error = KSError::SessionCreateFailed(
                    new_session_id.clone(),
                    anyhow!("Kernel session couldn't be established: {}", e),
                );
                return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
            }
        };

        let mut sessions = sessions.write().unwrap();
        sessions.push(kernel_session);
        Ok(NewSessionResponse::TheSessionID(session_id))
    }

    /// Adopt a session
    async fn adopt_session(
        &self,
        session_id: String,
        connection_info: ConnectionInfo,
        context: &C,
    ) -> Result<AdoptSessionResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "adopt_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );

        // Token validation
        if !self.validate_token(context) {
            return Ok(AdoptSessionResponse::Unauthorized);
        }

        // Find the session with the given ID
        let kernel_session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => {
                return Ok(AdoptSessionResponse::SessionNotFound);
            }
        };

        // Create the connection file from the connection info
        let connection_file = ConnectionFile::from_info(connection_info.clone());

        // Attempt to connect to the new session
        let result = kernel_session.connect(connection_file).await;
        match result {
            Ok(v) => Ok(AdoptSessionResponse::Adopted(v)),
            Err(error) => {
                error.log();
                Ok(AdoptSessionResponse::AdoptionFailed(error.to_json(None)))
            }
        }
    }

    /// Delete a session
    async fn delete_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<DeleteSessionResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "delete_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );

        // Token validation
        if !self.validate_token(context) {
            return Ok(DeleteSessionResponse::Unauthorized);
        }

        // Find the session with the given ID
        let kernel_session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => {
                return Ok(DeleteSessionResponse::SessionNotFound);
            }
        };

        // Ensure the session is not running
        let status = {
            let state = kernel_session.state.read().await;
            state.status.clone()
        };
        if status != models::Status::Exited {
            let error = KSError::SessionRunning(session_id.clone());
            error.log();
            return Ok(DeleteSessionResponse::FailedToDeleteSession(
                error.to_json(None),
            ));
        }

        // Ensure we get a write lock on the kernel sessions for the duration of
        // this function
        let mut sessions = self.kernel_sessions.write().unwrap();

        // Find the index of the session with the given ID
        let index = {
            sessions
                .iter()
                .position(|s| s.connection.session_id == session_id)
        };

        // Not likely since we just found it above, but be threadsafe!
        if index.is_none() {
            return Ok(DeleteSessionResponse::SessionNotFound);
        }

        // Remove the session from the list
        sessions.remove(index.unwrap());

        Ok(DeleteSessionResponse::SessionDeleted(
            serde_json::Value::Null,
        ))
    }

    async fn channels_websocket(
        &self,
        session_id: String,
        _context: &C,
    ) -> Result<ChannelsWebsocketResponse, ApiError> {
        info!("upgrade to websocket: {}", session_id);
        Ok(ChannelsWebsocketResponse::UpgradeConnectionToAWebsocket)
    }

    async fn connection_info(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<ConnectionInfoResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "connection_info(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );

        // Token validation
        if !self.validate_token(context) {
            return Ok(ConnectionInfoResponse::Unauthorized);
        }

        // Get the kernel session with the given ID
        let kernel_session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => {
                return Ok(ConnectionInfoResponse::SessionNotFound);
            }
        };

        // Return the connection info
        match kernel_session.ensure_connection_file().await {
            Ok(conn_file) => Ok(ConnectionInfoResponse::ConnectionInfo(
                conn_file.info.clone(),
            )),
            Err(e) => {
                let error = KSError::NoConnectionInfo(session_id.clone(), e);
                error.log();
                Ok(ConnectionInfoResponse::Failed(error.to_json(None)))
            }
        }
    }

    async fn kill_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<KillSessionResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "kill_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );
        // Token validation
        if !self.validate_token(context) {
            return Ok(KillSessionResponse::Unauthorized);
        }

        // Get the kernel session with the given ID
        let kernel_session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => {
                return Ok(KillSessionResponse::SessionNotFound);
            }
        };

        // Ensure that the kernel hasn't already exited
        let status = {
            let state = kernel_session.state.read().await;
            state.status.clone()
        };
        if status == models::Status::Exited {
            let err = KSError::SessionNotRunning(session_id.clone());
            err.log();
            return Ok(KillSessionResponse::KillFailed(err.to_json(None)));
        }

        // Clear the execution queue and get the process ID
        let pid = {
            let mut state = kernel_session.state.write().await;
            state.execution_queue.clear();
            state.process_id
        };
        match pid {
            Some(pid) => {
                // Kill the process
                let mut system = System::new();
                let process_id = Pid::from_u32(pid);
                system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[process_id]));
                if let Some(process) = system.process(process_id) {
                    if process.kill() {
                        Ok(KillSessionResponse::Killed(serde_json::Value::Null))
                    } else {
                        let err = KSError::ProcessNotFound(pid, session_id.clone());
                        err.log();
                        Ok(KillSessionResponse::KillFailed(err.to_json(Some(
                            String::from("Failed to send kill signal to process"),
                        ))))
                    }
                } else {
                    let err = KSError::ProcessNotFound(pid, session_id.clone());
                    err.log();
                    return Ok(KillSessionResponse::KillFailed(
                        err.to_json(Some(String::from("Could not look up process details"))),
                    ));
                }
            }
            None => {
                // This could happen if it's an adopted session (for which we don't have a process
                // ID)
                let err = KSError::NoProcess(session_id.clone());
                err.log();
                Ok(KillSessionResponse::KillFailed(err.to_json(None)))
            }
        }
    }

    async fn start_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<StartSessionResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "start_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );

        // Token validation
        if !self.validate_token(context) {
            return Ok(StartSessionResponse::Unauthorized);
        }

        // Find the kernel session with the given ID
        let kernel_session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => return Ok(StartSessionResponse::SessionNotFound),
        };

        // Attempt to start the kernel; blocks until the kernel sockets are
        // connected or a failure occurs
        match kernel_session.start().await {
            Ok(kernel_info) => Ok(StartSessionResponse::Started(kernel_info)),
            Err(e) => Ok(StartSessionResponse::StartFailed(e)),
        }
    }

    async fn interrupt_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<InterruptSessionResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "interrupt_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );

        // Token validation
        if !self.validate_token(context) {
            return Ok(InterruptSessionResponse::Unauthorized);
        }

        let kernel_session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => {
                return Ok(InterruptSessionResponse::SessionNotFound);
            }
        };

        match kernel_session.interrupt().await {
            Ok(_) => Ok(InterruptSessionResponse::Interrupted(
                serde_json::Value::Null,
            )),
            Err(e) => {
                let error = KSError::SessionInterruptFailed(session_id, e);
                error.log();
                Ok(InterruptSessionResponse::InterruptFailed(
                    error.to_json(None),
                ))
            }
        }
    }

    async fn restart_session(
        &self,
        session_id: String,
        restart_session: Option<models::RestartSession>,
        context: &C,
    ) -> Result<RestartSessionResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "restart_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            ctx_span.get().0.clone(),
        );

        // Token validation
        if !self.validate_token(context) {
            return Ok(RestartSessionResponse::Unauthorized);
        }

        let session = match self.find_session(session_id.clone()) {
            Some(kernel_session) => kernel_session,
            None => {
                return Ok(RestartSessionResponse::SessionNotFound);
            }
        };

        let working_dir = restart_session
            .as_ref()
            .and_then(|rs| rs.working_directory.as_ref())
            .filter(|dir| !dir.is_empty())
            .cloned();

        let env_vars = restart_session
            .as_ref()
            .and_then(|rs| rs.env.as_ref())
            .filter(|env| !env.is_empty())
            .cloned();

        match session.restart(working_dir, env_vars).await {
            Ok(_) => Ok(RestartSessionResponse::Restarted(serde_json::Value::Null)),
            Err(e) => Ok(RestartSessionResponse::RestartFailed(e)),
        }
    }

    async fn server_status(
        &self,
        _context: &C,
    ) -> Result<kallichore_api::ServerStatusResponse, ApiError> {
        // Make a copy of the active session list to avoid holding the lock
        let sessions = {
            let sessions = self.kernel_sessions.read().unwrap();
            sessions.clone()
        };

        // Aggregate busy/idle status across all sessions
        let mut busy = false;
        let mut active = 0;
        let mut earliest_busy: Option<std::time::Instant> = None;
        let mut latest_idle: Option<std::time::Instant> = None;
        for s in sessions.iter() {
            let state = s.state.read().await;
            if state.status == models::Status::Busy {
                // If the session is busy, set the busy flag and update the earliest
                // busy time (if multiple sessions are busy, we've been busy since
                // the earliest one)
                busy = true;
                earliest_busy = match earliest_busy {
                    Some(earliest) => match state.busy_since {
                        Some(busy_since) => {
                            if busy_since < earliest {
                                Some(busy_since)
                            } else {
                                earliest_busy
                            }
                        }
                        None => earliest_busy,
                    },
                    None => state.busy_since,
                };
            } else {
                // If the session is not busy, update the latest idle time (if
                // all sessions are idle, we've only been idle since the latest
                // one)
                latest_idle = match latest_idle {
                    Some(latest) => match state.idle_since {
                        Some(idle_since) => {
                            if idle_since > latest {
                                Some(idle_since)
                            } else {
                                latest_idle
                            }
                        }
                        None => latest_idle,
                    },
                    None => state.idle_since,
                };
            }
            if state.status != models::Status::Exited {
                active += 1;
            }
        }

        // Compute the number of seconds since the last idle/busy event
        let now = std::time::Instant::now();
        let idle_seconds = match sessions.len() {
            0 => {
                // If there are no sessions yet, we've been idle since the server started
                now.duration_since(self.started_time).as_secs() as i32
            }
            _ => match latest_idle {
                Some(latest_idle) => now.duration_since(latest_idle).as_secs() as i32,
                None => 0,
            },
        };

        let busy_seconds = match earliest_busy {
            Some(earliest_busy) => {
                let now = std::time::Instant::now();
                now.duration_since(earliest_busy).as_secs() as i32
            }
            None => 0,
        };

        let resp = ServerStatus {
            busy,
            idle_seconds,
            busy_seconds,
            sessions: sessions.len() as i32,
            active,
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        Ok(kallichore_api::ServerStatusResponse::ServerStatusAndInformation(resp))
    }

    async fn channels_websocket_request(
        &self,
        mut request: hyper::Request<Body>,
        session_id: String,
        _context: &C,
    ) -> Result<Response<Body>, ApiError> {
        // TODO: This should also validate the bearer token to initiate the
        // websocket connection
        log::debug!(
            "Upgrading channel connection to websocket for session '{}'",
            session_id
        );
        let derived = {
            let headers = request.headers();
            let key = headers.get(SEC_WEBSOCKET_KEY);
            key.map(|k| derive_accept_key(k.as_bytes()))
        };
        let version = request.version();
        let kernel_sessions = self.kernel_sessions.clone();
        {
            // Validate the session ID before upgrading the connection
            let kernel_sessions = kernel_sessions.read().unwrap();
            if kernel_sessions
                .iter()
                .find(|s| s.connection.session_id == session_id)
                .is_none()
            {
                let err = KSError::SessionNotFound(session_id.clone());
                err.log();
                let err = err.to_json(Some(
                    "Establishing a websocket connection requires a valid session ID".to_string(),
                ));
                // Serialize the error to JSON
                let err = serde_json::to_string(&err).unwrap();
                let mut response = Response::new(err.into());
                *response.status_mut() = StatusCode::BAD_REQUEST;
                return Ok(response);
            }
        };

        let client_sessions: Arc<RwLock<Vec<ClientSession>>> = self.client_sessions.clone();
        {
            // Check if the session already exists
            let mut client_sessions = client_sessions.write().unwrap();
            let index = client_sessions
                .iter()
                .position(|s| s.connection.session_id == session_id);
            match index {
                Some(pos) => {
                    let session = client_sessions.get(pos).unwrap();
                    // Disconnect the existing session
                    log::debug!("Disconnecting existing client session for '{}'", session_id);
                    session.disconnect.notify(usize::MAX);
                    // Remove the existing session
                    client_sessions.remove(pos);
                }
                None => {
                    log::trace!("No existing client session found for '{}'", session_id);
                }
            }
        }

        tokio::task::spawn(async move {
            match hyper::upgrade::on(&mut request).await {
                Ok(upgraded) => {
                    log::debug!("Creating session for websocket connection");

                    // Find the kernel session with the given ID
                    let client_session = {
                        let sessions = kernel_sessions.read().unwrap();
                        let session = sessions
                            .iter()
                            .find(|s| s.connection.session_id == session_id)
                            .expect("Session not found");

                        let ws_zmq_tx = session.ws_zmq_tx.clone();
                        let ws_json_rx = session.ws_json_rx.clone();
                        let connection = session.connection.clone();
                        let state = session.state.clone();
                        ClientSession::new(connection, ws_json_rx, ws_zmq_tx, state)
                    };

                    // Add it to the list of client sessions
                    {
                        let mut client_sessions = client_sessions.write().unwrap();
                        client_sessions.push(client_session.clone());
                    }

                    log::debug!(
                        "Connection upgraded to websocket for session '{}'",
                        session_id
                    );

                    let stream =
                        WebSocketStream::from_raw_socket(upgraded, Role::Server, None).await;
                    client_session.handle_channel_ws(stream).await;
                }
                Err(e) => {
                    log::error!("Failed to upgrade channel connection to websocket: {}", e);
                }
            }
        });
        let upgrade = HeaderValue::from_static("Upgrade");
        let websocket = HeaderValue::from_static("websocket");
        let mut response = Response::new(Body::default());
        *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
        *response.version_mut() = version;
        response.headers_mut().append(CONNECTION, upgrade);
        response.headers_mut().append(UPGRADE, websocket);
        response
            .headers_mut()
            .append(SEC_WEBSOCKET_ACCEPT, derived.unwrap().parse().unwrap());
        Ok(response)
    }

    async fn client_heartbeat(&self, context: &C) -> Result<ClientHeartbeatResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "client_heartbeat - X-Span-ID: {:?}",
            ctx_span.get().0.clone()
        );
        match self.idle_nudge_tx.send(()).await {
            Ok(_) => {
                log::trace!("Client heartbeat processed successfully");
            }
            Err(e) => {
                // This is a fire-and-forget operation, so we don't need to
                // return an error to the client.
                log::error!("Failed to send client heartbeat: {}", e);
            }
        }
        Ok(ClientHeartbeatResponse::HeartbeatReceived(
            serde_json::Value::Null,
        ))
    }

    async fn shutdown_server(&self, context: &C) -> Result<ShutdownServerResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "shutdown_server - X-Span-ID: {:?}",
            ctx_span.get().0.clone()
        );

        // Token validation
        if !self.validate_token(context) {
            return Ok(ShutdownServerResponse::Unauthorized);
        }

        // Create a vector of all the kernel sessions that are currently running
        let running_sessions = {
            let kernel_sessions = self.kernel_sessions.read().unwrap().clone();
            Self::running_sessions(&kernel_sessions).await
        };

        match Self::shutdown_sessions_and_exit(&running_sessions).await {
            Ok(_) => Ok(ShutdownServerResponse::ShuttingDown(
                serde_json::Value::Null,
            )),
            Err(e) => Ok(ShutdownServerResponse::ShutdownFailed(e.to_json(None))),
        }
    }

    async fn get_server_configuration(
        &self,
        context: &C,
    ) -> Result<kallichore_api::GetServerConfigurationResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "get_server_configuration - X-Span-ID: {:?}",
            ctx_span.get().0.clone()
        );

        // Read the idle_shutdown_hours from the shared value
        let idle_hours = {
            // Get a lock on the value and read it
            let idle_hours_guard = self.idle_shutdown_hours.read().unwrap();

            // Convert Option<u16> to Option<i32>
            idle_hours_guard.map(|hours| hours as i32)
        };

        // Return the server configuration
        Ok(
            kallichore_api::GetServerConfigurationResponse::TheCurrentServerConfiguration(
                models::ServerConfiguration {
                    idle_shutdown_hours: idle_hours,
                    log_level: self.log_level.clone(),
                },
            ),
        )
    }

    async fn set_server_configuration(
        &self,
        configuration: models::ServerConfiguration,
        context: &C,
    ) -> Result<kallichore_api::SetServerConfigurationResponse, ApiError> {
        let ctx_span: &dyn Has<XSpanIdString> = context;
        info!(
            "set_server_configuration - X-Span-ID: {:?}",
            ctx_span.get().0.clone()
        );

        // Check if idle_shutdown_hours is provided in the configuration
        if let Some(idle_hours) = configuration.idle_shutdown_hours {
            // Convert from i32 to u16, with validation
            if idle_hours < 0 {
                return Ok(kallichore_api::SetServerConfigurationResponse::Error(
                    models::Error {
                        code: "400".to_string(),
                        message: "idle_shutdown_hours must be a non-negative integer".to_string(),
                        details: None,
                    },
                ));
            }

            let new_idle_hours = if idle_hours == 0 {
                // Special case: 0 means disabled
                None
            } else {
                // Convert to u16, handling potential overflow
                match u16::try_from(idle_hours) {
                    Ok(hours) => Some(hours),
                    Err(_) => {
                        return Ok(kallichore_api::SetServerConfigurationResponse::Error(
                            models::Error {
                                code: "400".to_string(),
                                message: "idle_shutdown_hours is too large".to_string(),
                                details: None,
                            },
                        ));
                    }
                }
            };

            // Update the stored value by sending a message to the idle_poll_task
            log::info!("Updating idle_shutdown_hours to {:?}", new_idle_hours);
            if let Err(e) = self.idle_config_update_tx.send(new_idle_hours).await {
                log::error!("Failed to send idle configuration update: {:?}", e);
                return Ok(kallichore_api::SetServerConfigurationResponse::Error(
                    models::Error {
                        code: "500".to_string(),
                        message: "Failed to update idle shutdown configuration".to_string(),
                        details: Some(e.to_string()),
                    },
                ));
            }

            // Update the stored value in the Server struct using the thread-safe RwLock
            {
                let mut idle_hours_guard = self.idle_shutdown_hours.write().unwrap();
                *idle_hours_guard = new_idle_hours;
            }
        }

        // Return success
        Ok(
            kallichore_api::SetServerConfigurationResponse::ConfigurationUpdated(
                serde_json::Value::Null,
            ),
        )
    }
}
