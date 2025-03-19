//
// registration_socket.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

use crate::{
    jupyter_messages::JupyterMsg, kernel_connection::KernelConnection, wire_message::WireMessage,
};
use anyhow::Result;
use kcshared::jupyter_message::JupyterChannel;
use kcshared::{
    handshake_protocol::{HandshakeReply, HandshakeRequest, HandshakeStatus},
    jupyter_message::{JupyterMessage, JupyterMessageHeader},
};
use log::{debug, info, warn};
use std::collections::HashMap;
use tokio::sync::broadcast;
use zeromq::{RepSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

/// Return value from a handshake negotiation
#[derive(Debug, Clone)]
pub struct HandshakeResult {
    /// The received handshake request from the kernel
    pub request: HandshakeRequest,
    /// The status of the handshake
    pub status: HandshakeStatus,
}

/// Manages a registration socket for JEP 66 handshaking protocol
pub struct RegistrationSocket {
    /// The port on which the registration socket is listening
    pub port: u16,

    /// The ZeroMQ Reply socket for the registration socket
    socket: Option<RepSocket>,

    /// Channel for broadcasting handshake results
    result_tx: broadcast::Sender<HandshakeResult>,

    /// Flag indicating if the socket is running
    running: bool,

    /// The connection information for this socket
    connection: KernelConnection,
}

impl RegistrationSocket {
    /// Create a new registration socket with a required port and connection information
    pub fn new(port: u16, connection: KernelConnection) -> Self {
        let (result_tx, _) = broadcast::channel(32);
        Self {
            port,
            socket: None,
            result_tx,
            running: false,
            connection,
        }
    }

    /// Handle a handshake request from a kernel
    async fn handle_handshake_request(
        socket: &mut RepSocket,
        result_tx: &broadcast::Sender<HandshakeResult>,
        request_data: ZmqMessage, // Updated type to match expected
        connection: &KernelConnection,
    ) {
        info!("Received handshake request from kernel");
        let wire_message = WireMessage::from_zmq(
            "registration".to_string(),
            JupyterChannel::Registration,
            request_data.clone(),
        );
        match wire_message.to_jupyter(JupyterChannel::Registration) {
            Ok(jupyter_message) => match JupyterMsg::from(jupyter_message.clone()) {
                JupyterMsg::HandshakeRequest(request) => {
                    Self::send_successful_handshake(
                        socket,
                        result_tx,
                        jupyter_message,
                        request,
                        connection,
                    )
                    .await;
                }
                _ => {
                    warn!(
                        "Received non-handshake request from kernel: {:?}",
                        jupyter_message
                    );
                }
            },
            Err(e) => {
                warn!("Failed to parse JupyterMessage: {}", e);
                let reply = HandshakeReply {
                    status: HandshakeStatus::Error,
                    error: Some(format!("Failed to parse JupyterMessage: {}", e)),
                    capabilities: HashMap::new(),
                };
                let reply_data =
                    serde_json::to_vec(&reply).expect("Failed to serialize handshake reply");
                if let Err(e) = socket.send(reply_data.into()).await {
                    warn!("Failed to send handshake reply to kernel: {}", e);
                }
            }
        }
    }

    async fn send_successful_handshake(
        socket: &mut RepSocket,
        result_tx: &broadcast::Sender<HandshakeResult>,
        message: JupyterMessage,
        request: HandshakeRequest,
        connection: &KernelConnection,
    ) {
        let result = HandshakeResult {
            request: request.clone(),
            status: HandshakeStatus::Ok,
        };
        if let Err(e) = result_tx.send(result) {
            warn!("Failed to send handshake result internally: {}", e);
        }

        // Create a successful handshake reply
        let reply = HandshakeReply {
            status: HandshakeStatus::Ok,
            error: None,
            capabilities: HashMap::new(),
        };

        // Create a Jupyter message containing the handshake reply
        // Generate a message id
        let jupyter_msg = JupyterMessage {
            header: JupyterMessageHeader {
                msg_type: "handshake_reply".to_string(),
                msg_id: uuid::Uuid::new_v4().to_string(),
            },
            parent_header: Some(message.header),
            channel: JupyterChannel::Registration,
            content: serde_json::to_value(reply).unwrap(),
            metadata: serde_json::Value::Null,
            buffers: vec![],
        };

        // Convert to a wire message for sending
        let wire_message = WireMessage::from_jupyter(jupyter_msg, connection.clone());
        match wire_message {
            Ok(wire_message) => {
                info!(
                    "[session {}] Sending successful handshake reply to kernel",
                    connection.session_id
                );
                // Convert wire message to ZMQ format for sending
                if let Err(e) = socket.send(wire_message.into()).await {
                    warn!(
                        "[session {}] Failed to send handshake reply to kernel: {}",
                        connection.session_id, e
                    );
                }
            }
            Err(e) => {
                warn!(
                    "[session {}] Failed to create wire message for handshake reply: {}",
                    connection.session_id, e
                );
            }
        }
    }

    /// Start listening on the registration socket
    pub async fn start(&mut self) -> Result<()> {
        if self.running {
            return Ok(());
        }

        // Create and bind the ZeroMQ REP socket
        let mut socket = RepSocket::new();
        let address = format!("tcp://127.0.0.1:{}", self.port);
        socket.bind(&address).await?;

        // Store the socket
        self.socket = Some(socket);

        // Set the running flag
        self.running = true;

        // Create a new broadcast channel for receiving handshake results
        let (result_tx, _) = broadcast::channel(32);

        // Update our broadcast sender
        self.result_tx = result_tx.clone();

        // Take ownership of the socket to move into the task
        let socket = self.socket.take().unwrap();
        let connection = self.connection.clone();

        debug!(
            "[session {}] Started JEP 66 registration REP socket on port {}",
            connection.session_id, self.port
        );

        // Spawn a task to handle incoming registration requests from kernels
        // Remove unused variable
        let connection_clone = connection.clone();
        tokio::spawn(async move {
            let mut socket = socket;

            // Process one message only - in JEP 66, we receive a single handshake and then we can close
            match socket.recv().await {
                Ok(request_data) => {
                    Self::handle_handshake_request(
                        &mut socket,
                        &result_tx,
                        request_data,
                        &connection_clone,
                    )
                    .await;
                }
                Err(e) => {
                    warn!(
                        "[session {}] Error receiving handshake request from kernel: {}",
                        connection.session_id, e
                    );
                }
            }

            // Close the socket after handling the handshake (or on error)
            socket.close().await;
        });

        Ok(())
    }

    /// Stop the registration socket
    pub async fn stop(&mut self) {
        self.running = false;
        if let Some(socket) = self.socket.take() {
            socket.close().await;
        }
    }

    /// Get a receiver for handshake results
    pub fn get_result_receiver(&self) -> broadcast::Receiver<HandshakeResult> {
        // Create a new receiver from our broadcast channel
        self.result_tx.subscribe()
    }
}
