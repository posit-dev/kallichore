//
// registration_socket.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

use anyhow::Result;
use kcshared::{
    handshake_protocol::{HandshakeReply, HandshakeRequest, HandshakeStatus},
    jupyter_message::{JupyterMessage, JupyterMessageHeader},
};
use log::{debug, info, warn};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use zeromq::{RepSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

use crate::{
    jupyter_messages::JupyterMsg, kernel_connection::KernelConnection, wire_message::WireMessage,
};
use kcshared::jupyter_message::JupyterChannel;

// Global registry to track sessions waiting for handshakes.
//
// This registry is needed because we only have one registration socket that can
// receive handshakes from multiple kernels, the handshake request bears no
// identifying information about the originating session (its Jupyter header
// contains a randomly generated session ID), and we need to match the handshake
// to the correct kernel session.
//
// It would be preferable to have a more sophisticated system that can match
// handshakes to the correct session based on some identifying information in
// the handshake request (maybe the peer ID?). Failing that, we could start a
// just-in-time registration socket per session, to eliminate the chance of
// mismatching handshakes.
lazy_static::lazy_static! {
    static ref HANDSHAKE_REGISTRY: Arc<RwLock<HashMap<String, KernelConnection>>> = Arc::new(RwLock::new(HashMap::new()));

    // Global port for the registration socket. Set when the server starts.
    pub static ref REGISTRATION_PORT: Arc<RwLock<Option<u16>>> = Arc::new(RwLock::new(None));
}

/// Return value from a handshake negotiation
#[derive(Debug, Clone)]
pub struct HandshakeResult {
    /// The received handshake request from the kernel
    pub request: HandshakeRequest,

    /// The status of the handshake
    pub status: HandshakeStatus,
}

/// Registers a kernel session that is waiting for a handshake
pub async fn register_session_for_handshake(connection: KernelConnection) {
    let session_id = connection.session_id.clone();
    let mut registry = HANDSHAKE_REGISTRY.write().await;
    registry.insert(session_id.clone(), connection);
    info!("Registered session {} for handshake", session_id);
}

/// Removes a kernel session from the handshake registry
pub async fn unregister_session_for_handshake(session_id: &str) {
    let mut registry = HANDSHAKE_REGISTRY.write().await;
    registry.remove(session_id);
    info!(
        "Unregistered session {} from handshake registry",
        session_id
    );
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

        // Store the port in the global REGISTRATION_PORT
        tokio::spawn(async move {
            let mut port_lock = REGISTRATION_PORT.write().await;
            *port_lock = Some(port);
        });

        Self {
            port,
            socket: None,
            result_tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Handle a handshake request from a kernel
    async fn handle_handshake_request(
        socket: &mut RepSocket,
        result_tx: &broadcast::Sender<HandshakeResult>,
        request_data: ZmqMessage, // Updated type to match expected
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
                    Self::send_successful_handshake(socket, result_tx, jupyter_message, request)
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

        // Get the first waiting session from registry
        let connection = {
            let registry = HANDSHAKE_REGISTRY.read().await;
            // Use the first session in the registry as they're all waiting for handshakes
            // This is a simplification - ideally we would match the handshake to the specific session
            if registry.is_empty() {
                warn!("No sessions registered for handshake, creating an empty connection");
                // Fallback to an empty connection if no sessions are registered
                KernelConnection {
                    session_id: String::new(),
                    username: String::new(),
                    key: None,
                    hmac_key: None,
                }
            } else {
                // Clone the first connection we find
                registry.values().next().unwrap().clone()
            }
        };

        // Convert to a wire message for sending
        let wire_message = WireMessage::from_jupyter(jupyter_msg, connection, None);
        match wire_message {
            Ok(wire_message) => {
                info!("Sending successful handshake reply to kernel");
                // Convert wire message to ZMQ format for sending
                if let Err(e) = socket.send(wire_message.into()).await {
                    warn!("Failed to send handshake reply to kernel: {}", e);
                } else {
                    info!("Sent successful handshake reply to kernel");
                }
            }
            Err(e) => {
                warn!("Failed to create wire message for handshake reply: {}", e);
            }
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

        debug!(
            "Started JEP 66 registration REP socket on port {}",
            self.port
        );

        // Spawn a task to handle incoming registration requests from kernels
        tokio::spawn(async move {
            let mut socket = socket;
            while *running.read().await {
                // Wait for a request from a kernel
                match socket.recv().await {
                    Ok(request_data) => {
                        let request_data = request_data.into(); // Convert ZmqMessage to Vec<u8>
                        Self::handle_handshake_request(&mut socket, &result_tx, request_data).await;
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

        // Clear the stored port
        let mut port_lock = REGISTRATION_PORT.write().await;
        *port_lock = None;
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
