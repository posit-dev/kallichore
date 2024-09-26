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
    /// The kernel's current status.
    pub status: models::Status,

    /// Whether the kernel is connected to a client.
    pub connected: bool,

    /// Whether the kernel is currently restarting.
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
    pub fn new(working_directory: String, ws_status_tx: Sender<models::Status>) -> Self {
        KernelState {
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

        // Publish the new status to the WebSocket (for the client)
        /*
        let status_message = WebsocketMessage::Kernel(KernelMessage::Status(status.clone()));
        self.ws_json_tx.send(status_message).await.unwrap();
        */
    }
}
