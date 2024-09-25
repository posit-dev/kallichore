//
// kernel_session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use std::{process::Stdio, sync::Arc};

use async_channel::{Receiver, Sender};
use chrono::{DateTime, Utc};
use kallichore_api::models;
use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    kernel_message::{KernelMessage, OutputStream},
    websocket_message::WebsocketMessage,
};
use rand::Rng;
use std::iter;
use sysinfo::{Pid, Signal, System};
use tokio::io::{AsyncBufReadExt, AsyncRead};
use tokio::sync::RwLock;

use crate::{connection_file, kernel_connection::KernelConnection, kernel_state::KernelState};

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
}

impl KernelSession {
    /// Create a new kernel session.
    pub fn new(
        session: models::NewSession,
        connection_file: connection_file::ConnectionFile,
    ) -> Result<Self, anyhow::Error> {
        let (zmq_tx, zmq_rx) = async_channel::unbounded::<JupyterMessage>();
        let (json_tx, json_rx) = async_channel::unbounded::<WebsocketMessage>();
        let kernel_state = Arc::new(RwLock::new(KernelState::new(
            session.working_directory.clone(),
            json_tx.clone(),
        )));
        let connection = KernelConnection::from_session(&session, connection_file.key.clone())?;
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
        };
        Ok(kernel_session)
    }

    pub async fn start(&self) -> Result<(), anyhow::Error> {
        // Mark the kernel as starting
        {
            let mut state = self.state.write().await;
            state.set_status(models::Status::Starting).await;
        }

        log::debug!(
            "Starting kernel for session {}: {:?}",
            self.model.session_id,
            self.argv
        );

        // Attempt to actually start the kernel process
        let mut child = match tokio::process::Command::new(&self.argv[0])
            .args(&self.argv[1..])
            .current_dir(self.model.working_directory.clone())
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
                    state.set_status(models::Status::Exited).await;
                }
                return Err(e.into());
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
        }

        // Prepare the data needed in the thread that waits for the child process to exit
        let kernel = self.clone();
        tokio::spawn(async move {
            kernel.run_child(child).await;
            ()
        });

        Ok(())
    }

    async fn run_child(&self, mut child: tokio::process::Child) {
        // Actually run the kernel! This will block until the kernel exits.
        let status = child.wait().await.expect("Failed to wait on child process");

        log::info!(
            "Child process for session {} exited with status: {}",
            self.connection.session_id,
            status
        );
        {
            // update the status of the session
            let mut state = self.state.write().await;
            state.set_status(models::Status::Exited).await;
        }

        let code = status.code().unwrap_or(-1);
        let event = WebsocketMessage::Kernel(KernelMessage::Exited(code));
        self.ws_json_tx
            .send(event)
            .await
            .expect("Failed to send exit event to client");
    }

    pub async fn restart(&self) -> Result<(), anyhow::Error> {
        // Enter the restarting state.
        {
            let mut state = self.state.write().await;
            if state.restarting {
                return Err(anyhow::anyhow!("Kernel is already restarting"));
            }
            state.restarting = true;
        }

        // Make and send the shutdown request.
        let msg = JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: self.make_message_id(),
                msg_type: "shutdown_request".to_string(),
            },
            parent_header: None,
            metadata: serde_json::json!({}),
            content: serde_json::json!({
                "restart": true, // Restart the kernel
            }),
            channel: JupyterChannel::Control,
            buffers: vec![],
        };

        match self.ws_zmq_tx.send(msg).await {
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
                return Err(anyhow::anyhow!(
                    "Failed to send shutdown request to kernel: {}",
                    e
                ));
            }
        }

        Ok(())
    }

    /// Format this session as an active session.
    pub async fn as_active_session(&self) -> models::ActiveSession {
        let state = self.state.read().await;
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
                        msg_id: self.make_message_id(),
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

    fn make_message_id(&self) -> String {
        let mut rng = rand::thread_rng();
        iter::repeat_with(|| format!("{:x}", rng.gen_range(0..16)))
            .take(10)
            .collect()
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
}
