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

    /// The session model that was used to create this session
    pub model: models::Session,

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

        // Attempt to actually start the kernel process
        let mut child = match tokio::process::Command::new(&self.argv[0])
            .args(&self.argv[1..])
            .current_dir(self.model.working_directory.clone())
            .envs(&self.model.env)
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

        // Get the process ID of the child process
        let pid = child.id();
        {
            // update the status of the session
            let mut state = self.state.write().await;
            state.process_id = pid;
        }

        // Prepare the data needed in the thread that waits for the child process to exit
        let kernel_state = self.state.clone();
        let session_id = self.model.session_id.clone();
        let ws_json_tx = self.ws_json_tx.clone();
        tokio::spawn(async move {
            // Actually run the kernel! This will block until the kernel exits.
            let status = child.wait().await.expect("Failed to wait on child process");

            log::info!(
                "Child process for session {} exited with status: {}",
                session_id,
                status
            );
            {
                // update the status of the session
                let mut state = kernel_state.write().await;
                state.set_status(models::Status::Exited).await;
            }

            let code = status.code().unwrap_or(-1);
            let event = WebsocketMessage::Kernel(KernelMessage::Exited(code));
            ws_json_tx
                .send(event)
                .await
                .expect("Failed to send exit event to client");
        });

        Ok(())
    }
}
