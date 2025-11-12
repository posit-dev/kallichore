//
// startup.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Kernel startup logic and coordination.

use kallichore_api::models::{self, StartupError};
use kcshared::kernel_info::KernelInfoReply;
use std::collections::HashMap;
use std::sync::Arc;
use std::{fs, process::Stdio};
use tokio::sync::RwLock;

use crate::{
    connection_file::ConnectionFile, error::KSError, kernel_state::KernelState,
    startup_status::StartupStatus,
};

use super::environment::EnvironmentResolver;
use super::process::ProcessMonitor;
use super::shell_wrapper::ShellCommandBuilder;
use super::utils::strip_startup_markers;

/// Coordinates the kernel startup process.
pub struct StartupCoordinator {
    /// Session ID for logging
    pub session_id: String,

    /// Session model
    pub model: models::NewSession,

    /// Shared kernel state
    pub state: Arc<RwLock<KernelState>>,

    /// Kernel command line arguments
    pub argv: Vec<String>,

    /// Interrupt event handle (Windows only)
    #[cfg(windows)]
    pub interrupt_event_handle: Option<isize>,
}

impl StartupCoordinator {
    /// Validate that the session can be started.
    pub fn validate_startup(&self) -> Result<(), StartupError> {
        // Ensure that we have some arguments
        if self.argv.is_empty() {
            let err = KSError::ProcessStartFailed(anyhow::anyhow!("No arguments provided"));
            return Err(StartupError {
                exit_code: None,
                output: None,
                error: err.to_json(None),
            });
        }

        // Validate startup_environment_arg usage
        if self.model.startup_environment_arg.is_some() {
            match self.model.startup_environment {
                models::StartupEnvironment::Command | models::StartupEnvironment::Script => {
                    // Valid - these modes require an arg
                }
                _ => {
                    log::warn!(
                        "[session {}] startup_environment_arg specified but startup_environment is {:?} (ignored)",
                        self.session_id,
                        self.model.startup_environment
                    );
                }
            }
        }

        Ok(())
    }

    /// Substitute the connection file path in argv.
    pub fn substitute_connection_file(
        &self,
        argv: &[String],
        connection_file_path: &std::path::Path,
    ) -> Vec<String> {
        argv.iter()
            .map(|arg| {
                if arg.contains("{connection_file}") {
                    arg.replace("{connection_file}", connection_file_path.to_str().unwrap())
                } else {
                    arg.clone()
                }
            })
            .collect()
    }

    /// Resolve the environment variables for the kernel process.
    pub async fn resolve_environment(&self) -> Result<HashMap<String, String>, StartupError> {
        // Read the set of environment variable actions from the state
        let env_var_actions = {
            let state = self.state.read().await;
            state.env_vars.clone()
        };

        // Create the environment resolver
        #[cfg(windows)]
        let resolver = {
            use super::environment::get_parent_process_handle;

            EnvironmentResolver::new(env_var_actions)
                .with_interrupt_handle(self.interrupt_event_handle)
                .with_parent_handle(get_parent_process_handle())
        };

        #[cfg(not(windows))]
        let resolver = EnvironmentResolver::new(env_var_actions);

        // Resolve the environment
        let resolved_env = resolver.resolve();

        // Store the resolved environment back in the kernel state
        {
            let mut state = self.state.write().await;
            state.resolved_env = resolved_env.clone();
        }

        Ok(resolved_env)
    }

    /// Build the command to start the kernel.
    pub async fn build_command(
        &self,
        argv: &[String],
        resolved_env: &HashMap<String, String>,
        working_directory: &str,
    ) -> Result<tokio::process::Command, StartupError> {
        // Create the shell command builder
        let shell_builder = ShellCommandBuilder::new(
            self.session_id.clone(),
            self.model.startup_environment.clone(),
            self.model.startup_environment_arg.clone(),
            working_directory.to_string(),
        );

        // Try to build a shell-wrapped command
        let shell_command = shell_builder.build_command(argv, resolved_env)?;

        // If we formed a shell command, use it; otherwise, use the original args
        let mut cmd = match shell_command {
            Some(command) => command,
            None => {
                let mut cmd = tokio::process::Command::new(&argv[0]);
                cmd.args(&argv[1..]);
                cmd
            }
        };

        // Set the working directory if specified and valid
        if !working_directory.is_empty() {
            match fs::metadata(working_directory) {
                Ok(metadata) => {
                    if metadata.is_dir() {
                        cmd.current_dir(working_directory);
                        log::trace!(
                            "[session {}] Set working directory to '{}'",
                            self.session_id,
                            working_directory
                        );
                    } else {
                        log::warn!(
                            "[session {}] Requested working directory '{}' exists but is not a directory; using current directory '{}'",
                            self.session_id,
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
                        self.session_id,
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

        Ok(cmd)
    }

    /// Spawn the kernel process and set up output capture.
    pub async fn spawn_kernel_process(
        &self,
        cmd: &mut tokio::process::Command,
        resolved_env: &HashMap<String, String>,
        process_monitor: &ProcessMonitor,
    ) -> Result<tokio::process::Child, StartupError> {
        // Spawn the process
        let mut child = match cmd
            .envs(resolved_env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to start kernel: {}", e);
                {
                    // Mark kernel as exited since it never started
                    let mut state = self.state.write().await;
                    state
                        .set_status(
                            models::Status::Exited,
                            Some(format!("failed to spawn process: {}", e)),
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

        // Capture output streams
        process_monitor.capture_output_streams(&mut child);

        // Get the process ID
        let pid = child.id();
        {
            let mut state = self.state.write().await;
            state.process_id = pid;
            log::trace!(
                "[session {}]: Session child process started with pid {}",
                self.session_id,
                match pid {
                    Some(pid) => pid.to_string(),
                    None => "<none>".to_string(),
                }
            );
        }

        Ok(child)
    }

    /// Handle kernel info after successful connection.
    pub async fn handle_kernel_info(&self, kernel_info: &serde_json::Value) {
        // Save the kernel info to the state first
        {
            let mut state = self.state.write().await;
            state.set_kernel_info(kernel_info.clone());
        }

        // Try to parse the kernel info to extract prompts
        let parsed = serde_json::from_value::<KernelInfoReply>(kernel_info.clone());
        match parsed {
            Ok(info) => {
                if let Some(language_info) = info.language_info.positron {
                    // Write the input and continuation prompts to the kernel state
                    let mut state = self.state.write().await;
                    if let Some(input_prompt) = &language_info.input_prompt {
                        log::trace!(
                            "[session {}] Got input prompt from kernel info: {}",
                            self.session_id,
                            input_prompt
                        );
                        state.input_prompt = input_prompt.clone();
                    }
                    if let Some(continuation_prompt) = &language_info.continuation_prompt {
                        log::trace!(
                            "[session {}] Got continuation prompt from kernel info: {}",
                            self.session_id,
                            continuation_prompt
                        );
                        state.continuation_prompt = continuation_prompt.clone();
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[session {}] Failed to parse kernel info: {} (content: {}); passing to client anyway",
                    self.session_id,
                    e,
                    serde_json::to_string(kernel_info).unwrap_or_else(|_| "<could not serialize>".to_string())
                );
            }
        }
    }

    /// Process startup results and return appropriate response.
    pub fn process_startup_result(
        &self,
        startup_result: Result<StartupStatus, async_channel::RecvError>,
    ) -> Result<serde_json::Value, StartupError> {
        match startup_result {
            Ok(StartupStatus::Connected(kernel_info)) => {
                log::trace!(
                    "[session {}] Kernel sockets connected successfully, returning from start",
                    self.session_id
                );
                Ok(kernel_info)
            }
            Ok(StartupStatus::ConnectionFailed(output, err)) => {
                // Determine what failed by analyzing the output markers
                let error_context = self.determine_connection_failure_context(&output);

                // Strip markers from output before showing to user
                let clean_output = strip_startup_markers(&output);

                log::error!("[session {}] {}: {}", self.session_id, error_context, err);
                log::error!(
                    "[session {}] Output before failure: \n{}",
                    self.session_id,
                    clean_output
                );

                Err(StartupError {
                    exit_code: Some(130),
                    output: Some(clean_output),
                    error: err.to_json(Some(error_context.to_string())),
                })
            }
            Ok(StartupStatus::AbnormalExit(exit_code, output, err)) => {
                // Determine what failed by analyzing the output markers
                let error_context = self.determine_abnormal_exit_context(&output);

                // Strip markers from output before showing to user
                let clean_output = strip_startup_markers(&output);

                log::error!(
                    "[session {}] {}: exit code {}: {}",
                    self.session_id,
                    error_context,
                    exit_code,
                    err
                );
                log::error!(
                    "[session {}] Output before exit: \n{}",
                    self.session_id,
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
        }
    }

    /// Determine the context for a connection failure based on output.
    fn determine_connection_failure_context(&self, output: &str) -> &'static str {
        match self.model.startup_environment {
            models::StartupEnvironment::Command => {
                if output.contains("KALLICHORE_STARTUP_BEGIN") {
                    if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                        "Kernel failed to connect to ZeroMQ sockets"
                    } else {
                        "Startup command failed to execute"
                    }
                } else {
                    "Startup command failed before execution"
                }
            }
            models::StartupEnvironment::Script => {
                if output.contains("KALLICHORE_STARTUP_BEGIN") {
                    if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                        "Kernel failed to connect to ZeroMQ sockets"
                    } else {
                        "Startup script failed to execute"
                    }
                } else {
                    "Startup script failed before execution"
                }
            }
            _ => "Kernel failed to connect to ZeroMQ sockets",
        }
    }

    /// Determine the context for an abnormal exit based on output.
    fn determine_abnormal_exit_context(&self, output: &str) -> String {
        match self.model.startup_environment {
            models::StartupEnvironment::Command => {
                if output.contains("KALLICHORE_STARTUP_BEGIN") {
                    if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                        String::from("Kernel failed to start")
                    } else {
                        String::from("Startup command failed")
                    }
                } else {
                    String::from("Startup command could not be executed")
                }
            }
            models::StartupEnvironment::Script => {
                if output.contains("KALLICHORE_STARTUP_BEGIN") {
                    if output.contains("KALLICHORE_STARTUP_SUCCESS") {
                        String::from("Kernel failed to start")
                    } else {
                        String::from("Startup script failed")
                    }
                } else {
                    String::from("Startup script could not be executed")
                }
            }
            _ => String::from("Kernel failed to start"),
        }
    }

    /// Write a connection file to disk.
    pub fn write_connection_file(
        &self,
        connection_file: &ConnectionFile,
        file_type: &str,
    ) -> Result<std::path::PathBuf, StartupError> {
        let mut file_name = std::ffi::OsString::from(format!("{}_", file_type));
        file_name.push(&self.session_id);
        file_name.push(".json");
        let file_path = std::env::temp_dir().join(file_name);

        connection_file
            .to_file(file_path.clone())
            .map_err(|e| StartupError {
                exit_code: None,
                output: None,
                error: KSError::ProcessStartFailed(anyhow::anyhow!(
                    "Failed to write {} file: {}",
                    file_type,
                    e
                ))
                .to_json(None),
            })?;

        log::debug!(
            "Wrote {} file for session {} at {:?}",
            file_type,
            self.session_id,
            file_path
        );

        Ok(file_path)
    }
}
