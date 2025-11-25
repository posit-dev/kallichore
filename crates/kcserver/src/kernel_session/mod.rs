//
// mod.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Wraps Jupyter kernel sessions.

mod connection;
mod environment;
mod handshake;
mod lifecycle;
mod process;
mod shell_wrapper;
mod startup;
mod utils;

use std::sync::Arc;

use async_channel::{Receiver, Sender};
use chrono::{DateTime, Utc};
use event_listener::Event;
use kallichore_api::models::{self, StartupError};
use kcshared::{jupyter_message::JupyterMessage, websocket_message::WebsocketMessage};
use tokio::sync::RwLock;

use crate::{
    connection_file::ConnectionFile, kernel_connection::KernelConnection,
    kernel_state::KernelState, startup_status::StartupStatus,
};

use connection::ConnectionManager;
#[cfg(windows)]
use environment::create_interrupt_event;
use handshake::HandshakeCoordinator;
use lifecycle::LifecycleManager;
use process::ProcessMonitor;
use startup::StartupCoordinator;

// Re-export utility functions for external use
pub use utils::make_message_id;

/// A Jupyter kernel session.
///
/// This object represents an instance of Jupyter kernel. It consists of only
/// immutable state so that it can safely be cloned; all mutable kernel state is
/// stored in the `KernelState` object.
#[derive(Debug, Clone)]
pub struct KernelSession {
    /// Metadata about the session
    pub connection: KernelConnection,

    /// The session model that was used to create this session
    pub model: models::NewSession,

    /// The interrupt event handle, if we have one. Only used on Windows.
    #[cfg(windows)]
    pub interrupt_event_handle: Option<isize>,

    /// The command line arguments used to start the kernel. The first is the
    /// path to the kernel itself.
    pub argv: Vec<String>,

    /// The current state of the kernel
    pub state: Arc<RwLock<KernelState>>,

    /// The current set of reserved ports for all kernels
    pub reserved_ports: Arc<std::sync::RwLock<Vec<i32>>>,

    /// The date and time the kernel was started
    pub started: DateTime<Utc>,

    /// The channel to send JSON messages to the WebSocket
    pub ws_json_tx: Sender<WebsocketMessage>,

    /// The channel to receive JSON messages from the WebSocket
    pub ws_json_rx: Receiver<WebsocketMessage>,

    /// The channel to send ZMQ messages to the kernel
    pub ws_zmq_tx: Sender<JupyterMessage>,

    /// The channel to receive ZMQ messages from the kernel
    pub ws_zmq_rx: Receiver<JupyterMessage>,

    /// The exit event; fires when the kernel process exits
    pub exit_event: Arc<Event>,
}

impl KernelSession {
    /// Create a new kernel session.
    pub async fn new(
        session: models::NewSession,
        key: String,
        idle_nudge_tx: tokio::sync::mpsc::Sender<Option<u32>>,
        reserved_ports: Arc<std::sync::RwLock<Vec<i32>>>,
    ) -> Result<Self, anyhow::Error> {
        let (zmq_tx, zmq_rx) = async_channel::unbounded::<JupyterMessage>();
        let (json_tx, json_rx) = async_channel::unbounded::<WebsocketMessage>();
        let kernel_state = Arc::new(RwLock::new(KernelState::new(
            session.clone(),
            session.working_directory.clone(),
            idle_nudge_tx,
            json_tx.clone(),
        )));

        let connection = KernelConnection::from_session(&session, key.clone())?;
        let started = Utc::now();

        // On Windows, if the interrupt mode is Signal, create an event for
        // interruptions since Windows doesn't have signals.
        #[cfg(windows)]
        let interrupt_event_handle = match session.interrupt_mode {
            models::InterruptMode::Signal => match create_interrupt_event() {
                Ok(event) => Some(event),
                Err(e) => {
                    log::error!("Failed to create interrupt event: {}", e);
                    None
                }
            },
            models::InterruptMode::Message => None,
        };

        let kernel_session = KernelSession {
            argv: session.argv.clone(),
            state: kernel_state.clone(),
            ws_json_tx: json_tx.clone(),
            model: session,
            ws_json_rx: json_rx,
            ws_zmq_tx: zmq_tx,
            ws_zmq_rx: zmq_rx,
            connection,
            started,
            #[cfg(windows)]
            interrupt_event_handle,
            exit_event: Arc::new(Event::new()),
            reserved_ports,
        };
        Ok(kernel_session)
    }

    /// Start the kernel.
    ///
    /// # Returns
    ///
    /// The kernel info, as a JSON object.
    pub async fn start(&self) -> Result<serde_json::Value, StartupError> {
        // Create the startup coordinator
        let mut coordinator = StartupCoordinator {
            session_id: self.connection.session_id.clone(),
            model: self.model.clone(),
            state: self.state.clone(),
            argv: self.argv.clone(),
            #[cfg(windows)]
            interrupt_event_handle: self.interrupt_event_handle,
            shell_used: None,
            startup_command: None,
        };

        // Validate that the session can be started
        coordinator.validate_startup()?;

        // Mark the kernel as starting
        let working_directory = {
            let mut state = self.state.write().await;
            state
                .set_status(
                    models::Status::Starting,
                    Some(String::from("start API called")),
                )
                .await;
            state.working_directory.clone()
        };

        // Check if we expect JEP 66 handshaking based on protocol version
        let jep66_enabled =
            ConnectionFile::requires_handshaking(&self.connection.protocol_version.clone());

        // Set up connection or registration file
        let (connection_file_path, registration_socket, handshake_result_rx) = if jep66_enabled {
            log::info!(
                "[session {}] Kernel supports JEP 66 (protocol version {}) - creating registration socket for handshake",
                self.connection.session_id,
                self.connection.protocol_version
            );

            // Set up JEP 66 handshaking
            let handshake_coord = HandshakeCoordinator::new(
                self.connection.session_id.clone(),
                self.connection.clone(),
                self.ws_json_tx.clone(),
            );

            let (path, socket, rx) = handshake_coord.setup_registration_socket().await?;
            (path, Some(socket), Some(rx))
        } else {
            // Traditional kernel - generate connection file
            let connection_file = ConnectionFile::generate(
                "127.0.0.1".to_string(),
                self.reserved_ports.clone(),
                self.connection.key.clone(),
            )
            .map_err(|e| StartupError {
                exit_code: None,
                output: None,
                error: crate::error::KSError::ProcessStartFailed(anyhow::anyhow!(
                    "Failed to generate connection file: {}",
                    e
                ))
                .to_json(None),
            })?;

            // Store the connection file
            let conn_mgr = self.connection_manager();
            conn_mgr
                .update_connection_file(connection_file.clone())
                .await;

            // Write to disk
            let path = coordinator.write_connection_file(&connection_file, "connection")?;
            (path, None, None)
        };

        // Substitute connection file path in arguments
        let argv = coordinator.substitute_connection_file(&self.argv, &connection_file_path);

        log::debug!(
            "Starting kernel for session {}: {:?}",
            self.model.session_id,
            argv
        );

        // Resolve environment variables
        let resolved_env = coordinator.resolve_environment().await?;

        // Build the command
        let mut cmd = coordinator
            .build_command(&argv, &resolved_env, &working_directory)
            .await?;

        // Create a channel to receive startup status
        let (startup_tx, startup_rx) = async_channel::unbounded::<StartupStatus>();

        // Create the process monitor
        let process_monitor = ProcessMonitor::new(
            self.connection.session_id.clone(),
            self.state.clone(),
            self.exit_event.clone(),
            self.ws_json_tx.clone(),
        );

        // Spawn the kernel process
        let child = coordinator
            .spawn_kernel_process(&mut cmd, &resolved_env, &process_monitor)
            .await?;

        // Spawn a task to monitor the child process
        let kernel = self.clone();
        let startup_child_tx = startup_tx.clone();
        tokio::spawn(async move {
            let monitor = ProcessMonitor::new(
                kernel.connection.session_id.clone(),
                kernel.state.clone(),
                kernel.exit_event.clone(),
                kernel.ws_json_tx.clone(),
            );
            let consume_output = || kernel.consume_output_streams();
            monitor
                .run_child(child, startup_child_tx, consume_output)
                .await;
        });

        // If JEP 66 is enabled, wait for the handshake
        if jep66_enabled {
            log::info!(
                "[session {}] Waiting for JEP 66 handshake",
                self.connection.session_id
            );

            let registration_socket = registration_socket.unwrap();
            let handshake_result_rx = handshake_result_rx.unwrap();

            let connection_timeout = self.model.connection_timeout.unwrap_or(30) as u64;

            let handshake_coord = HandshakeCoordinator::new(
                self.connection.session_id.clone(),
                self.connection.clone(),
                self.ws_json_tx.clone(),
            );

            match handshake_coord
                .wait_for_handshake(
                    registration_socket,
                    handshake_result_rx,
                    startup_rx.clone(),
                    connection_timeout,
                )
                .await
            {
                Ok(connection_file) => {
                    log::info!(
                        "[session {}] JEP 66 handshake completed successfully",
                        self.connection.session_id
                    );
                    let conn_mgr = self.connection_manager();
                    conn_mgr.update_connection_file(connection_file).await;
                }
                Err(startup_error) => {
                    log::error!(
                        "[session {}] JEP 66 handshake failed: {:?}",
                        self.connection.session_id,
                        startup_error
                    );
                    return Err(startup_error);
                }
            }
        }

        // Start the ZeroMQ proxy
        let kernel = self.clone();
        let conn_mgr = self.connection_manager();
        let connection_file = match conn_mgr.get_connection_file().await {
            Some(cf) => cf,
            None => {
                log::error!(
                    "[session {}] Failed to get connection information!",
                    self.connection.session_id
                );
                return Err(StartupError {
                    exit_code: None,
                    output: None,
                    error: crate::error::KSError::ProcessStartFailed(anyhow::anyhow!(
                        "Failed to get connection file for ZeroMQ proxy"
                    ))
                    .to_json(None),
                });
            }
        };

        let startup_proxy_tx = startup_tx.clone();
        tokio::spawn(async move {
            kernel
                .start_zmq_proxy(connection_file.clone(), startup_proxy_tx)
                .await;
        });

        // Wait for the kernel to connect
        log::trace!(
            "[session {}] Waiting for kernel sockets to connect",
            self.connection.session_id
        );
        let startup_result = startup_rx.recv().await;
        log::trace!("[session {}] Waiting complete", self.connection.session_id);

        let result = coordinator.process_startup_result(startup_result)?;

        // Handle kernel info
        coordinator.handle_kernel_info(&result).await;

        Ok(result)
    }

    /// Shutdown the kernel.
    pub async fn shutdown(&self) -> Result<(), anyhow::Error> {
        let lifecycle = self.lifecycle_manager();
        lifecycle.shutdown().await
    }

    /// Restart the kernel.
    pub async fn restart(
        &self,
        working_directory: Option<String>,
        env: Option<Vec<models::VarAction>>,
    ) -> Result<(), StartupError> {
        let lifecycle = self.lifecycle_manager();
        lifecycle.restart(working_directory, env).await?;

        // Wait for the kernel to exit
        if !lifecycle.wait_for_exit_and_signal().await {
            return Ok(());
        }

        // Restart the kernel
        match self.start().await {
            Ok(_) => {
                log::debug!(
                    "[session {}] Kernel restarted successfully",
                    self.connection.session_id
                );
                Ok(())
            }
            Err(e) => {
                log::error!(
                    "[session {}] Failed to restart kernel: {}",
                    self.connection.session_id,
                    e.error.message
                );
                Err(e)
            }
        }
    }

    /// Interrupt the kernel.
    pub async fn interrupt(&self) -> Result<(), anyhow::Error> {
        let lifecycle = self.lifecycle_manager();
        lifecycle
            .interrupt(
                self.model.interrupt_mode.clone(),
                #[cfg(windows)]
                self.interrupt_event_handle,
            )
            .await
    }

    /// Connect to an existing kernel.
    ///
    /// This is used when adopting a kernel that is already running.
    pub async fn connect(
        &self,
        connection_file: ConnectionFile,
    ) -> Result<serde_json::Value, crate::error::KSError> {
        use crate::error::KSError;

        // Store the connection file
        let conn_mgr = self.connection_manager();
        conn_mgr
            .update_connection_file(connection_file.clone())
            .await;

        // Create a channel to receive startup status
        let (startup_tx, startup_rx) = async_channel::unbounded::<StartupStatus>();

        // Start the ZeroMQ proxy
        let kernel = self.clone();
        tokio::spawn(async move {
            log::debug!(
                "[session {}] Starting ZeroMQ proxy for adopted kernel",
                kernel.connection.session_id
            );

            kernel.start_zmq_proxy(connection_file, startup_tx).await;

            log::debug!(
                "[session {}] ZeroMQ proxy for adopted kernel has exited",
                kernel.connection.session_id
            );

            // Mark kernel as exited
            {
                let mut state = kernel.state.write().await;
                kernel.exit_event.notify(usize::MAX);
                state
                    .set_status(
                        models::Status::Exited,
                        Some(String::from(
                            "all sockets disconnected from an adopted kernel",
                        )),
                    )
                    .await;
            }

            // Fire the exit event
            let event = kcshared::kernel_message::KernelMessage::Exited(0);
            let _ = kernel
                .ws_json_tx
                .send(WebsocketMessage::Kernel(event))
                .await;
        });

        // Wait for the proxy to connect
        let startup_result = startup_rx.recv().await;
        match startup_result {
            Ok(StartupStatus::Connected(kernel_info)) => {
                log::trace!(
                    "[session {}] Kernel sockets connected successfully; kernel successfully adopted",
                    self.connection.session_id
                );
                // Save the kernel info
                {
                    let mut state = self.state.write().await;
                    state.set_kernel_info(kernel_info.clone());
                }
                Ok(kernel_info)
            }
            Ok(StartupStatus::ConnectionFailed(_, e)) => {
                log::error!(
                    "[session {}] Failed to connect to adopted kernel: {}",
                    self.connection.session_id,
                    e
                );
                Err(e)
            }
            Ok(StartupStatus::AbnormalExit(_, _, e)) => {
                log::error!(
                    "[session {}] Unexpected exit from adopted kernel: {}",
                    self.connection.session_id,
                    e
                );
                Err(e)
            }
            Err(e) => {
                log::error!(
                    "[session {}] Failed to connect to adopted kernel: {}",
                    self.connection.session_id,
                    e
                );
                Err(KSError::SessionConnectionFailed(anyhow::anyhow!("{}", e)))
            }
        }
    }

    /// Format this session as an active session.
    pub async fn as_active_session(&self) -> models::ActiveSession {
        let state = self.state.read().await;

        // Compute idle and busy times
        let idle_seconds = match state.idle_since {
            Some(instant) => instant.elapsed().as_secs() as i32,
            None => 0,
        };

        let busy_seconds = match state.busy_since {
            Some(instant) => instant.elapsed().as_secs() as i32,
            None => 0,
        };

        models::ActiveSession {
            session_id: self.connection.session_id.clone(),
            session_mode: self.model.session_mode.clone(),
            username: self.connection.username.clone(),
            display_name: self.model.display_name.clone(),
            language: self.model.language.clone(),
            interrupt_mode: self.model.interrupt_mode.clone(),
            initial_env: Some(state.resolved_env.clone()),
            argv: self.argv.clone(),
            process_id: state.process_id.map(|pid| pid as i32),
            input_prompt: state.input_prompt.clone(),
            idle_seconds,
            busy_seconds,
            continuation_prompt: state.continuation_prompt.clone(),
            connected: state.connected,
            working_directory: state.working_directory.clone(),
            notebook_uri: self.model.notebook_uri.clone(),
            started: self.started,
            status: state.status,
            execution_queue: state.execution_queue.to_json(),
            socket_path: state.client_socket_path.clone(),
            kernel_info: state.kernel_info.clone().unwrap_or(serde_json::json!({})),
        }
    }

    /// Ensure a connection file exists, generating one if necessary.
    pub async fn ensure_connection_file(&self) -> Result<ConnectionFile, anyhow::Error> {
        let conn_mgr = self.connection_manager();
        conn_mgr.ensure_connection_file().await
    }

    // Helper methods to create managers

    fn connection_manager(&self) -> ConnectionManager {
        ConnectionManager::new(
            self.connection.session_id.clone(),
            self.state.clone(),
            self.reserved_ports.clone(),
            self.connection.key.clone(),
        )
    }

    fn lifecycle_manager(&self) -> LifecycleManager {
        LifecycleManager::new(
            self.connection.session_id.clone(),
            self.state.clone(),
            self.ws_zmq_tx.clone(),
            self.exit_event.clone(),
            self.model.working_directory.clone(),
        )
    }

    /// Start the ZeroMQ proxy for this kernel session.
    async fn start_zmq_proxy(
        &self,
        connection_file: ConnectionFile,
        status_tx: Sender<StartupStatus>,
    ) {
        use crate::{error::KSError, zmq_ws_proxy::ZmqWsProxy};

        let mut proxy = ZmqWsProxy::new(
            connection_file.clone(),
            self.connection.clone(),
            self.state.clone(),
            self.ws_json_tx.clone(),
            self.ws_zmq_rx.clone(),
            self.exit_event.clone(),
        );

        // Wait for either the proxy to connect or for the session to exit
        let connect_or_exit = async {
            tokio::select! {
                result = proxy.connect() => {
                    match result {
                        Ok(_) => Ok(()),
                        Err(e) => Err(KSError::SessionConnectionFailed(e)),
                    }
                },
                _ = self.exit_event.listen() => {
                    Err(KSError::ExitedBeforeConnection)
                }
            }
        };

        let connection_timeout = self.model.connection_timeout.unwrap_or(30) as u64;

        match tokio::time::timeout(
            std::time::Duration::new(connection_timeout, 0),
            connect_or_exit,
        )
        .await
        {
            Ok(Ok(())) => {
                // Get kernel info
                match proxy.get_kernel_info().await {
                    Ok(info) => {
                        let _ = status_tx.send(StartupStatus::Connected(info)).await;
                    }
                    Err(e) => {
                        let error = KSError::SessionConnectionFailed(e);
                        error.log();
                        let output = self.consume_output_streams();
                        let _ = status_tx
                            .send(StartupStatus::ConnectionFailed(output, error))
                            .await;
                        return;
                    }
                }
            }
            Ok(Err(e)) => {
                e.log();
                let output = self.consume_output_streams();
                if matches!(e, KSError::SessionConnectionFailed(_)) {
                    let _ = status_tx
                        .send(StartupStatus::ConnectionFailed(output, e))
                        .await;
                }
                return;
            }
            Err(_) => {
                let error = KSError::SessionConnectionTimeout(connection_timeout as u32);
                error.log();
                let output = self.consume_output_streams();
                let _ = status_tx
                    .send(StartupStatus::ConnectionFailed(output, error))
                    .await;
                return;
            }
        }

        // Listen for messages
        match proxy.listen().await {
            Ok(_) => (),
            Err(e) => {
                let error = KSError::ZmqProxyError(e);
                error.log();
            }
        }

        // Release reserved ports
        let mut reserved_ports = self.reserved_ports.write().unwrap();
        reserved_ports.retain(|&port| {
            port != connection_file.info.control_port
                && port != connection_file.info.shell_port
                && port != connection_file.info.stdin_port
                && port != connection_file.info.iopub_port
                && port != connection_file.info.hb_port
        });
        log::trace!(
            "Released reserved ports for session {}; there are now {} reserved ports",
            self.connection.session_id,
            reserved_ports.len()
        );
    }

    /// Collect any standard out and standard error messages that were sent
    /// to the websocket during startup but haven't been delivered to the
    /// client.
    fn consume_output_streams(&self) -> String {
        use kcshared::kernel_message::KernelMessage;

        let mut output = String::new();
        while let Ok(msg) = self.ws_json_rx.try_recv() {
            if let WebsocketMessage::Kernel(KernelMessage::Output(_, text)) = msg {
                output.push_str(&text);
            }
        }
        output
    }
}
