//
// registration_socket.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

use anyhow::Result;
use kcshared::handshake_protocol::{
    HandshakeReply, HandshakeRequest, HandshakeStatus, HandshakeVersion,
};
use log::{debug, info, warn};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use zeromq::{RepSocket, Socket, SocketRecv, SocketSend};

use crate::{jupyter_messages::JupyterMsg, wire_message::WireMessage};
use futures::StreamExt;
use kcshared::jupyter_message::JupyterChannel;

/// Return value from a handshake negotiation
#[derive(Debug, Clone)]
pub struct HandshakeResult {
    /// The received handshake request from the kernel
    pub request: HandshakeRequest,

    /// The status of the handshake
    pub status: HandshakeStatus,

    /// Any capabilities the kernel requested
    #[allow(dead_code)]
    pub capabilities: HashMap<String, serde_json::Value>,
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
    running: Arc<RwLock<bool>>,
}

impl RegistrationSocket {
    /// Create a new registration socket with a required port
    pub fn new(port: u16) -> Self {
        let (result_tx, _) = broadcast::channel(32);

        Self {
            port,
            socket: None,
            result_tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Start listening on the registration socket
    pub async fn start(&mut self) -> Result<()> {
        // Create and bind the ZeroMQ REP socket
        let mut socket = RepSocket::new();
        let address = format!("tcp://127.0.0.1:{}", self.port);
        socket.bind(&address).await?;

        // Store the socket
        self.socket = Some(socket);

        // Set the running flag
        *self.running.write().await = true;

        // Create a new broadcast channel for receiving handshake results
        let (result_tx, _) = broadcast::channel(32);

        // Update our broadcast sender
        self.result_tx = result_tx.clone();

        // Take ownership of the socket to move into the task
        let socket = self.socket.take().unwrap();
        let running = self.running.clone();

        info!(
            "Started JEP 66 registration REP socket on port {}",
            self.port
        );

        // Spawn a task to handle incoming registration requests from kernels
        tokio::spawn(async move {
            let mut socket = socket;
            info!("Waiting for handshake requests from kernels");
            let mut monitor = socket.monitor();
            // Wait for the socket to change states
            match monitor.next().await {
                Some(event) => {
                    info!("Socket event: {:?}", event);
                }
                None => {
                    warn!("Socket monitor stream ended");
                }
            };
            while *running.read().await {
                info!("Waiting socket.recv().await");
                // Wait for a request from a kernel
                match socket.recv().await {
                    Ok(request_data) => {
                        info!("Received handshake request from kernel");
                        // Convert raw message data to a Jupyter message
                        let wire_message = WireMessage::from_zmq(
                            "session_id".to_string(),
                            JupyterChannel::Registration,
                            request_data.clone(),
                        );
                        match wire_message.to_jupyter(JupyterChannel::Registration) {
                            Ok(jupyter_message) => {
                                match JupyterMsg::from(jupyter_message) {
                                    JupyterMsg::HandshakeRequest(request) => {
                                        debug!(
                                            "Received handshake request from kernel with protocol version {}",
                                            request.protocol_version
                                        );

                                        // Check if the request is from a kernel that supports JEP 66
                                        if HandshakeVersion::supports_handshaking(
                                            &request.protocol_version,
                                        ) {
                                            // Create the handshake result for internal use
                                            let result = HandshakeResult {
                                                request: request.clone(),
                                                status: HandshakeStatus::Ok,
                                                capabilities: request.capabilities.clone(),
                                            };

                                            // Send the result internally for processing using broadcast
                                            if let Err(e) = result_tx.send(result) {
                                                warn!("Failed to send handshake result internally: {}", e);
                                            }

                                            // Create a successful reply to send back to the kernel
                                            let reply = HandshakeReply {
                                                status: HandshakeStatus::Ok,
                                                error: None,
                                                capabilities: HashMap::new(),
                                            };

                                            // Serialize the reply
                                            let reply_data = serde_json::to_vec(&reply)
                                                .expect("Failed to serialize handshake reply");

                                            // Send the reply to the kernel
                                            if let Err(e) = socket.send(reply_data.into()).await {
                                                warn!(
                                                    "Failed to send handshake reply to kernel: {}",
                                                    e
                                                );
                                            } else {
                                                info!("Sent successful handshake reply to kernel");
                                            }
                                        } else {
                                            // Create the handshake result for internal use
                                            let result = HandshakeResult {
                                                request: request.clone(),
                                                status: HandshakeStatus::Error,
                                                capabilities: request.capabilities.clone(),
                                            };

                                            // Send the result internally for processing using broadcast
                                            if let Err(e) = result_tx.send(result) {
                                                warn!("Failed to send handshake result internally: {}", e);
                                            }

                                            // Create an error reply to send back to the kernel
                                            let reply = HandshakeReply {
                                                status: HandshakeStatus::Error,
                                                error: Some(format!(
                                                    "Kernel protocol version {} does not support JEP 66 handshaking (>= 5.5 required)",
                                                    request.protocol_version
                                                )),
                                                capabilities: HashMap::new(),
                                            };

                                            // Serialize the reply
                                            let reply_data = serde_json::to_vec(&reply)
                                                .expect("Failed to serialize handshake reply");

                                            // Send the reply to the kernel
                                            if let Err(e) = socket.send(reply_data.into()).await {
                                                warn!(
                                                    "Failed to send handshake reply to kernel: {}",
                                                    e
                                                );
                                            } else {
                                                warn!(
                                                    "Sent error handshake reply to kernel with unsupported protocol version {}",
                                                    request.protocol_version
                                                );
                                            }
                                        }
                                    }
                                    _ => {
                                        // Exceedingly unlikely, but log a warning if we receive a non-handshake request
                                        let data_vec = request_data.into_vec();
                                        let flat_bytes: Vec<u8> =
                                            data_vec.iter().flat_map(|b| b.to_vec()).collect();
                                        let request_str = String::from_utf8_lossy(&flat_bytes);
                                        warn!(
                                            "Received non-handshake request from kernel: {}",
                                            request_str
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse JupyterMessage: {}", e);
                                // Handle the error (e.g., send an error reply)
                                let reply = HandshakeReply {
                                    status: HandshakeStatus::Error,
                                    error: Some(format!("Failed to parse JupyterMessage: {}", e)),
                                    capabilities: HashMap::new(),
                                };

                                let reply_data = serde_json::to_vec(&reply)
                                    .expect("Failed to serialize handshake reply");

                                if let Err(e) = socket.send(reply_data.into()).await {
                                    warn!("Failed to send handshake reply to kernel: {}", e);
                                }
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Error receiving handshake request from kernel: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop the registration socket
    pub async fn stop(&mut self) {
        *self.running.write().await = false;

        if let Some(socket) = self.socket.take() {
            socket.close().await;
        }
    }

    /// Get the sender for handshake results
    pub fn get_result_sender(&self) -> broadcast::Sender<HandshakeResult> {
        self.result_tx.clone()
    }

    /// Get a receiver for handshake results
    pub fn get_result_receiver(&self) -> broadcast::Receiver<HandshakeResult> {
        // Create a new receiver from our broadcast channel
        self.result_tx.subscribe()
    }
}
