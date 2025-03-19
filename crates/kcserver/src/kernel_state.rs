//
// kernel_state.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

use async_channel::Sender;
use kallichore_api::models;
use kcshared::{
    kernel_message::{KernelMessage, StatusUpdate},
    websocket_message::WebsocketMessage,
};

use crate::connection_file::ConnectionFile;
use crate::execution_queue::ExecutionQueue;

#[cfg(not(target_os = "windows"))]
use crate::working_dir::get_process_cwd;

/// The mutable state of the kernel.
///
/// Does not implement the Clone trait; only one instance of the kernel state
/// should exist at a time.
#[derive(Debug)]
pub struct KernelState {
    /// The session ID for this kernel instance.
    pub session_id: String,

    /// The kernel's current status.
    pub status: models::Status,

    /// Whether the kernel is connected to a client.
    pub connected: bool,

    /// Whether the kernel is currently restarting. This flag triggers an automatic
    /// startup after the kernel exits for restart.
    pub restarting: bool,

    /// The current working directory of the kernel.
    pub working_directory: String,

    /// The current process ID of the kernel, or None if the kernel is not running.
    pub process_id: Option<u32>,

    /// The execution queue for the kernel.
    pub execution_queue: ExecutionQueue,

    /// The current input prompt.
    pub input_prompt: String,

    /// The current continuation prompt.
    pub continuation_prompt: String,

    /// The time at which the kernel last became idle.
    pub idle_since: Option<std::time::Instant>,

    /// The time at which the kernel last became busy.
    pub busy_since: Option<std::time::Instant>,

    /// A channel on which to send idle nudges
    pub idle_nudge_tx: tokio::sync::mpsc::Sender<()>,

    /// A channel to publish status updates to the websocket
    ws_json_tx: Sender<WebsocketMessage>,

    /// The connection file for the kernel, or None if not set.
    pub connection_file: Option<ConnectionFile>,
}

impl KernelState {
    /// Create a new kernel state.
    pub fn new(
        session: models::NewSession,
        working_directory: String,
        idle_nudge_tx: tokio::sync::mpsc::Sender<()>,
        ws_json_tx: Sender<WebsocketMessage>,
    ) -> Self {
        KernelState {
            session_id: session.session_id.clone(),
            status: models::Status::Idle,
            working_directory,
            connected: false,
            restarting: false,
            process_id: None,
            execution_queue: ExecutionQueue::new(),
            input_prompt: session.input_prompt.clone(),
            continuation_prompt: session.continuation_prompt.clone(),
            ws_json_tx,
            idle_nudge_tx,
            idle_since: Some(std::time::Instant::now()),
            busy_since: None,
            connection_file: None,
        }
    }

    async fn nudge_idle(&mut self) {
        if let Err(err) = self.idle_nudge_tx.send(()).await {
            log::error!(
                "[session {}] Failed to send idle nudge: {}",
                self.session_id,
                err
            );
        }
    }

    pub async fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
        self.nudge_idle().await;
    }

    /// Polls the working directory to see if it's changed.
    ///
    /// If it has, updates state and sends a message to the client. Only supported
    /// on non-Windows platforms.
    #[cfg(not(target_os = "windows"))]
    pub async fn poll_working_dir(&mut self) {
        if self.process_id.is_none() {
            return;
        }

        let working_dir = match get_process_cwd(self.process_id.unwrap()) {
            Ok(dir) => dir,
            Err(err) => {
                log::error!(
                    "[session {}] Failed to get working directory: {}",
                    self.session_id,
                    err
                );
                return;
            }
        };

        let working_dir = working_dir.to_string_lossy().to_string();

        if working_dir != self.working_directory {
            log::debug!(
                "[session {}] Working directory changed: '{}' => '{}'",
                self.session_id,
                self.working_directory,
                working_dir
            );
            let msg =
                WebsocketMessage::Kernel(KernelMessage::WorkingDirChanged(working_dir.clone()));
            self.ws_json_tx.send(msg).await.unwrap();
            self.working_directory = working_dir;
        }
    }

    /// Set the kernel's status.
    pub async fn set_status(&mut self, status: models::Status, reason: Option<String>) {
        log::debug!(
            "[session {}] status '{}' => '{}' {}",
            self.session_id,
            self.status,
            status,
            match reason {
                Some(ref r) => format!("({})", r),
                None => "".to_string(),
            }
        );

        // If the status didn't change, don't perform any side effects or notify the client.
        if self.status == status {
            return;
        }

        self.status = status;

        // When exiting ...
        if status == models::Status::Exited {
            // ... clear the execution queue
            self.execution_queue.clear();
            // ... clear the process ID (no longer running)
            self.process_id = None;
        }

        // When idle, record the time at which the kernel became idle
        if status == models::Status::Idle
            || status == models::Status::Ready
            || status == models::Status::Exited
        {
            self.idle_since = Some(std::time::Instant::now());
            self.nudge_idle().await;
        } else {
            self.idle_since = None;
        }

        // When busy, record the time at which the kernel became busy
        if status == models::Status::Busy {
            self.busy_since = Some(std::time::Instant::now());
        } else {
            self.busy_since = None;
        }

        // Publish the new status to the status stream (for internal use)
        let update = StatusUpdate {
            status: self.status.clone(),
            reason,
        };

        let status_message = WebsocketMessage::Kernel(KernelMessage::Status(update.clone()));
        self.ws_json_tx.send(status_message).await.unwrap();

        // When the kernel becomes idle after executing code, poll the working
        // directory to see if it's changed.
        #[cfg(not(target_os = "windows"))]
        if status == models::Status::Idle {
            if let Some(reason) = update.reason {
                if reason == "execute_request" {
                    self.poll_working_dir().await;
                }
            }
        }
    }
}
