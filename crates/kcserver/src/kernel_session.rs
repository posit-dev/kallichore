//
// kernel_session.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Wraps Jupyter kernel sessions.

use std::collections::HashMap;
use std::{fs, process::Stdio, sync::Arc};

use async_channel::{Receiver, SendError, Sender};
use chrono::{DateTime, Utc};
use event_listener::Event;
use kallichore_api::models::{self, StartupError};
use kallichore_api::models::{ConnectionInfo, VarAction};
use kcshared::{
    handshake_protocol::HandshakeStatus,
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    kernel_info::KernelInfoReply,
    kernel_message::{KernelMessage, OutputStream},
    websocket_message::WebsocketMessage,
};
use rand::Rng;
use std::iter;
use tokio::io::{AsyncBufReadExt, AsyncRead};
use tokio::sync::{oneshot, RwLock};

use crate::registration_socket::HandshakeResult;
use crate::{
    connection_file::ConnectionFile, error::KSError, kernel_connection::KernelConnection,
    kernel_state::KernelState, registration_file::RegistrationFile,
    registration_socket::RegistrationSocket, startup_status::StartupStatus,
    working_dir::expand_path, zmq_ws_proxy::ZmqWsProxy,
};

/// Escape a string for use in a shell command.
///
/// This function escapes a string so that it can be safely used as an argument
/// to a shell command. It uses single quotes to wrap the string, which is the
/// safest option in most Unix shells. If the string contains single quotes,
/// they are escaped by replacing them with the sequence '\'' (close quote,
/// escaped quote, open quote).
///
/// # Arguments
///
/// * `s` - The string to escape
///
/// # Returns
///
/// The escaped string
#[cfg(not(target_os = "windows"))]
fn escape_for_shell(s: &str) -> String {
    // If the string is empty, return ''
    if s.is_empty() {
        return "''".to_string();
    }

    // If the string doesn't contain any special characters,
    // we can return it as-is
    if !s.chars().any(|c| "\\\"'`${}()*?! \t\n;&|<>[]".contains(c)) {
        return s.to_string();
    }

    // Otherwise, wrap in single quotes and escape any internal single quotes
    let mut result = String::with_capacity(s.len() + 2);
    result.push('\'');

    // Replace any single quotes in the input with '\''
    for part in s.split('\'') {
        if !result.ends_with('\'') {
            result.push('\'');
        }

        result.push_str(part);

        if !part.is_empty() {
            result.push('\'');
        }

        // Add the escaped single quote sequence if this isn't the last part
        if part.len() < s.len() {
            result.push_str("\\'");
        }
    }

    // Ensure the string ends with a quote
    if !result.ends_with('\'') {
        result.push('\'');
    }

    result
}

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
            models::InterruptMode::Signal => match KernelSession::create_interrupt_event() {
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

    /// Strip startup markers from output to prevent them from leaking to users.
    ///
    /// The markers KALLICHORE_STARTUP_BEGIN and KALLICHORE_STARTUP_SUCCESS are used
    /// internally to determine what failed during startup, but should never be visible
    /// in user-facing output or error messages.
    fn strip_startup_markers(output: &str) -> String {
        output
            .lines()
            .filter(|line| {
                !line.contains("KALLICHORE_STARTUP_BEGIN")
                    && !line.contains("KALLICHORE_STARTUP_SUCCESS")
            })
            .collect::<Vec<_>>()
            .join("\n")
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
        let (connection_file_path, registration_socket, handshake_result_rx) = if jep66_enabled {
            log::info!(
                "[session {}] Kernel supports JEP 66 (protocol version {}) - creating registration socket for handshake",
                self.connection.session_id,
                self.connection.protocol_version
            );

            let (handshake_result_tx, handshake_result_rx) = oneshot::channel();

            // For JEP 66 handshaking, start a registration socket that immediately binds
            // to an OS selected port
            let registration_socket = match RegistrationSocket::new(handshake_result_tx).await {
                Ok(socket) => socket,
                Err(e) => {
                    return Err(StartupError {
                        exit_code: None,
                        output: None,
                        error: KSError::HandshakeFailed(
                            self.connection.session_id.clone(),
                            anyhow::anyhow!("Failed to create registration socket: {e}"),
                        )
                        .to_json(None),
                    })
                }
            };

            let registration_port = registration_socket.port();

            log::debug!(
                "Started registration socket for session {} with port {}",
                self.connection.session_id.clone(),
                registration_port
            );

            // Create the registration file name and path
            let mut registration_file_name = std::ffi::OsString::from("registration_");
            registration_file_name.push(self.connection.session_id.clone());
            registration_file_name.push(".json");
            let registration_path = std::env::temp_dir().join(registration_file_name);

            // Create the registration file
            let registration_file = RegistrationFile::new(
                "127.0.0.1".to_string(),
                registration_port,
                self.connection.key.clone(),
            );
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
                registration_port
            );

            (
                registration_path,
                Some(registration_socket),
                Some(handshake_result_rx),
            )
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
            (connection_path, None, None)
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

        // Start with a copy of the process environment
        let initial = std::env::vars();
        let mut resolved_env = HashMap::new();
        for (key, value) in initial {
            // Here and elsewhere, we convert the key to uppercase on Windows
            // since environment variables are case-insensitive on Windows to
            // ensure that references to the same variable are consistent.
            // The behavior of `spawn()` for multiple environment variables with
            // the same name but different cases is undefined on Windows.
            #[cfg(target_os = "windows")]
            let key = key.to_uppercase();
            #[cfg(not(target_os = "windows"))]
            let key = key;
            resolved_env.insert(key, value);
        }
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::DuplicateHandle;
            use windows::Win32::Foundation::DUPLICATE_SAME_ACCESS;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::Threading::GetCurrentProcess;

            // On Windows, if we have an interrupt event, add it directly to the environment.
            // The event was created with inheritable security attributes, so the child process
            // will inherit the handle automatically.
            if let Some(handle) = self.interrupt_event_handle {
                log::trace!(
                    "[session {}] Adding interrupt event handle to environment: {}",
                    self.connection.session_id,
                    handle
                );
                resolved_env.insert("JPY_INTERRUPT_EVENT".to_string(), format!("{}", handle));
                // Also set the legacy environment variable for backward compatibility
                resolved_env.insert("IPY_INTERRUPT_EVENT".to_string(), format!("{}", handle));
            }

            // Add the parent process handle to the environment for process monitoring
            #[allow(unsafe_code)]
            unsafe {
                let current_process = GetCurrentProcess();
                let mut target_handle = HANDLE::default();
                match DuplicateHandle(
                    current_process,                     // Source process handle
                    current_process, // Source handle to duplicate (current process)
                    current_process, // Target process handle
                    &mut target_handle, // Destination handle (out)
                    0,               // Desired access
                    windows::Win32::Foundation::BOOL(1), // Inheritable
                    DUPLICATE_SAME_ACCESS,
                ) {
                    Ok(_) => {
                        let handle = target_handle.0 as u64;
                        log::trace!(
                            "[session {}] Adding parent process handle to environment: {}",
                            self.connection.session_id,
                            handle
                        );
                        resolved_env.insert("JPY_PARENT_PID".to_string(), format!("{}", handle));
                    }
                    Err(e) => {
                        log::error!("Failed to duplicate parent process handle: {}", e);
                    }
                }
            }
        }

        // Read the set of environment variable actions from the state
        let env_var_actions = {
            let state = self.state.read().await;
            state.env_vars.clone()
        };

        // Apply mutations from model
        for action in &env_var_actions {
            #[cfg(target_os = "windows")]
            let env_key = action.name.to_uppercase();
            #[cfg(not(target_os = "windows"))]
            let env_key = action.name.clone();
            match action.action {
                models::VarActionType::Replace => {
                    resolved_env.insert(env_key, action.value.clone())
                }
                models::VarActionType::Append => {
                    let mut value = resolved_env.get(&env_key).unwrap_or(&String::new()).clone();
                    value.push_str(&action.value);
                    resolved_env.insert(env_key, value)
                }
                models::VarActionType::Prepend => {
                    let mut value = resolved_env.get(&env_key).unwrap_or(&String::new()).clone();
                    value.insert_str(0, &action.value);
                    resolved_env.insert(env_key, value)
                }
            };
        }

        // Store the resolved environment back in the kernel state so it can be
        // queried later
        {
            let mut state = self.state.write().await;
            state.resolved_env = resolved_env.clone();
        }

        // Validate startup_environment_arg usage
        let startup_env = &self.model.startup_environment;
        let startup_arg = &self.model.startup_environment_arg;

        // Warn if arg is specified but not needed
        if startup_arg.is_some() {
            match startup_env {
                models::StartupEnvironment::Command | models::StartupEnvironment::Script => {
                    // Valid - these modes require an arg
                }
                _ => {
                    log::warn!(
                        "[session {}] startup_environment_arg specified but startup_environment is {:?} (ignored)",
                        self.model.session_id,
                        startup_env
                    );
                }
            }
        }

        // Create the command to start the kernel with the processed arguments
        let shell_command = match &self.model.startup_environment {
            models::StartupEnvironment::None => {
                // No shell wrapper - use direct execution
                None
            }

            models::StartupEnvironment::Shell
            | models::StartupEnvironment::Command
            | models::StartupEnvironment::Script => {
                #[cfg(not(target_os = "windows"))]
                {
                    let candidates = vec![
                        std::env::var("SHELL").unwrap_or_else(|_| String::from("")),
                        String::from("/bin/bash"),
                        String::from("/bin/sh"),
                    ];

                    let mut login_shell = None;
                    for i in 0..candidates.len() {
                        let shell_path = &candidates[i];
                        // Ignore if empty (happens if SHELL is not set)
                        if shell_path.is_empty() {
                            continue;
                        }
                        // Found a valid shell path
                        if fs::metadata(&shell_path).is_ok() {
                            login_shell = Some(shell_path);
                            break;
                        } else if i == 0 {
                            // The first candidate comes from $SHELL. If it doesn't exist,
                            // log a warning but continue to try the others.
                            log::warn!(
                                "[session {}] Shell path specified in $SHELL '{}' does not exist",
                                self.model.session_id,
                                shell_path
                            );
                        }
                    }

                    // If we found a login shell, use it to run the kernel
                    if let Some(login_shell) = login_shell {
                        log::debug!(
                            "[session {}] Running kernel in login shell: {}",
                            self.model.session_id,
                            login_shell
                        );

                        // Build the base kernel command with escaping
                        let mut kernel_command = argv
                            .iter()
                            .map(|arg| escape_for_shell(arg))
                            .collect::<Vec<_>>()
                            .join(" ");

                        // On macOS, if the DYLD_LIBRARY_PATH environment variable was
                        // requested, set it explictly; it is not inherited by default
                        // in login shells due to SIP.
                        if let Some(dyld_path) = resolved_env.get("DYLD_LIBRARY_PATH") {
                            log::debug!(
                                "[session {}] Explicitly forwarding DYLD_LIBRARY_PATH: {}",
                                self.model.session_id,
                                dyld_path
                            );
                            kernel_command = format!(
                                "DYLD_LIBRARY_PATH={} {}",
                                escape_for_shell(dyld_path),
                                kernel_command
                            );
                        }

                        // Add startup command or script if specified
                        // We inject marker echo statements to help distinguish startup failures from kernel failures
                        kernel_command = match &self.model.startup_environment {
                            models::StartupEnvironment::Command => {
                                if let Some(cmd) = &self.model.startup_environment_arg {
                                    log::debug!(
                                        "[session {}] Executing startup command: {}",
                                        self.model.session_id,
                                        cmd
                                    );
                                    // Wrap command in markers to identify failure point
                                    format!(
                                        "echo 'KALLICHORE_STARTUP_BEGIN' && {{ {}; }} && echo 'KALLICHORE_STARTUP_SUCCESS' && {}",
                                        cmd,
                                        kernel_command
                                    )
                                } else {
                                    log::warn!(
                                        "[session {}] StartupEnvironment::Command specified but no command provided",
                                        self.model.session_id
                                    );
                                    kernel_command
                                }
                            }

                            models::StartupEnvironment::Script => {
                                if let Some(script_path_str) = &self.model.startup_environment_arg {
                                    // Resolve script path
                                    let script_path =
                                        if std::path::Path::new(script_path_str).is_absolute() {
                                            std::path::PathBuf::from(script_path_str)
                                        } else {
                                            // Resolve relative to working directory
                                            std::path::PathBuf::from(&working_directory)
                                                .join(script_path_str)
                                        };

                                    // Validate script exists and is a file
                                    match fs::metadata(&script_path) {
                                        Ok(metadata) => {
                                            if !metadata.is_file() {
                                                let err = KSError::ProcessStartFailed(
                                                    anyhow::anyhow!(
                                                        "Startup script path '{}' exists but is not a file",
                                                        script_path.display()
                                                    ),
                                                );
                                                return Err(StartupError {
                                                    exit_code: None,
                                                    output: None,
                                                    error: err.to_json(None),
                                                });
                                            }
                                        }
                                        Err(e) => {
                                            let err = KSError::ProcessStartFailed(anyhow::anyhow!(
                                                "Startup script not found or cannot be read: '{}' ({})",
                                                script_path.display(),
                                                e
                                            ));
                                            return Err(StartupError {
                                                exit_code: None,
                                                output: None,
                                                error: err.to_json(None),
                                            });
                                        }
                                    }

                                    log::debug!(
                                        "[session {}] Sourcing startup script: {}",
                                        self.model.session_id,
                                        script_path.display()
                                    );

                                    // Determine shell type for proper source command
                                    let source_cmd = match login_shell.split('/').last() {
                                        Some("csh") | Some("tcsh") => "source", // csh uses 'source'
                                        _ => ".", // sh/bash/zsh use '.' or 'source', '.' is more portable
                                    };

                                    // Wrap script in markers to identify failure point
                                    format!(
                                        "echo 'KALLICHORE_STARTUP_BEGIN' && {{ {} {}; }} && echo 'KALLICHORE_STARTUP_SUCCESS' && {}",
                                        source_cmd,
                                        escape_for_shell(&script_path.to_string_lossy()),
                                        kernel_command
                                    )
                                } else {
                                    log::warn!(
                                        "[session {}] StartupEnvironment::Script specified but no script path provided",
                                        self.model.session_id
                                    );
                                    kernel_command
                                }
                            }

                            _ => kernel_command, // Shell mode - no prefix
                        };

                        // Determine login argument based on shell type
                        let login_arg = match login_shell.split('/').last() {
                            None => "-l", // Unknown shell, presume bash-alike
                            Some(shell) => {
                                match shell {
                                    // csh-like shells don't support -c for login
                                    // shells. Instead, we emulate a login shell by
                                    // asking it to load the directory stack (-d)
                                    "csh" => "-d",
                                    "tcsh" => "-d",

                                    // Bash and zsh support the long-form --login option
                                    "bash" => "--login",
                                    "zsh" => "--login",

                                    // Sh only supports -l
                                    "dash" => "-l",
                                    "sh" => "-l",

                                    // For all other shells, presume -l
                                    _ => "-l",
                                }
                            }
                        };

                        // Create the shell command
                        let mut cmd = tokio::process::Command::new(login_shell);
                        cmd.args(&[login_arg, "-c", &kernel_command]);
                        Some(cmd)
                    } else {
                        log::warn!(
                            "[session {}] No valid login shell found; running kernel without a login shell",
                            self.model.session_id
                        );
                        None
                    }
                }

                #[cfg(target_os = "windows")]
                {
                    // On Windows, these modes have no effect
                    log::debug!(
                        "[session {}] startup_environment parameter ignored on Windows",
                        self.model.session_id
                    );
                    None
                }
            }
        };

        // If we formed a shell command, start the kernel with the shell
        // command; otherwise, start it with the original command line
        // arguments.
        let mut cmd = match shell_command {
            Some(command) => command,
            None => {
                let mut cmd = tokio::process::Command::new(&argv[0]);
                cmd.args(&argv[1..]);
                cmd
            }
        };

        // If a working directory was specified, test the working directory to
        // see if it exists. If it doesn't, log a warning and don't set the
        // process's working directory.
        if working_directory != "" {
            match fs::metadata(&working_directory) {
                Ok(metadata) => {
                    if metadata.is_dir() {
                        cmd.current_dir(&working_directory);
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
        let mut child = match cmd
            .envs(&resolved_env)
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
                "[session {}] Waiting for JEP 66 handshake",
                self.connection.session_id
            );

            // We unconditionally created these earlier in the other `jep66_enabled` path
            let registration_socket = registration_socket.unwrap();
            let handshake_result_rx = handshake_result_rx.unwrap();

            // Get the connection timeout from the model, defaulting to 30 seconds
            let connection_timeout = match self.model.connection_timeout {
                Some(timeout) => timeout as u64,
                None => 30,
            };

            // Wait for the handshake to complete
            match self
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

                    // Update the connection file with the negotiated ports
                    self.update_connection_file(connection_file).await;
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

                // Determine what failed by analyzing the output markers
                let error_context = match &self.model.startup_environment {
                    models::StartupEnvironment::Command => {
                        if output.contains("KALLICHORE_STARTUP_BEGIN") {
                            if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                                "Kernel started but failed to connect to ZeroMQ sockets"
                            } else {
                                "Startup command failed before kernel could start"
                            }
                        } else {
                            "Failed to initialize startup environment"
                        }
                    }

                    models::StartupEnvironment::Script => {
                        if output.contains("KALLICHORE_STARTUP_BEGIN") {
                            if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                                "Kernel started but failed to connect to ZeroMQ sockets"
                            } else {
                                "Startup script failed before kernel could start"
                            }
                        } else {
                            "Failed to initialize startup environment"
                        }
                    }

                    _ => "Kernel failed to connect to ZeroMQ sockets",
                };

                // Strip markers from output before showing to user
                let clean_output = Self::strip_startup_markers(&output);

                log::error!(
                    "[session {}] {}: {}",
                    self.connection.session_id.clone(),
                    error_context,
                    err
                );
                log::error!(
                    "[session {}] Output before failure: \n{}",
                    self.connection.session_id.clone(),
                    clean_output
                );

                Err(StartupError {
                    exit_code: Some(130),
                    output: Some(clean_output),
                    error: err.to_json(Some(error_context.to_string())),
                })
            }
            Ok(StartupStatus::AbnormalExit(exit_code, output, err)) => {
                // This error is emitted when the process exits before it
                // finishes starting.

                // Determine what failed by analyzing the output markers
                let error_context = match &self.model.startup_environment {
                    models::StartupEnvironment::Command => {
                        if output.contains("KALLICHORE_STARTUP_BEGIN") {
                            if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                                // Got past startup command, so kernel failed
                                String::from("Kernel failed to start")
                            } else {
                                // Startup command failed
                                if let Some(cmd) = &self.model.startup_environment_arg {
                                    format!("Startup command failed: '{}'", cmd)
                                } else {
                                    String::from("Startup command failed")
                                }
                            }
                        } else {
                            // Didn't even get to startup command (shell issue?)
                            String::from("Failed to initialize startup environment")
                        }
                    }

                    models::StartupEnvironment::Script => {
                        if output.contains("KALLICHORE_STARTUP_BEGIN") {
                            if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                                // Got past startup script, so kernel failed
                                String::from("Kernel failed to start")
                            } else {
                                // Startup script failed
                                if let Some(script) = &self.model.startup_environment_arg {
                                    format!("Startup script failed: '{}'", script)
                                } else {
                                    String::from("Startup script failed")
                                }
                            }
                        } else {
                            // Didn't even get to startup script (shell issue?)
                            String::from("Failed to initialize startup environment")
                        }
                    }

                    _ => String::from("Kernel failed to start"),
                };

                // Strip markers from output before showing to user
                let clean_output = Self::strip_startup_markers(&output);

                log::error!(
                    "[session {}] {}: exit code {}: {}",
                    self.connection.session_id.clone(),
                    error_context,
                    exit_code,
                    err
                );
                log::error!(
                    "[session {}] Output before exit: \n{}",
                    self.connection.session_id.clone(),
                    clean_output
                );

                Err(StartupError {
                    exit_code: Some(exit_code),
                    output: Some(clean_output),
                    error: err.to_json(Some(error_context)),
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
            // Save the kernel info to the state first
            {
                let mut state = self.state.write().await;
                state.set_kernel_info(value.clone());
            }

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
    /// * `working_directory` - The working directory to use after restart.
    /// Optional; if not supplied, the working directory supplied when the
    /// kernel was started will be used (Windows) or the kernel's current
    /// working directory will be used (non-Windows).
    ///
    /// * `env` - The new set of environment variable actions to apply to
    /// the kernel's environment.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the kernel was restarted successfully, or an error if the
    /// kernel could not be restarted.
    pub async fn restart(
        &self,
        working_directory: Option<String>,
        env: Option<Vec<VarAction>>,
    ) -> Result<(), StartupError> {
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

            // Set the environment variables, if supplied
            if let Some(env) = env {
                log::debug!(
                    "[session {}] Will restart with environment variables: {:?}",
                    self.connection.session_id,
                    env
                );
                state.env_vars = env;
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
            session_mode: self.model.session_mode.clone(),
            username: self.connection.username.clone(),
            display_name: self.model.display_name.clone(),
            language: self.model.language.clone(),
            interrupt_mode: self.model.interrupt_mode.clone(),
            initial_env: Some(state.resolved_env.clone()),
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
            notebook_uri: self.model.notebook_uri.clone(),
            started: self.started.clone(),
            status: state.status,
            execution_queue: state.execution_queue.to_json(),
            socket_path: state.client_socket_path.clone(),
            kernel_info: state.kernel_info.clone().unwrap_or(serde_json::json!({})),
        }
    }

    pub async fn interrupt(&self) -> Result<(), anyhow::Error> {
        match self.model.interrupt_mode {
            models::InterruptMode::Signal => {
                // On Windows, interrupts are signaled by setting a kernel event.
                //
                // Note that this requires coordination with the kernel to check
                // the event and interrupt itself. Currently, only ipykernel
                // supports this.
                #[cfg(windows)]
                {
                    use windows::Win32::Foundation::HANDLE;
                    use windows::Win32::System::Threading::SetEvent;
                    if let Some(handle_value) = self.interrupt_event_handle {
                        #[allow(unsafe_code)]
                        unsafe {
                            log::debug!(
                                "Setting interrupt event {} for session {}",
                                handle_value,
                                self.connection.session_id
                            );
                            // Convert the stored isize back to a HANDLE
                            // HANDLE is *mut c_void, so we preserve pointer semantics
                            let handle = HANDLE(handle_value as *mut std::ffi::c_void);
                            return match SetEvent(handle) {
                                Ok(_) => {
                                    log::debug!(
                                        "Successfully set interrupt event for session {}",
                                        self.connection.session_id
                                    );
                                    Ok(())
                                }
                                Err(e) => Err(anyhow::anyhow!(
                                    "Failed to set interrupt event: {}, process not interrupted",
                                    e
                                )),
                            };
                        }
                    } else {
                        return Err(anyhow::anyhow!("No interrupt event to set"));
                    }
                }

                // On Unix-alikes, interrupts are signaled by sending a SIGINT
                // signal to the process.
                #[cfg(not(windows))]
                {
                    use sysinfo::{Pid, Signal, System};
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
                // Save the kernel info to the state
                {
                    let mut state = self.state.write().await;
                    state.set_kernel_info(kernel_info.clone());
                }
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
     * Helper method to convert a KSError to a StartupError
     */
    fn ks_error_to_startup_error(&self, error: KSError) -> StartupError {
        StartupError {
            exit_code: None,
            output: None,
            error: error.to_json(None),
        }
    }

    /**
     * Wait for a handshake to be completed. This is used when starting a kernel that supports
     * JEP 66 handshaking.
     *
     * @param registration_socket The socket to use for the handshake procedure
     * @param handshake_result_rx The channel to receive the handshake result over
     * @param startup_rx The channel to receive startup status messages (including kernel exits)
     * @param timeout_secs Time to wait for the handshake in seconds
     */
    pub async fn wait_for_handshake(
        &self,
        registration_socket: RegistrationSocket,
        handshake_result_rx: oneshot::Receiver<HandshakeResult>,
        startup_rx: Receiver<StartupStatus>,
        timeout_secs: u64,
    ) -> Result<ConnectionFile, StartupError> {
        // Start listening on the registration socket in a separate thread
        registration_socket.listen(self.connection.clone());

        // Create a channel to listen for the handshake completed event
        let (handshake_tx, result_rx) = async_channel::bounded::<ConnectionFile>(1);

        // Monitor for handshake results from the registration socket
        let session_id = self.connection.session_id.clone();
        let connection_key = match &self.connection.key {
            Some(key) => key.clone(),
            None => String::new(),
        };

        tokio::spawn(async move {
            if let Ok(result) = handshake_result_rx.await {
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

        // Wait for the handshake to complete, kernel exit, or timeout
        let result = tokio::select! {
            handshake_result = result_rx.recv() => {
                match handshake_result {
                    Ok(connection_file) => {
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
                    Err(e) => {
                        // Error receiving from channel
                        log::error!(
                            "[session {}] Error waiting for handshake: {}",
                            self.connection.session_id,
                            e
                        );
                        Err(self.ks_error_to_startup_error(KSError::HandshakeFailed(
                            self.connection.session_id.clone(),
                            anyhow::anyhow!("Channel error: {}", e),
                        )))
                    }
                }
            }
            startup_status = startup_rx.recv() => {
                match startup_status {
                    Ok(StartupStatus::AbnormalExit(exit_code, output, error)) => {
                        // The kernel exited before the handshake could complete
                        log::error!(
                            "[session {}] Kernel exited during handshake with code {}: {}",
                            self.connection.session_id,
                            exit_code,
                            error
                        );
                        Err(StartupError {
                            exit_code: Some(exit_code),
                            output: Some(output),
                            error: KSError::ExitedBeforeConnection.to_json(None),
                        })
                    }
                    Ok(status) => {
                        // Other startup status (shouldn't happen during handshake)
                        log::warn!(
                            "[session {}] Unexpected startup status during handshake: {:?}",
                            self.connection.session_id,
                            status
                        );
                        Err(self.ks_error_to_startup_error(KSError::HandshakeFailed(
                            self.connection.session_id.clone(),
                            anyhow::anyhow!("Unexpected startup status"),
                        )))
                    }
                    Err(e) => {
                        // Error receiving from startup channel
                        log::error!(
                            "[session {}] Error receiving startup status during handshake: {}",
                            self.connection.session_id,
                            e
                        );
                        Err(self.ks_error_to_startup_error(KSError::HandshakeFailed(
                            self.connection.session_id.clone(),
                            anyhow::anyhow!("Startup channel error: {}", e),
                        )))
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs)) => {
                // Timeout waiting for handshake
                log::error!(
                    "[session {}] Timeout waiting for handshake",
                    self.connection.session_id
                );
                Err(self.ks_error_to_startup_error(KSError::HandshakeFailed(
                    self.connection.session_id.clone(),
                    anyhow::anyhow!("Timeout waiting for handshake"),
                )))
            }
        };

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
        log::trace!(
            "Released reserved ports for session {}; there are now {} reserved ports",
            self.connection.session_id,
            reserved_ports.len()
        );
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
    #[cfg(windows)]
    fn create_interrupt_event() -> Result<isize, anyhow::Error> {
        use windows::Win32::Security::SECURITY_ATTRIBUTES;
        use windows::Win32::System::Threading::CreateEventA;

        #[allow(unsafe_code)]
        unsafe {
            // Create a security attributes struct that permits inheritance of
            // the handle by new processes.
            let sa = SECURITY_ATTRIBUTES {
                #[allow(unused_qualifications)]
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: windows::Win32::Foundation::BOOL(1),
            };
            let event = CreateEventA(
                Some(&sa),
                windows::Win32::Foundation::BOOL(0), // bManualReset: false (auto-reset event)
                windows::Win32::Foundation::BOOL(0), // bInitialState: false (non-signaled)
                windows::core::PCSTR::null(),        // lpName: unnamed event
            );
            match event {
                Ok(handle) => {
                    let handle_value = handle.0 as isize;
                    log::debug!(
                        "Created interrupt event with handle value: {}",
                        handle_value
                    );
                    Ok(handle_value)
                }
                Err(e) => Err(anyhow::anyhow!("Failed to create interrupt event: {}", e)),
            }
        }
    }

    async fn get_connection_file(&self) -> Option<ConnectionFile> {
        let state = self.state.read().await;
        state.connection_file.clone()
    }

    pub async fn ensure_connection_file(&self) -> Result<ConnectionFile, anyhow::Error> {
        match self.get_connection_file().await {
            Some(connection_file) => {
                // We have a connection file; return it
                Ok(connection_file)
            }
            None => {
                let connection_file = ConnectionFile::generate(
                    "127.0.0.1".to_string(),
                    self.reserved_ports.clone(),
                    self.connection.key.clone(),
                )?;
                self.update_connection_file(connection_file.clone()).await;
                Ok(connection_file)
            }
        }
    }
}

pub fn make_message_id() -> String {
    let mut rng = rand::thread_rng();
    iter::repeat_with(|| format!("{:x}", rng.gen_range(0..16)))
        .take(10)
        .collect()
}
