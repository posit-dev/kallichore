//
// kernel_state.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use async_channel::Sender;
use kallichore_api::models;
use kcshared::kernel_message::StatusUpdate;

use crate::execution_queue::ExecutionQueue;

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

    /// A channel to publish status updates
    ws_status_tx: Sender<StatusUpdate>,
}

impl KernelState {
    /// Create a new kernel state.
    pub fn new(
        session: models::NewSession,
        working_directory: String,
        ws_status_tx: Sender<StatusUpdate>,
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
            ws_status_tx,
            idle_since: Some(std::time::Instant::now()),
            busy_since: None,
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
        self.ws_status_tx.send(update).await.unwrap();
    }
}
