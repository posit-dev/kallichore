//
// server.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
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
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use kallichore_api::{
    models, AdoptSessionResponse, ChannelsWebsocketResponse, DeleteSessionResponse,
    GetSessionResponse, InterruptSessionResponse, KillSessionResponse, NewSessionResponse,
    RestartSessionResponse, ShutdownServerResponse, StartSessionResponse,
};

pub async fn create(addr: &str, token: Option<String>) {
    let addr = addr.parse().expect("Failed to parse bind address");

    let server = Server::new(token);

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
    reserved_ports: Arc<RwLock<Vec<u16>>>,
}

impl<C> Server<C> {
    pub fn new(token: Option<String>) -> Self {
        Server {
            token,
            started_time: std::time::Instant::now(),
            marker: PhantomData,
            kernel_sessions: Arc::new(RwLock::new(vec![])),
            client_sessions: Arc::new(RwLock::new(vec![])),
            reserved_ports: Arc::new(RwLock::new(vec![])),
        }
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

use kallichore_api::server::MakeService;
use kallichore_api::{Api, ListSessionsResponse};
use std::error::Error;
use swagger::ApiError;

use crate::client_session::ClientSession;
use crate::connection_file::{self, ConnectionFile};
use crate::error::KSError;
use crate::kernel_session::{self, KernelSession};
use crate::zmq_ws_proxy::{self, ZmqWsProxy};

impl<C> Server<C> {}

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
        let connection_file = match ConnectionFile::generate(
            String::from("127.0.0.1"),
            self.reserved_ports.clone(),
        ) {
            Ok(connection_file) => connection_file,
            Err(e) => {
                let error = KSError::SessionCreateFailed(
                    new_session_id.clone(),
                    anyhow!("Couldn't create connection file: {}", e),
                );
                error.log();
                return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
            }
        };

        let temp_dir = env::temp_dir();
        let mut connection_file_name = std::ffi::OsString::from("connection_");
        connection_file_name.push(new_session_id.clone());
        connection_file_name.push(".json");

        // Combine the temporary directory with the file name to get the full path
        let connection_path: PathBuf = temp_dir.join(connection_file_name);
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
        };

        let sessions = self.kernel_sessions.clone();
        let kernel_session = match KernelSession::new(
            session,
            connection_file.clone(),
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
        adopted_session: models::AdoptedSession,
        _context: &C,
    ) -> Result<AdoptSessionResponse, ApiError> {
        info!("adopt_session not yet implemented");
        Ok(AdoptSessionResponse::SessionID(NewSession200Response {
            session_id: adopted_session.session.session_id,
        }))
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
        match session.restart().await {
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
        info!("shutdown server");

        // Token validation
        if !self.validate_token(context) {
            return Ok(ShutdownServerResponse::Unauthorized);
        }

        // Create a vector of all the kernel sessions that are currently running
        let running_sessions = {
            let kernel_sessions = self.kernel_sessions.read().unwrap().clone();
            let mut running_sessions: Vec<KernelSession> = vec![];
            for session in kernel_sessions.iter() {
                let running = session.state.read().await.status != models::Status::Exited;
                if running {
                    running_sessions.push(session.clone());
                }
            }
            running_sessions
        };

        // Early exit if all sessions are already stopped
        if running_sessions.is_empty() {
            tokio::spawn(async move {
                // Wait 1 second before exiting
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                log::info!("All sessions have exited; exiting Kallichore server.");
                std::process::exit(0);
            });
            return Ok(ShutdownServerResponse::ShuttingDown(
                serde_json::Value::Null,
            ));
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

            loop {
                // Wait for any of the exit events to be triggered
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
                    return Ok(ShutdownServerResponse::ShutdownFailed(error.to_json(None)));
                }
            }
        }

        Ok(ShutdownServerResponse::ShuttingDown(
            serde_json::Value::Null,
        ))
    }
}
