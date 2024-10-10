//
// kernel_state.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use async_channel::Sender;
use kallichore_api::models;

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

    /// A channel to publish status updates
    ws_status_tx: Sender<models::Status>,
}

impl KernelState {
    /// Create a new kernel state.
    pub fn new(
        session_id: String,
        working_directory: String,
        ws_status_tx: Sender<models::Status>,
    ) -> Self {
        KernelState {
            session_id,
            status: models::Status::Idle,
            working_directory,
            connected: false,
            restarting: false,
            process_id: None,
            execution_queue: ExecutionQueue::new(),
            ws_status_tx,
        }
    }

    /// Set the kernel's status.
    pub async fn set_status(&mut self, status: models::Status) {
        log::debug!(
            "[session {}] status '{}' => '{}'",
            self.session_id,
            self.status,
            status
        );
        self.status = status;

        // When exiting ...
        if status == models::Status::Exited {
            // ... clear the execution queue
            self.execution_queue.clear();
            // ... clear the process ID (no longer running)
            self.process_id = None;
        }

        // Publish the new status to the status stream (for internal use)
        self.ws_status_tx.send(status.clone()).await.unwrap();
    }
}
