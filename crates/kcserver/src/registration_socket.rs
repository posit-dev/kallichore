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
use log::{info, warn};
use std::collections::HashMap;
use tokio::sync::oneshot;
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
    /// The ZeroMQ Reply socket for the registration socket
    socket: RepSocket,

    /// The port that the `socket` is bound to
    port: u16,

    /// Channel for broadcasting handshake results
    handshake_result_tx: oneshot::Sender<HandshakeResult>,
}

impl RegistrationSocket {
    /// Create a new registration socket
    ///
    /// It is bound to a port immediately on creation. Uses an initial port of `0` to
    /// allow the OS to pick the port, avoiding race conditions.
    pub async fn new(handshake_result_tx: oneshot::Sender<HandshakeResult>) -> Result<Self> {
        let address = "tcp://127.0.0.1:0";

        let mut socket = RepSocket::new();

        // Save the port that the OS binds to
        let port = match socket.bind(address).await? {
            zeromq::Endpoint::Tcp(_, port) => port,
            _ => unreachable!("Address is always TCP"),
        };

        Ok(Self {
            socket,
            port,
            handshake_result_tx,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Handle a handshake request from a kernel
    async fn handle_handshake_request(
        socket: &mut RepSocket,
        handshake_result_tx: oneshot::Sender<HandshakeResult>,
        request_data: ZmqMessage, // Updated type to match expected
        connection: KernelConnection,
    ) {
        info!("Received handshake request from kernel");
        let wire_message = WireMessage::from_zmq(
            "registration".to_string(),
            JupyterChannel::Registration,
            request_data.clone(),
        );
        match wire_message.to_jupyter(JupyterChannel::Registration, connection.hmac_key.clone()) {
            Ok(jupyter_message) => match JupyterMsg::from(jupyter_message.clone()) {
                JupyterMsg::HandshakeRequest(request) => {
                    Self::send_successful_handshake(
                        socket,
                        handshake_result_tx,
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
        handshake_result_tx: oneshot::Sender<HandshakeResult>,
        message: JupyterMessage,
        request: HandshakeRequest,
        connection: KernelConnection,
    ) {
        let result = HandshakeResult {
            request: request.clone(),
            status: HandshakeStatus::Ok,
        };
        if let Err(_) = handshake_result_tx.send(result) {
            warn!("Failed to send handshake result internally");
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
    ///
    /// Consumes `self`, avoiding the possibility of ever starting the same socket twice.
    pub fn listen(self, connection: KernelConnection) {
        // Take ownership of `socket` and `handshake_result_tx` to move into the task
        let mut socket = self.socket;
        let handshake_result_tx = self.handshake_result_tx;

        // Spawn a task to handle incoming registration requests from kernels
        tokio::spawn(async move {
            // Process one message only - in JEP 66, we receive a single handshake and then we can close
            match socket.recv().await {
                Ok(request_data) => {
                    Self::handle_handshake_request(
                        &mut socket,
                        handshake_result_tx,
                        request_data,
                        connection,
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
    }
}
