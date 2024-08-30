//
// kernel_session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use std::sync::Arc;

use async_channel::{Receiver, Sender};
use chrono::{DateTime, Utc};
use kallichore_api::models;
use kcshared::{kernel_message::KernelMessage, websocket_message::WebsocketMessage};
use tokio::sync::RwLock;

use crate::{
    connection_file, kernel_connection::KernelConnection, kernel_state::KernelState,
    wire_message::ZmqChannelMessage,
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

    /// The command line arguments used to start the kernel. The first is the
    /// path to the kernel itself.
    pub argv: Vec<String>,

    /// The current state of the kernel
    pub state: Arc<RwLock<KernelState>>,

    /// The date and time the kernel was started
    pub started: DateTime<Utc>,

    /// The channel to send JSON messages to the WebSocket
    pub ws_json_tx: Sender<String>,

    /// The channel to receive JSON messages from the WebSocket
    pub ws_json_rx: Receiver<String>,

    /// The channel to send ZMQ messages to the kernel
    pub ws_zmq_tx: Sender<ZmqChannelMessage>,

    /// The channel to receive ZMQ messages from the kernel
    pub ws_zmq_rx: Receiver<ZmqChannelMessage>,
}

impl KernelSession {
    /// Create a new kernel session.
    pub fn new(
        session: models::Session,
        connection_file: connection_file::ConnectionFile,
    ) -> Result<Self, anyhow::Error> {
        let (zmq_tx, zmq_rx) = async_channel::unbounded::<ZmqChannelMessage>();
        let (json_tx, json_rx) = async_channel::unbounded::<String>();
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
            ws_json_rx: json_rx,
            ws_zmq_tx: zmq_tx,
            ws_zmq_rx: zmq_rx,
            connection,
            started,
        };

        // Start the kernel in a separate task
        let new_session = kernel_session.clone();
        tokio::spawn(async move {
            new_session.start(session).await;
        });
        Ok(kernel_session)
    }

    async fn start(&self, session: models::Session) {
        let mut child = tokio::process::Command::new(&self.argv[0])
            .args(&self.argv[1..])
            .current_dir(session.working_directory.clone())
            .envs(&session.env)
            .spawn()
            .expect("Failed to start child process");

        // Get the process ID of the child process
        let pid = child.id();
        {
            // update the status of the session
            let mut state = self.state.write().await;
            state.process_id = pid;
            state.set_status(models::Status::Starting).await;
        }

        // Actually run the kernel! This will block until the kernel exits.
        let status = child.wait().await.expect("Failed to wait on child process");

        log::info!(
            "Child process for session {} exited with status: {}",
            session.session_id,
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
            .send(serde_json::to_string(&event).unwrap())
            .await
            .expect("Failed to send exit event to client");
    }
}
