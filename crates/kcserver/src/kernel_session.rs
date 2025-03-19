//
// kernel_session.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use std::{fs, process::Stdio, sync::Arc};

use async_channel::{Receiver, SendError, Sender};
use chrono::{DateTime, Utc};
use event_listener::Event;
use kallichore_api::models::ConnectionInfo;
use kallichore_api::models::{self, StartupError};
use kcshared::{
    handshake_protocol::HandshakeStatus,
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    kernel_info::KernelInfoReply,
    kernel_message::{KernelMessage, OutputStream},
    websocket_message::WebsocketMessage,
};
use rand::Rng;
use std::iter;
use sysinfo::{Pid, Signal, System};
use tokio::io::{AsyncBufReadExt, AsyncRead};
use tokio::sync::RwLock;

use crate::{
    connection_file::ConnectionFile, error::KSError, kernel_connection::KernelConnection,
    kernel_state::KernelState, registration_file::RegistrationFile,
    registration_socket::RegistrationSocket, startup_status::StartupStatus,
    working_dir::expand_path, zmq_ws_proxy::ZmqWsProxy,
};

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
        idle_nudge_tx: tokio::sync::mpsc::Sender<()>,
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
        // Ensure that we have some arguments. It is possible to create a session that has no
        // arguments (because it is intended to be started externally); these sessions can't be
        // started by the server.
        if self.argv.is_empty() {
            let err = KSError::ProcessStartFailed(anyhow::anyhow!("No arguments provided"));
            return Err(StartupError {
                exit_code: None,
                output: None,
                error: err.to_json(None),
            });
        }

        let working_directory = {
            // Mark the kernel as starting
            let mut state = self.state.write().await;
            state
                .set_status(
                    models::Status::Starting,
                    Some(String::from("start API called")),
                )
                .await;
            // Get the working directory
            state.working_directory.clone()
        };

        // First, check if we expect JEP 66 handshaking based on protocol version
        let jep66_enabled =
            ConnectionFile::requires_handshaking(&self.connection.protocol_version.clone());

        // Create a copy of argv where we substitute the connection file path. This needs to be done
        // before we start the kernel process.
        let mut argv = self.argv.clone();

        // Write the appropriate connection or registration file and get its path for substitution
        // in the kernel arguments
        let (connection_file_path, registration_port) = if jep66_enabled {
            // For JEP 66 handshaking, pick a free port for the registration socket to use
            let port = portpicker::pick_unused_port().ok_or_else(|| StartupError {
                exit_code: None,
                output: None,
                error: KSError::ProcessStartFailed(anyhow::anyhow!(
                    "Could not find a free port for the registration socket"
                ))
                .to_json(None),
            })?;

            // Create the registration file name and path
            let mut registration_file_name = std::ffi::OsString::from("registration_");
            registration_file_name.push(self.connection.session_id.clone());
            registration_file_name.push(".json");
            let registration_path = std::env::temp_dir().join(registration_file_name);

            // Create the registration file
            let registration_file =
                RegistrationFile::new("127.0.0.1".to_string(), port, self.connection.key.clone());
            registration_file
                .to_file(registration_path.clone())
                .map_err(|e| StartupError {
                    exit_code: None,
                    output: None,
                    error: KSError::ProcessStartFailed(anyhow::anyhow!(
                        "Failed to write registration file: {}",
                        e
                    ))
                    .to_json(None),
                })?;

            log::debug!(
                "Wrote registration file for session {} at {:?} with port {}",
                self.connection.session_id.clone(),
                registration_path,
                port
            );

            (registration_path, Some(port))
        } else {
            // For traditional kernels, generate a new connection file with allocated ports first
            let connection_file = ConnectionFile::generate(
                "127.0.0.1".to_string(),
                self.reserved_ports.clone(),
                self.connection.key.clone(),
            )
            .map_err(|e| StartupError {
                exit_code: None,
                output: None,
                error: KSError::ProcessStartFailed(anyhow::anyhow!(
                    "Failed to generate connection file: {}",
                    e
                ))
                .to_json(None),
            })?;

            // Store the generated connection file in our state
            self.update_connection_file(connection_file.clone()).await;

            // Write the connection file to disk
            let mut connection_file_name = std::ffi::OsString::from("connection_");
            connection_file_name.push(self.connection.session_id.clone());
            connection_file_name.push(".json");
            let connection_path = std::env::temp_dir().join(connection_file_name);

            connection_file
                .to_file(connection_path.clone())
                .map_err(|e| StartupError {
                    exit_code: None,
                    output: None,
                    error: KSError::ProcessStartFailed(anyhow::anyhow!(
                        "Failed to write connection file: {}",
                        e
                    ))
                    .to_json(None),
                })?;
            log::debug!(
                "Wrote connection file for session {} at {:?}",
                self.connection.session_id.clone(),
                connection_path
            );
            (connection_path, None)
        };

        // Substitute the connection file path in the arguments
        for arg in argv.iter_mut() {
            if arg.contains("{connection_file}") {
                *arg = arg.replace("{connection_file}", connection_file_path.to_str().unwrap());
            }
        }

        log::debug!(
            "Starting kernel for session {}: {:?}",
            self.model.session_id,
            argv
        );

        // Create the command to start the kernel with the processed arguments
        let mut command = tokio::process::Command::new(&argv[0]);
        command.args(&argv[1..]);

        // If a working directory was specified, test the working directory to
        // see if it exists. If it doesn't, log a warning and don't set the
        // process's working directory.
        if working_directory != "" {
            match fs::metadata(&working_directory) {
                Ok(metadata) => {
                    if metadata.is_dir() {
                        command.current_dir(&working_directory);
                        log::trace!(
                            "[session {}] Using working directory '{}'",
                            self.model.session_id.clone(),
                            working_directory
                        );
                    } else {
                        log::warn!(
                            "[session {}] Requested working directory '{}' is not a directory; using current directory '{}'",
                            self.model.session_id.clone(),
                            working_directory,
                            match std::env::current_dir() {
                                Ok(dir) => dir.display().to_string(),
                                Err(e) => format!("<error: {}>", e),
                            }
                        );
                    }
                }
                Err(e) => {
                    log::warn!(
                    "[session {}] Requested working directory '{}' could not be read ({}); using current directory '{}'",
                    self.model.session_id.clone(),
                    working_directory,
                    e,
                    match std::env::current_dir() {
                        Ok(dir) => dir.display().to_string(),
                        Err(e) => format!("<error: {}>", e),
                    }
                );
                }
            }
        }

        // Attempt to actually start the kernel process
        let mut child = match command
            .envs(&self.model.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to start kernel: {}", e);
                {
                    let mut state = self.state.write().await;
                    self.exit_event.notify(usize::MAX);
                    state
                        .set_status(
                            models::Status::Exited,
                            Some(String::from("kernel start failed")),
                        )
                        .await;
                }
                let err = KSError::ProcessStartFailed(anyhow::anyhow!("{}", e));
                return Err(StartupError {
                    exit_code: e.raw_os_error(),
                    output: None,
                    error: err.to_json(None),
                });
            }
        };

        // Capture the stdout and stderr of the child process and forward it to
        // the WebSocket
        let stdout = child
            .stdout
            .take()
            .expect("Failed to get stdout of child process");
        Self::stream_output(stdout, OutputStream::Stdout, self.ws_json_tx.clone());
        let stderr = child
            .stderr
            .take()
            .expect("Failed to get stderr of child process");
        Self::stream_output(stderr, OutputStream::Stderr, self.ws_json_tx.clone());

        // Get the process ID of the child process
        let pid = child.id();
        {
            // update the status of the session
            let mut state = self.state.write().await;
            state.process_id = pid;
            log::trace!(
                "[session {}]: Session child process started with pid {}",
                self.connection.session_id,
                match pid {
                    Some(pid) => pid.to_string(),
                    None => "<none>".to_string(),
                }
            );
        }

        // Create a channel to receive startup status from the kernel
        let (startup_tx, startup_rx) = async_channel::unbounded::<StartupStatus>();

        // Spawn a task to wait for the child process to exit
        let kernel = self.clone();
        let startup_child_tx = startup_tx.clone();
        tokio::spawn(async move {
            kernel.run_child(child, startup_child_tx).await;
        });

        if jep66_enabled {
            log::info!(
                "[session {}] Kernel supports JEP 66 (protocol version {}) - waiting for handshake",
                self.connection.session_id,
                self.connection.protocol_version
            );

            // Get the connection timeout from the model, defaulting to 30 seconds
            let connection_timeout = match self.model.connection_timeout {
                Some(timeout) => timeout as u64,
                None => 30,
            };

            // Wait for the handshake to complete using the port we already chose
            match self
                .wait_for_handshake(connection_timeout, registration_port.unwrap())
                .await
            {
                Ok(connection_file) => {
                    log::info!(
                        "[session {}] JEP 66 handshake completed successfully",
                        self.connection.session_id
                    );

                    // Update the connection file with the negotiated ports
                    self.update_connection_file(connection_file).await;
                }
                Err(e) => {
                    log::warn!(
                        "[session {}] JEP 66 handshake failed: {}",
                        self.connection.session_id,
                        e
                    );
                    log::info!(
                        "[session {}] Continuing with traditional connection method",
                        self.connection.session_id
                    );

                    // Allocate ports using the generate method
                    if let Some(connection_file) = self.get_connection_file().await {
                        if let Ok(new_connection_file) = ConnectionFile::generate(
                            connection_file.info.ip.clone(), // Use IP from the existing connection file
                            self.reserved_ports.clone(),
                            self.connection.key.clone(),
                        ) {
                            self.update_connection_file(new_connection_file).await;
                        } else {
                            log::error!(
                                "[session {}] Failed to allocate ports for traditional connection method",
                                self.connection.session_id
                            );
                            return Err(StartupError {
                                exit_code: None,
                                output: None,
                                error: KSError::SessionCreateFailed(
                                    self.connection.session_id.clone(),
                                    anyhow::anyhow!("Port allocation failed"),
                                )
                                .to_json(None),
                            });
                        }
                    } else {
                        // Allocate ports using the generate method
                        if let Ok(new_connection_file) = ConnectionFile::generate(
                            "127.0.0.1".to_string(), // Use 127.0.0.1 as the default IP
                            self.reserved_ports.clone(),
                            self.connection.key.clone(),
                        ) {
                            self.update_connection_file(new_connection_file).await;
                        } else {
                            log::error!(
                                "[session {}] Failed to allocate ports for traditional connection method",
                                self.connection.session_id
                            );
                            return Err(StartupError {
                                exit_code: None,
                                output: None,
                                error: KSError::SessionCreateFailed(
                                    self.connection.session_id.clone(),
                                    anyhow::anyhow!("Port allocation failed"),
                                )
                                .to_json(None),
                            });
                        }
                    }
                }
            }
        }

        // Spawn the ZeroMQ proxy thread
        let kernel = self.clone();
        let connection_file = match self.get_connection_file().await {
            Some(connection_file) => connection_file,
            None => {
                log::error!(
                    "[session {}] Failed to get connection information!",
                    self.connection.session_id
                );
                return Err(StartupError {
                    exit_code: None,
                    output: None,
                    error: KSError::ProcessStartFailed(anyhow::anyhow!(
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

        // Wait for either the session to connect to its sockets or for
        // something awful to happen
        log::trace!(
            "[session {}] Waiting for kernel sockets to connect",
            self.connection.session_id
        );
        let startup_result = startup_rx.recv().await;
        log::trace!("[session {}] Waiting complete", self.connection.session_id);

        let result = match startup_result {
            Ok(StartupStatus::Connected(kernel_info)) => {
                log::trace!(
                    "[session {}] Kernel sockets connected successfully, returning from start",
                    self.connection.session_id.clone()
                );
                Ok(kernel_info)
            }
            Ok(StartupStatus::ConnectionFailed(output, err)) => {
                // This error is emitted when the ZeroMQ proxy fails to connect
                // to the ZeroMQ sockets of the kernel.
                log::error!(
                    "[session {}] Startup failed. Can't connect to kernel: {}",
                    self.connection.session_id.clone(),
                    err
                );
                log::error!(
                    "[session {}] Output before failure: \n{}",
                    self.connection.session_id.clone(),
                    output
                );
                Err(StartupError {
                    exit_code: Some(130),
                    output: Some(output),
                    error: err.to_json(None),
                })
            }
            Ok(StartupStatus::AbnormalExit(exit_code, output, err)) => {
                // This error is emitted when the process exits before it
                // finishes starting.
                log::error!(
                    "[session {}] Startup failed; abnormal exit with code {}: {}",
                    self.connection.session_id.clone(),
                    exit_code,
                    err
                );
                log::error!(
                    "[session {}] Output before exit: \n{}",
                    self.connection.session_id.clone(),
                    output
                );
                Err(StartupError {
                    exit_code: Some(exit_code),
                    output: Some(output),
                    error: err.to_json(None),
                })
            }
            Err(e) => {
                let err = KSError::StartFailed(anyhow::anyhow!("{}", e));
                err.log();
                Err(StartupError {
                    exit_code: None,
                    output: None,
                    error: err.to_json(None),
                })
            }
        };

        // If the session reported kernel info, mine it to get the initial
        // values for our input and continuation prompts
        if let Ok(value) = &result {
            let kernel_info = serde_json::from_value::<KernelInfoReply>(value.clone());
            match kernel_info {
                Ok(info) => match info.language_info.positron {
                    Some(language_info) => {
                        // Write the input and continuation prompts to the kernel state
                        let mut state = self.state.write().await;
                        if let Some(input_prompt) = &language_info.input_prompt {
                            log::trace!(
                                "[session {}] Setting input prompt to '{}'",
                                self.connection.session_id,
                                input_prompt,
                            );
                            state.input_prompt = input_prompt.clone();
                        }
                        if let Some(continuation_promt) = &language_info.continuation_prompt {
                            log::trace!(
                                "[session {}] Setting continuation prompt to '{}'",
                                self.connection.session_id,
                                continuation_promt,
                            );
                            state.continuation_prompt = continuation_promt.clone();
                        }
                    }
                    None => {
                        // Not an error; not all kernels provide this
                        // information (it's a Posit specific extension)
                        log::trace!(
                            "[session {}] Kernel did not provide Positron language info",
                            self.connection.session_id
                        );
                    }
                },
                Err(e) => {
                    // If we got here, the kernel emitted kernel information but
                    // we could not parse it into our internal format. It's
                    // probably not compliant with the Jupyter spec. We'll still
                    // pass it to the client, but log a warning.
                    log::warn!(
                        "[session {}] Failed to parse kernel info: {} (content: {}); passing to client anyway",
                        self.connection.session_id,
                        serde_json::to_string(value).unwrap_or_else(|_| "<could not serialize>".to_string()),
                        e
                    );
                }
            }
        }

        // Return the result to the caller
        result
    }

    async fn run_child(&self, mut child: tokio::process::Child, startup_tx: Sender<StartupStatus>) {
        // Actually run the kernel! This will block until the kernel exits.
        let status = child.wait().await.expect("Failed to wait on child process");
        let code = status.code().unwrap_or(-1);

        log::info!(
            "Child process for session {} exited with status: {}",
            self.connection.session_id,
            status
        );

        // Check the kernel state. If we were still in the Starting state when
        // the process exited, that's bad.
        {
            let state = self.state.read().await;
            if state.status == models::Status::Starting {
                let output = self.consume_output_streams();
                startup_tx
                    .send(StartupStatus::AbnormalExit(
                        code,
                        output,
                        KSError::ProcessAbnormalExit(status),
                    ))
                    .await
                    .expect("Failed to send startup status");
            }
        }

        // We are now exited; mark the kernel as such
        {
            // update the status of the session
            let mut state = self.state.write().await;
            state
                .set_status(
                    models::Status::Exited,
                    Some(String::from("child process exited")),
                )
                .await;
        }

        // Notify anyone listening that the kernel has exited
        self.exit_event.notify(usize::MAX);

        let event = WebsocketMessage::Kernel(KernelMessage::Exited(code));
        self.ws_json_tx
            .send(event)
            .await
            .expect("Failed to send exit event to client");
    }

    pub async fn shutdown(&self) -> Result<(), anyhow::Error> {
        self.shutdown_request(false).await?;
        Ok(())
    }

    async fn shutdown_request(&self, restart: bool) -> Result<(), SendError<JupyterMessage>> {
        // Make and send the shutdown request.
        let msg = JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: make_message_id(),
                msg_type: "shutdown_request".to_string(),
            },
            parent_header: None,
            metadata: serde_json::json!({}),
            content: serde_json::json!({
                "restart": restart,
            }),
            channel: JupyterChannel::Control,
            buffers: vec![],
        };

        self.ws_zmq_tx.send(msg).await
    }

    /// Restart the kernel.
    ///
    /// # Arguments
    ///
    /// * `working_directory` - The working directory to use after restart. Optional; if not
    /// supplied, the working directory supplied when the kernel was started will be used (Windows)
    /// or the kernel's current working directory will be used (non-Windows).
    ///
    /// # Returns
    ///
    /// `Ok(())` if the kernel was restarted successfully, or an error if the
    /// kernel could not be restarted.
    pub async fn restart(&self, working_directory: Option<String>) -> Result<(), StartupError> {
        // Expand the working directory if it was supplied
        let working_directory = match working_directory {
            Some(dir) => match expand_path(dir.clone()) {
                Ok(dir) => Some(dir.to_string_lossy().to_string()),
                Err(e) => {
                    log::warn!(
                        "[session {}] Requested working directory '{}' could not be expanded: {} (ignoring)",
                        self.connection.session_id,
                        dir,
                        e
                    );
                    None
                }
            },
            None => None,
        };

        // Validate the working directory if it was supplied.
        let working_directory = match working_directory {
            Some(dir) => {
                // Test the working directory to see if it exists.
                match fs::metadata(dir.clone()) {
                    Ok(metadata) => {
                        if !metadata.is_dir() {
                            log::warn!(
                                "[session {}] Requested working directory '{}' is not a directory; ignoring",
                                self.connection.session_id,
                                dir
                            );
                            None
                        } else {
                            Some(dir)
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "[session {}] Requested working directory '{}' could not be read: {} (ignoring)",
                            self.connection.session_id,
                            dir,
                            e
                        );
                        None
                    }
                }
            }
            None => None,
        };

        // Enter the restarting state.
        {
            let mut state = self.state.write().await;
            if state.restarting {
                let err = KSError::RestartFailed(anyhow::anyhow!("Kernel is already restarting"));
                err.log();
                return Err(StartupError {
                    exit_code: None,
                    output: None,
                    error: err.to_json(None),
                });
            }

            // Set the working directory.
            match working_directory {
                Some(dir) => {
                    log::debug!(
                        "[session {}] Will restart in working directory '{}' (supplied by client)",
                        self.connection.session_id,
                        dir
                    );
                    state.working_directory = dir
                }
                None => {
                    #[cfg(not(target_os = "windows"))]
                    {
                        state.poll_working_dir().await;
                        log::debug!(
                            "[session {}] Will restart in working directory '{}' (read from OS)",
                            self.connection.session_id,
                            state.working_directory
                        );
                    }

                    #[cfg(target_os = "windows")]
                    {
                        state.working_directory = self.model.working_directory.clone();
                        log::debug!(
                            "[session {}] Will restart in working directory '{}' (original)",
                            self.connection.session_id,
                            state.working_directory
                        );
                    }
                }
            }
            state.restarting = true;
        }

        match self.shutdown_request(true).await {
            Ok(_) => {
                log::debug!("Preparing for restart; sent shutdown request to kernel");
            }
            Err(e) => {
                // Leave the restarting state since we failed to send the
                // shutdown request.
                {
                    let mut state = self.state.write().await;
                    state.restarting = false;
                }
                let err = KSError::RestartFailed(anyhow::anyhow!(
                    "Failed to send shutdown request to kernel"
                ));
                return Err(StartupError {
                    exit_code: None,
                    output: None,
                    error: err.to_json(Some(format!("{}", e))),
                });
            }
        }

        // Spawn a task to wait for tho kernel to exit; when it does, complete
        // the restart by starting it again.
        log::debug!(
            "[session {}] Waiting for kernel to exit before restarting",
            self.connection.session_id
        );
        return self.complete_restart().await;
    }

    /// Complete restart by waiting for the kernel to exit and then starting it
    /// again.
    async fn complete_restart(&self) -> Result<(), StartupError> {
        // Wait for the kernel to exit
        let listener = self.exit_event.listen();
        listener.await;

        // Make sure the kernel is still restarting, and then clear the
        // restarting flag.
        {
            let mut state = self.state.write().await;
            if !state.restarting {
                log::debug!(
                    "[session {}] Kernel is no longer restarting; stopping restart",
                    self.connection.session_id
                );
                return Ok(());
            }
            state.restarting = false;
        }

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
            username: self.connection.username.clone(),
            display_name: self.model.display_name.clone(),
            language: self.model.language.clone(),
            interrupt_mode: self.model.interrupt_mode.clone(),
            initial_env: Some(self.model.env.clone()),
            argv: self.argv.clone(),
            process_id: match state.process_id {
                Some(pid) => Some(pid as i32),
                None => None,
            },
            input_prompt: state.input_prompt.clone(),
            idle_seconds,
            busy_seconds,
            continuation_prompt: state.continuation_prompt.clone(),
            connected: state.connected,
            working_directory: state.working_directory.clone(),
            started: self.started.clone(),
            status: state.status,
            execution_queue: state.execution_queue.to_json(),
        }
    }

    pub async fn interrupt(&self) -> Result<(), anyhow::Error> {
        match self.model.interrupt_mode {
            models::InterruptMode::Signal => {
                let pid = self.state.read().await.process_id.unwrap_or(0);
                if pid == 0 {
                    return Err(anyhow::anyhow!("No process ID to interrupt"));
                }
                let mut system = System::new();
                let pid = Pid::from_u32(pid);
                system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]));
                if let Some(process) = system.process(pid) {
                    process.kill_with(Signal::Interrupt);
                } else {
                    return Err(anyhow::anyhow!("Process {} not found", pid));
                }
            }
            models::InterruptMode::Message => {
                let msg = JupyterMessage {
                    header: JupyterMessageHeader {
                        msg_id: make_message_id(),
                        msg_type: "interrupt_request".to_string(),
                    },
                    parent_header: None,
                    metadata: serde_json::json!({}),
                    content: serde_json::json!({}),
                    channel: JupyterChannel::Control,
                    buffers: vec![],
                };
                self.ws_zmq_tx.send(msg).await?;
            }
        }
        Ok(())
    }

    /// Stream output from a child process to the WebSocket.
    ///
    /// This function reads lines from a stream and sends them to the WebSocket. It's used to forward
    /// the stdout and stderr of a child process to the client.
    ///
    /// # Arguments
    ///
    /// - `stream`: The stream to read from
    /// - `kind`: The kind of output (stdout or stderr)
    /// - `ws_json_tx`: The channel to send JSON messages to the WebSocket
    fn stream_output<T: AsyncRead + Unpin + Send + 'static>(
        stream: T,
        kind: OutputStream,
        ws_json_tx: Sender<WebsocketMessage>,
    ) {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(Box::pin(stream));
            let mut buffer = String::new();
            loop {
                buffer.clear();
                match reader.read_line(&mut buffer).await {
                    Ok(0) => {
                        log::debug!("End of output stream (kind: {:?})", kind);
                        break;
                    }
                    Ok(_) => {
                        let message = WebsocketMessage::Kernel(KernelMessage::Output(
                            kind.clone(),
                            buffer.to_string(),
                        ));
                        ws_json_tx
                            .send(message)
                            .await
                            .expect("Failed to send standard stream message to client");
                    }
                    Err(e) => {
                        log::error!("Failed to read from standard stream: {}", e);
                        break;
                    }
                }
            }
        });
    }

    /**
     * Connect to the kernel. This is only used when connecting to a kernel that is already running
     * (i.e. when adopting a kernel).
     */
    pub async fn connect(
        &self,
        connection_file: ConnectionFile,
    ) -> Result<serde_json::Value, KSError> {
        // Store the connection file
        // Since we're adopting an existing kernel, we always have a full connection file
        self.update_connection_file(connection_file.clone()).await;

        // Create a channel to receive startup status from the kernel.
        let (startup_tx, startup_rx) = async_channel::unbounded::<StartupStatus>();

        // Attempt to start the ZeroMQ proxy.
        let kernel = self.clone();
        tokio::spawn(async move {
            log::debug!(
                "[session {}] Starting ZeroMQ proxy for adopted kernel",
                kernel.connection.session_id.clone()
            );

            // Start the proxy. The proxy runs until all sockets are disconnected.
            kernel.start_zmq_proxy(connection_file, startup_tx).await;

            log::debug!(
                "[session {}] ZeroMQ proxy for adopted kernel has exited",
                kernel.connection.session_id.clone()
            );

            // Since this kernel has no backing process, once all the sockets are disconnected, we
            // should treat the kernel as exited.
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

            // Fire the exit event; use a code of 0 since there's no such thing as a non-zero exit
            // for an adopted kernel
            let event = WebsocketMessage::Kernel(KernelMessage::Exited(0));
            kernel
                .ws_json_tx
                .send(event)
                .await
                .expect("Failed to send exit event to client");
        });

        // Wait for the proxy to connect
        let startup_result = startup_rx.recv().await;
        let result = match startup_result {
            Ok(StartupStatus::Connected(kernel_info)) => {
                log::trace!(
                    "[session {}] Kernel sockets connected successfully; kernel successfully adopted",
                    self.connection.session_id.clone()
                );
                Ok(kernel_info)
            }
            Ok(StartupStatus::ConnectionFailed(_output, e)) => {
                // Ignore the output; we can't capture output from an adopted kernel so it'll be
                // empty
                log::error!(
                    "[session {}] Failed to connect to adopted kernel: {}",
                    self.connection.session_id.clone(),
                    e
                );
                Err(e)
            }
            Ok(StartupStatus::AbnormalExit(_, _, e)) => {
                // We don't expect an adopted kernel to exit before connecting; in fact, we can't
                // even detect it since we don't have the process ID, so this should be considered
                // an error.
                log::error!(
                    "[session {}] Unexpected exit from adopted kernel: {}",
                    self.connection.session_id.clone(),
                    e
                );
                Err(e)
            }
            Err(e) => {
                log::error!(
                    "[session {}] Failed to connect to adopted kernel: {}",
                    self.connection.session_id.clone(),
                    e
                );
                Err(KSError::SessionConnectionFailed(anyhow::anyhow!("{}", e)))
            }
        };
        result
    }

    /**
     * Update the connection file for this kernel session
     * Used when connection details become available after handshaking
     */
    pub async fn update_connection_file(&self, connection_file: ConnectionFile) {
        let mut state = self.state.write().await;
        state.connection_file = Some(connection_file);
    }

    /**
     * Wait for a handshake to be completed. This is used when starting a kernel that supports
     * JEP 66 handshaking.
     *
     * @param timeout_secs Time to wait for the handshake in seconds
     * @param port The port to use for the registration socket (already chosen earlier)
     */
    pub async fn wait_for_handshake(
        &self,
        timeout_secs: u64,
        port: u16,
    ) -> Result<ConnectionFile, KSError> {
        // Create a new registration socket for this session with the already chosen port
        let mut registration_socket = RegistrationSocket::new(port, self.connection.clone());

        // Start the registration socket
        if let Err(e) = registration_socket.start().await {
            return Err(KSError::HandshakeFailed(
                self.connection.session_id.clone(),
                anyhow::anyhow!("Failed to start registration socket: {}", e),
            ));
        }

        // Get a receiver for handshake results
        let mut handshake_rx = registration_socket.get_result_receiver();

        // Create a channel to listen for the handshake completed event
        let (handshake_tx, result_rx) = async_channel::bounded::<ConnectionFile>(1);

        // Monitor for handshake results from the registration socket
        let session_id = self.connection.session_id.clone();
        let connection_key = match &self.connection.key {
            Some(key) => key.clone(),
            None => String::new(),
        };

        tokio::spawn(async move {
            if let Ok(result) = handshake_rx.recv().await {
                if result.status == HandshakeStatus::Ok {
                    // Create connection info from the handshake request
                    let info = ConnectionInfo {
                        shell_port: result.request.shell_port as i32,
                        iopub_port: result.request.iopub_port as i32,
                        stdin_port: result.request.stdin_port as i32,
                        control_port: result.request.control_port as i32,
                        hb_port: result.request.hb_port as i32,
                        transport: "tcp".to_string(),
                        signature_scheme: "hmac-sha256".to_string(),
                        key: connection_key,
                        ip: "127.0.0.1".to_string(),
                    };

                    // Create a connection file from the connection info
                    let connection_file = ConnectionFile::from_info(info);

                    // Send the connection file to the waiting thread
                    if let Err(e) = handshake_tx.send(connection_file).await {
                        log::warn!(
                            "[session {}] Failed to send handshake result: {}",
                            session_id,
                            e
                        );
                    }
                } else {
                    log::warn!("[session {}] Received failed handshake result", session_id);
                }
            }
        });

        // Wait for the handshake to complete or for a timeout
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            result_rx.recv(),
        )
        .await
        {
            Ok(Ok(connection_file)) => {
                // Handshake completed successfully
                log::info!(
                    "[session {}] Handshake completed successfully",
                    self.connection.session_id
                );

                // Create a new connection file with the key included
                let mut connection_file = connection_file;
                connection_file.info.key = match &self.connection.key {
                    Some(key) => key.clone(),
                    None => String::new(),
                };

                // Create an event message to send through the websocket
                let msg = WebsocketMessage::Kernel(KernelMessage::HandshakeCompleted(
                    self.connection.session_id.clone(),
                    connection_file.info.clone(),
                ));

                // Send the event through the websocket channel
                if let Err(e) = self.ws_json_tx.send(msg).await {
                    log::warn!(
                        "[session {}] Failed to send handshake completed message: {}",
                        self.connection.session_id,
                        e
                    );
                }

                Ok(connection_file)
            }
            Ok(Err(e)) => {
                // Error receiving from channel
                log::error!(
                    "[session {}] Error waiting for handshake: {}",
                    self.connection.session_id,
                    e
                );
                Err(KSError::HandshakeFailed(
                    self.connection.session_id.clone(),
                    anyhow::anyhow!("Channel error: {}", e),
                ))
            }
            Err(_) => {
                // Timeout waiting for handshake
                log::error!(
                    "[session {}] Timeout waiting for handshake",
                    self.connection.session_id
                );
                Err(KSError::HandshakeFailed(
                    self.connection.session_id.clone(),
                    anyhow::anyhow!("Timeout waiting for handshake"),
                ))
            }
        };

        // Stop the registration socket
        registration_socket.stop().await;

        result
    }

    async fn start_zmq_proxy(
        &self,
        connection_file: ConnectionFile,
        status_tx: Sender<StartupStatus>,
    ) {
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
                        Ok(()) => {
                            // The proxy connected successfully.
                            log::debug!(
                                "[session {}] All ZeroMQ sockets connected successfully",
                                self.connection.session_id
                            );
                            Ok(())
                        }
                        Err(e) => {
                            // The proxy failed to connect.
                            Err(KSError::SessionConnectionFailed(e))
                        }
                    }
                },
                _ = self.exit_event.listen() => {
                    // The session exited before the proxy could connect.
                    Err(KSError::ExitedBeforeConnection)
                }
            }
        };

        // Read the timeout from the model, defaulting to 30 seconds
        let connection_timeout = match self.model.connection_timeout {
            Some(timeout) => timeout as u64,
            None => 30,
        };

        // Wait for the proxy to connect or for the session to exit
        match tokio::time::timeout(
            std::time::Duration::new(connection_timeout, 0),
            connect_or_exit,
        )
        .await
        {
            Ok(Ok(())) => {
                // Get the kernel info from the shell channel
                let kernel_info = proxy.get_kernel_info().await;
                match kernel_info {
                    Ok(info) => {
                        log::trace!(
                            "[session {}] Kernel info received: {:?}",
                            self.connection.session_id,
                            info
                        );

                        // JEP 66 handshaking is done through the registration socket, not directly here.
                        // The kernel would have connected to the registration socket before starting.
                        // At this point, we're just confirming we have a successful connection
                        // through the traditional Jupyter protocol sockets.

                        log::debug!(
                            "[session {}] Successfully connected to kernel using traditional Jupyter protocol",
                            self.connection.session_id
                        );

                        status_tx
                            .send(StartupStatus::Connected(info))
                            .await
                            .expect("Failed to send startup status");
                    }
                    Err(e) => {
                        let error = KSError::NoKernelInfo(e);
                        let output = self.consume_output_streams();
                        status_tx
                            .send(StartupStatus::ConnectionFailed(output, error))
                            .await
                            .expect("Failed to send startup status");
                    }
                }
            }
            Ok(Err(e)) => {
                // If the connection failed, send an error status to the caller.
                // We could also get here if the session exits before it can
                // connect; in that case we don't need to send a status since
                // the exit event sends one from the thread monitoring the child
                // process.
                e.log();
                let output = self.consume_output_streams();
                if let KSError::SessionConnectionFailed(_) = e {
                    status_tx
                        .send(StartupStatus::ConnectionFailed(output, e))
                        .await
                        .expect("Could not send startup status");
                }
                return;
            }
            Err(_) => {
                // If the connection timed out, send an error status to the caller
                let error = KSError::SessionConnectionTimeout(connection_timeout as u32);
                error.log();
                let output = self.consume_output_streams();
                status_tx
                    .send(StartupStatus::ConnectionFailed(output, error))
                    .await
                    .expect("Could not send startup status");
                return;
            }
        }

        // Listen for messages from the ZeroMQ sockets and forward them to the
        // WebSocket channel. Doesn't return until the proxy stops.
        match proxy.listen().await {
            Ok(_) => (),
            Err(e) => {
                let error = KSError::ZmqProxyError(e);
                error.log();
            }
        }

        // When this listen future resolves, the proxy has stopped and the
        // sockets are closed; release the reserved ports
        let mut reserved_ports = self.reserved_ports.write().unwrap();
        reserved_ports.retain(|&port| {
            port != connection_file.info.control_port
                && port != connection_file.info.shell_port
                && port != connection_file.info.stdin_port
                && port != connection_file.info.iopub_port
                && port != connection_file.info.hb_port
        });
        {
            let reserved_ports = self.reserved_ports.read().unwrap();
            log::trace!(
                "Released reserved ports for session {}; there are now {} reserved ports",
                self.connection.session_id,
                reserved_ports.len()
            );
        }
    }

    /// Collect any standard out and standard error messages that were sent
    /// to the websocket during startup but haven't been delivered to the
    /// client. (The client typically doesn't connect to the websocket until
    /// the kernel has started, so we expect there to be some if the kernel
    /// emitted any startup errors.)
    fn consume_output_streams(&self) -> String {
        let mut output = String::new();
        while let Ok(msg) = self.ws_json_rx.try_recv() {
            if let WebsocketMessage::Kernel(KernelMessage::Output(_, text)) = msg {
                output.push_str(&text);
            }
        }
        output
    }

    pub async fn get_connection_file(&self) -> Option<ConnectionFile> {
        let state = self.state.read().await;
        state.connection_file.clone()
    }
}

pub fn make_message_id() -> String {
    let mut rng = rand::thread_rng();
    iter::repeat_with(|| format!("{:x}", rng.gen_range(0..16)))
        .take(10)
        .collect()
}
