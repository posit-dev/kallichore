//
// kernel_state.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use async_channel::Sender;
use kallichore_api::models;
use kcshared::{kernel_message::KernelMessage, websocket_message::WebsocketMessage};

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

    /// The current working directory of the kernel.
    pub working_directory: String,

    /// The current process ID of the kernel, or None if the kernel is not running.
    pub process_id: Option<u32>,

    /// A channel to send JSON messages to the WebSocket, for automatically
    /// publishing kernel status updates.
    ws_json_tx: Sender<String>,
}

impl KernelState {
    /// Create a new kernel state.
    pub fn new(working_directory: String, ws_json_tx: Sender<String>) -> Self {
        KernelState {
            status: models::Status::Idle,
            working_directory,
            connected: false,
            process_id: None,
            ws_json_tx,
        }
    }

    /// Set the kernel's status.
    pub async fn set_status(&mut self, status: models::Status) {
        self.status = status;

        // Publish the new status to the WebSocket
        let status_message = WebsocketMessage::Kernel(KernelMessage::Status(status.clone()));
        self.ws_json_tx
            .send(serde_json::to_string(&status_message).unwrap())
            .await
            .unwrap();
    }
}
