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

use kallichore_api::{
    models, AdoptSessionResponse, ChannelsWebsocketResponse, ConnectionInfoResponse,
    DeleteSessionResponse, GetSessionResponse, InterruptSessionResponse, KillSessionResponse,
    NewSessionResponse, RestartSessionResponse, ShutdownServerResponse, StartSessionResponse,
};

pub async fn create(addr: &str, token: Option<String>, idle_shutdown_hours: Option<u16>) {
    let addr = addr.parse().expect("Failed to parse bind address");

    let server = Server::new(token, idle_shutdown_hours);

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
    /// Registration socket for JEP 66 handshaking
    #[allow(dead_code)]
    registration_socket: Arc<RwLock<Option<RegistrationSocket>>>,
}

impl<C> Server<C> {
    pub fn new(token: Option<String>, idle_shutdown_hours: Option<u16>) -> Self {
        // Create the list of kernel sessions we'll use throughout the server lifetime
        let kernel_sessions = Arc::new(RwLock::new(vec![]));

        // Start the idle poll task
        let (idle_nudge_tx, idle_nudge_rx) = mpsc::channel(256);
        Server::<C>::idle_poll_task(kernel_sessions.clone(), idle_nudge_rx, idle_shutdown_hours);

        // Create registration socket for JEP 66 handshaking
        let registration_socket = Arc::new(RwLock::new(None));

        // Pick a free port for the registration socket
        let port = portpicker::pick_unused_port()
            .expect("Could not find a free port for the registration socket");

        // Start the registration socket in a separate async task
        let reg_socket_arc = registration_socket.clone();
        let kernel_sessions_clone = kernel_sessions.clone();
        tokio::spawn(async move {
            // Create the registration socket with the default port
            let mut socket = RegistrationSocket::new(port);

            // Start the socket
            match socket.start().await {
                Ok(()) => {
                    // Store the socket in the arc after successful start
                    let mut reg_socket = reg_socket_arc.write().unwrap();
                    *reg_socket = Some(socket);

                    log::info!("JEP 66 handshaking protocol is available - kernels can connect to the registration socket");

                    // Get a receiver for handshake results
                    if let Some(ref socket) = *reg_socket {
                        let receiver = socket.get_result_receiver();

                        // Spawn a task to handle handshake results
                        let kernel_sessions = kernel_sessions_clone.clone();
                        tokio::spawn(async move {
                            Self::process_handshake_results(receiver, kernel_sessions).await;
                        });
                    }
                }
                Err(e) => {
                    // Log the error but continue without the handshaking protocol
                    log::warn!("Failed to start JEP 66 registration socket: {}. Handshaking protocol will not be available.", e);
                }
            }
        });

        Server {
            token,
            started_time: std::time::Instant::now(),
            marker: PhantomData,
            kernel_sessions,
            client_sessions: Arc::new(RwLock::new(vec![])),
            reserved_ports: Arc::new(RwLock::new(vec![])),
            idle_nudge_tx,
            registration_socket,
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
    fn idle_poll_task(
        all_sessions: Arc<RwLock<Vec<KernelSession>>>,
        mut idle_nudge_rx: mpsc::Receiver<()>,
        idle_shutdown_hours: Option<u16>,
    ) {
        tokio::spawn(async move {
            // Mark the start time of the idle poll task
            let start_time = std::time::Instant::now();

            // Create the interval for the idle poll task.
            let duration = match idle_shutdown_hours {
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
                // Wait for either the interval to expire or an idle nudge
                tokio::select! {
                    _ = interval.tick() => {
                        // If no idle shutdown hours are specified, skip the check
                        if idle_shutdown_hours.is_none() {
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

use kallichore_api::models::ConnectionInfo;
use kallichore_api::server::MakeService;
use kallichore_api::{Api, ListSessionsResponse};
use std::error::Error;
use swagger::ApiError;

use crate::client_session::ClientSession;
use crate::connection_file::{self, ConnectionFile, RegistrationFile};
use crate::error::KSError;
use crate::kernel_session::{self, KernelSession};
use crate::registration_socket::HandshakeResult;
use crate::registration_socket::RegistrationSocket;
use crate::working_dir;
use crate::zmq_ws_proxy::{self, ZmqWsProxy};
use kcshared::{
    handshake_protocol::{HandshakeStatus, HandshakeVersion},
    kernel_message::KernelMessage,
    websocket_message::WebsocketMessage,
};
use tokio::sync::broadcast;

impl<C> Server<C> {
    /// Process handshake results received from kernels
    async fn process_handshake_results(
        mut receiver: broadcast::Receiver<HandshakeResult>,
        kernel_sessions: Arc<RwLock<Vec<KernelSession>>>,
    ) {
        while let Ok(result) = receiver.recv().await {
            match result.status {
                HandshakeStatus::Ok => {
                    log::info!(
                        "Received handshake request from kernel with ports: shell={}, iopub={}, stdin={}, control={}, hb={}",
                        result.request.shell_port,
                        result.request.iopub_port,
                        result.request.stdin_port,
                        result.request.control_port,
                        result.request.hb_port
                    );

                    // Process the handshake in a separate function to avoid deadlocks
                    Self::process_single_handshake(result, kernel_sessions.clone()).await;
                }
                HandshakeStatus::Error => {
                    log::warn!("Received invalid handshake request from kernel");
                }
            }
        }
    }

    /// Process a single handshake result
    async fn process_single_handshake(
        result: HandshakeResult,
        kernel_sessions: Arc<RwLock<Vec<KernelSession>>>,
    ) {
        // First, find a session in Starting state by getting the session IDs
        let sessions_to_check = {
            let sessions = kernel_sessions.read().unwrap();
            sessions
                .iter()
                .map(|s| s.connection.session_id.clone())
                .collect::<Vec<_>>()
        };

        // Now check each session individually
        for session_id in sessions_to_check {
            // Find this session again
            let session_opt = {
                let sessions = kernel_sessions.read().unwrap();
                sessions
                    .iter()
                    .find(|s| s.connection.session_id == session_id)
                    .cloned()
            };

            if let Some(session) = session_opt {
                // Check if it's in Starting state
                let is_starting = {
                    let state = session.state.read().await;
                    state.status == models::Status::Starting
                };

                if is_starting {
                    // Parse the handshake version
                    let version =
                        if HandshakeVersion::supports_handshaking(&result.request.protocol_version)
                        {
                            // Parse the version from the string (e.g., "5.5")
                            if let Some((major, minor)) =
                                result.request.protocol_version.split_once('.')
                            {
                                if let (Ok(major), Ok(minor)) =
                                    (major.parse::<u32>(), minor.parse::<u32>())
                                {
                                    Some(HandshakeVersion { major, minor })
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                    // Update the session in the sessions list
                    {
                        let mut sessions = kernel_sessions.write().unwrap();

                        if let Some(session) = sessions
                            .iter_mut()
                            .find(|s| s.connection.session_id == session_id)
                        {
                            // Update the connection file with the ports from the handshake
                            // Create or update the connection file with ports from handshake
                            if let Some(ref mut conn_file) = session.connection_file {
                                // Update existing connection file
                                conn_file.info.shell_port = result.request.shell_port as i32;
                                conn_file.info.iopub_port = result.request.iopub_port as i32;
                                conn_file.info.stdin_port = result.request.stdin_port as i32;
                                conn_file.info.control_port = result.request.control_port as i32;
                                conn_file.info.hb_port = result.request.hb_port as i32;
                            } else {
                                // Create a new connection file with ports from handshake
                                let info = ConnectionInfo {
                                    shell_port: result.request.shell_port as i32,
                                    iopub_port: result.request.iopub_port as i32,
                                    stdin_port: result.request.stdin_port as i32,
                                    control_port: result.request.control_port as i32,
                                    hb_port: result.request.hb_port as i32,
                                    transport: "tcp".to_string(),
                                    signature_scheme: "hmac-sha256".to_string(),
                                    // The HandshakeRequest doesn't have a key field, use an empty string
                                    // This doesn't matter as it's just used for displaying the connection info
                                    key: String::new(),
                                    ip: "127.0.0.1".to_string(),
                                };
                                session.connection_file = Some(ConnectionFile::from_info(info));
                            }
                        }
                    }

                    // Update the kernel state with handshake information
                    {
                        let mut state = session.state.write().await;
                        state.handshake_version = version;
                        state.kernel_capabilities = result.request.capabilities.clone();
                    }

                    log::info!(
                        "[session {}] Updated session with ports from handshake: shell={}, iopub={}, stdin={}, control={}, hb={}",
                        session_id,
                        result.request.shell_port,
                        result.request.iopub_port,
                        result.request.stdin_port,
                        result.request.control_port,
                        result.request.hb_port
                    );

                    // Send an event via the session's websocket channel
                    let msg = WebsocketMessage::Kernel(KernelMessage::HandshakeCompleted(
                        session_id.clone(),
                    ));
                    if let Err(e) = session.ws_json_tx.send(msg).await {
                        log::warn!(
                            "[session {}] Failed to send handshake completed message: {}",
                            session_id,
                            e
                        );
                    }

                    // Only update one session per handshake
                    break;
                }
            }
        }
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
        let args = session.argv.clone();

        // Create a connection file for the session in a temporary directory
        // Determine if the kernel supports JEP 66 handshaking based on the protocol version
        let protocol_version = session.protocol_version.as_deref().unwrap_or("5.3");
        let supports_handshaking = ConnectionFile::requires_handshaking(protocol_version);

        // The optional connection file and optional registration file
        let mut connection_file_opt = None;
        let mut registration_file_opt = None;

        if supports_handshaking {
            // For JEP 66 handshaking, create a registration file first
            // The actual ports for the connection will be determined by the kernel during handshaking

            // Generate a key for the registration file
            let key_bytes = rand::Rng::gen::<[u8; 16]>(&mut rand::thread_rng());
            let key = hex::encode(key_bytes);

            // Generate the registration file from our registration_socket's port
            let port = match self.registration_socket.read().unwrap().as_ref() {
                Some(socket) => socket.port,
                None => {
                    let error = KSError::SessionCreateFailed(
                        new_session_id.clone(),
                        anyhow!("Registration socket not available for JEP 66 handshaking"),
                    );
                    error.log();
                    return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
                }
            };
            let registration_file =
                RegistrationFile::new(String::from("127.0.0.1"), port, key.clone());

            // Store the registration port for later use
            let registration_port = registration_file.registration_port;

            // Store the registration file for writing later
            registration_file_opt = Some(registration_file);

            log::debug!(
                "[session {}] Using JEP 66 handshaking (protocol version {}) - registration port: {}",
                new_session_id,
                protocol_version,
                registration_port
            );

            // No connection file is created initially for JEP 66
            // The connection details will be provided by the kernel during handshaking

            // We'll wait for the handshake to provide connection info
        } else {
            // Create a standard connection file for the session with pre-assigned ports
            let conn_file = match ConnectionFile::generate(
                String::from("127.0.0.1"),
                self.reserved_ports.clone(),
            ) {
                Ok(conn_file) => conn_file,
                Err(e) => {
                    let error = KSError::SessionCreateFailed(
                        new_session_id.clone(),
                        anyhow!("Couldn't create connection file: {}", e),
                    );
                    error.log();
                    return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
                }
            };

            // Store the connection file
            connection_file_opt = Some(conn_file);

            log::debug!(
                "[session {}] Using traditional Jupyter protocol (version {}) - pre-assigning ports",
                new_session_id,
                protocol_version
            );
        }

        // Create the connection file path
        let temp_dir = env::temp_dir();
        let mut connection_file_name = std::ffi::OsString::from("connection_");
        connection_file_name.push(new_session_id.clone());
        connection_file_name.push(".json");

        // Combine the temporary directory with the file name to get the full path
        let connection_path: PathBuf = temp_dir.join(connection_file_name);

        // Write either a standard connection file or a registration file
        if supports_handshaking {
            // For JEP 66 handshaking, use the registration file we created earlier
            if let Some(registration_file) = &registration_file_opt {
                // Write the registration file
                if let Err(err) = registration_file.to_file(connection_path.clone()) {
                    let error = KSError::SessionCreateFailed(
                        new_session_id.clone(),
                        anyhow!(
                            "Failed to write registration file {}: {}",
                            connection_path.to_string_lossy(),
                            err
                        ),
                    );
                    error.log();
                    return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
                }

                log::debug!(
                    "Created registration file for session {} at {:?} (port: {})",
                    new_session_id.clone(),
                    connection_path,
                    registration_file.registration_port
                );
            } else {
                // This shouldn't happen - we should always have a registration file at this point
                log::error!(
                    "[session {}] Missing registration file for JEP 66 handshaking",
                    new_session_id
                );
            }
        } else {
            // For traditional protocol, write the connection file
            if let Some(connection_file) = &connection_file_opt {
                if let Err(err) = connection_file.to_file(connection_path.clone()) {
                    let error = KSError::SessionCreateFailed(
                        new_session_id.clone(),
                        anyhow!(
                            "Failed to write connection file {}: {}",
                            connection_path.to_string_lossy(),
                            err
                        ),
                    );
                    error.log();
                    return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
                }

                log::debug!(
                    "Created connection file for session {} at {:?}",
                    new_session_id.clone(),
                    connection_path
                );
            } else {
                // This shouldn't happen - we should always have a connection file at this point
                log::error!(
                    "[session {}] Missing connection file for traditional protocol",
                    new_session_id
                );
            }
        }

        // Debug logs were already output above for each case (JEP 66 and standard)

        let mut log_file_name = std::ffi::OsString::from("kernel_log_");
        log_file_name.push(new_session_id.clone());
        log_file_name.push(".txt");
        let log_path: PathBuf = temp_dir.join(log_file_name);

        log::debug!(
            "Created log file for session {} at {:?}",
            new_session_id.clone(),
            log_path
        );

        // Loop through the arguments; if any is the special string "{connection_file}", replace it with the session id
        let args: Vec<String> = args
            .iter()
            .map(|arg| {
                if arg == "{connection_file}" {
                    connection_path.to_string_lossy().to_string()
                } else if arg == "{log_file}" {
                    log_path.to_string_lossy().to_string()
                } else {
                    arg.clone()
                }
            })
            .collect();

        let session = models::NewSession {
            session_id: session_id.session_id.clone(),
            argv: args,
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
        // Create the appropriate connection file for the KernelSession
        let session_connection_file = if supports_handshaking {
            // For JEP 66, we need to create a ConnectionFile with empty ports
            let dummy_info = ConnectionInfo {
                control_port: 0,
                shell_port: 0,
                stdin_port: 0,
                iopub_port: 0,
                hb_port: 0,
                transport: "tcp".to_string(),
                signature_scheme: "hmac-sha256".to_string(),
                key: registration_file_opt.as_ref().unwrap().key.clone(),
                ip: "127.0.0.1".to_string(),
            };
            ConnectionFile::from_info(dummy_info)
        } else {
            // For traditional protocol, use the connection file we created
            connection_file_opt.clone().unwrap()
        };

        let kernel_session = match KernelSession::new(
            session,
            Some(session_connection_file),
            self.idle_nudge_tx.clone(),
            self.reserved_ports.clone(),
        ) {
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
        // Ensure we have connection info available
        match &kernel_session.connection_file {
            Some(conn_file) => Ok(ConnectionInfoResponse::ConnectionInfo(
                conn_file.info.clone(),
            )),
            None => {
                let error = KSError::NoConnectionInfo(session_id.clone());
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

        // Extract the working directory from the restart session request if present.
        let working_dir = match restart_session {
            Some(restart_session) => match restart_session.working_directory {
                Some(working_directory) => match working_directory.is_empty() {
                    true => None,
                    false => Some(working_directory),
                },
                None => None,
            },
            None => None,
        };

        match session.restart(working_dir).await {
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
}
