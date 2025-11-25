//
// lifecycle.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Kernel lifecycle management (restart, shutdown, interrupt).

use async_channel::{SendError, Sender};
use event_listener::Event;
use kallichore_api::models::{self, StartupError, VarAction};
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use std::{fs, sync::Arc};
use tokio::sync::RwLock;

use crate::{error::KSError, kernel_state::KernelState, working_dir::expand_path};

use super::utils::make_message_id;

/// Manages kernel lifecycle operations.
pub struct LifecycleManager {
    /// Session ID for logging
    session_id: String,

    /// Shared kernel state
    state: Arc<RwLock<KernelState>>,

    /// Channel to send ZMQ messages to the kernel
    ws_zmq_tx: Sender<JupyterMessage>,

    /// Event that fires when the process exits
    exit_event: Arc<Event>,

    /// Original working directory from the model
    #[allow(dead_code)]
    original_working_directory: String,
}

impl LifecycleManager {
    /// Create a new lifecycle manager.
    pub fn new(
        session_id: String,
        state: Arc<RwLock<KernelState>>,
        ws_zmq_tx: Sender<JupyterMessage>,
        exit_event: Arc<Event>,
        original_working_directory: String,
    ) -> Self {
        Self {
            session_id,
            state,
            ws_zmq_tx,
            exit_event,
            original_working_directory,
        }
    }

    /// Shutdown the kernel.
    pub async fn shutdown(&self) -> Result<(), anyhow::Error> {
        self.shutdown_request(false).await?;
        Ok(())
    }

    /// Send a shutdown request to the kernel.
    async fn shutdown_request(&self, restart: bool) -> Result<(), SendError<JupyterMessage>> {
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
                        self.session_id,
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
                                self.session_id,
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
                            self.session_id,
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
                        self.session_id,
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
                            self.session_id,
                            state.working_directory
                        );
                    }

                    #[cfg(target_os = "windows")]
                    {
                        state.working_directory = self.original_working_directory.clone();
                        log::debug!(
                            "[session {}] Will restart in working directory '{}' (original)",
                            self.session_id,
                            state.working_directory
                        );
                    }
                }
            }

            // Set the environment variables, if supplied
            if let Some(env) = env {
                log::debug!(
                    "[session {}] Will restart with environment variables: {:?}",
                    self.session_id,
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

        // Wait for the kernel to exit and then restart it
        log::debug!(
            "[session {}] Waiting for kernel to exit before restarting",
            self.session_id
        );
        Ok(())
    }

    /// Complete restart by waiting for the kernel to exit and then starting it again.
    ///
    /// This should be called after `restart()` sends the shutdown request.
    /// It returns when the kernel has exited and can be started again.
    pub async fn wait_for_exit_and_signal(&self) -> bool {
        // Wait for the kernel to exit
        let listener = self.exit_event.listen();
        listener.await;

        // Check if the kernel is still restarting, and clear the restarting flag
        let mut state = self.state.write().await;
        if !state.restarting {
            log::debug!(
                "[session {}] Kernel is no longer restarting; stopping restart",
                self.session_id
            );
            return false;
        }
        state.restarting = false;
        true
    }

    /// Interrupt the kernel.
    ///
    /// Sends an interrupt signal to the kernel, either via a system signal
    /// (Unix) or an event handle (Windows), or via a Jupyter message.
    pub async fn interrupt(
        &self,
        interrupt_mode: models::InterruptMode,
        #[cfg(windows)] interrupt_event_handle: Option<isize>,
    ) -> Result<(), anyhow::Error> {
        match interrupt_mode {
            models::InterruptMode::Signal => {
                // On Windows, interrupts are signaled by setting a kernel event.
                #[cfg(windows)]
                {
                    use windows::Win32::Foundation::HANDLE;
                    use windows::Win32::System::Threading::SetEvent;

                    if let Some(handle_value) = interrupt_event_handle {
                        #[allow(unsafe_code)]
                        unsafe {
                            log::debug!(
                                "Setting interrupt event {} for session {}",
                                handle_value,
                                self.session_id
                            );
                            // Convert the stored isize back to a HANDLE
                            let handle = HANDLE(handle_value as *mut std::ffi::c_void);
                            return match SetEvent(handle) {
                                Ok(_) => {
                                    log::debug!(
                                        "Successfully set interrupt event for session {}",
                                        self.session_id
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

                // On Unix-alikes, interrupts are signaled by sending a SIGINT signal
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
}
