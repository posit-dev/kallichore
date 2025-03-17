//
// registration_socket.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

use anyhow::Result;
use kcshared::handshake_protocol::{
    HandshakeRequest, HandshakeReply, HandshakeStatus, HandshakeVersion,
    DEFAULT_REGISTRATION_PORT, JEP66_PROTOCOL_VERSION,
};
use kallichore_api::models::ConnectionInfo;
use log::{debug, info, warn};
use std::{
    collections::HashMap,
    sync::Arc,
};
use tokio::{
    sync::{mpsc, RwLock},
};
use zeromq::{RepSocket, Socket, SocketRecv, SocketSend};

/// Message sent when we need to establish a handshake with a kernel
#[derive(Debug, Clone)]
pub struct HandshakeInitRequest {
    /// The connection info for the ZeroMQ sockets
    pub connection_info: ConnectionInfo,
    
    /// The channel to send the handshake reply to
    pub reply_tx: mpsc::Sender<Option<HandshakeReply>>,
}

/// Manages a registration socket for JEP 66 handshaking protocol
pub struct RegistrationSocket {
    /// The port on which the registration socket is listening
    pub port: u16,
    
    /// The ZeroMQ Reply socket for the registration socket
    socket: Option<RepSocket>,
    
    /// Channel for sending handshake requests
    handshake_tx: mpsc::Sender<HandshakeInitRequest>,
    
    /// Flag indicating if the socket is running
    running: Arc<RwLock<bool>>,
}

impl RegistrationSocket {
    /// Create a new registration socket
    pub fn new(port: Option<u16>) -> Self {
        let (handshake_tx, _) = mpsc::channel(32);
        
        Self {
            port: port.unwrap_or(DEFAULT_REGISTRATION_PORT),
            socket: None,
            handshake_tx,
            running: Arc::new(RwLock::new(false)),
        }
    }
    
    /// Start listening on the registration socket
    pub async fn start(&mut self) -> Result<()> {
        // Create and bind the ZeroMQ REP socket
        let mut socket = RepSocket::new();
        let address = format!("tcp://*:{}", self.port);
        socket.bind(&address).await?;
        
        // Store the socket
        self.socket = Some(socket);
        
        // Set the running flag
        *self.running.write().await = true;
        
        // Create a channel for processing handshake requests
        let (process_tx, mut process_rx) = mpsc::channel::<HandshakeInitRequest>(32);
        
        // Update our handshake_tx to the processor channel
        self.handshake_tx = process_tx.clone();
        
        // Get a clone of the socket and running flag for the message handling task
        let mut socket_clone = self.socket.as_ref().unwrap().clone();
        let running = self.running.clone();
        
        info!("Started JEP 66 registration REP socket on port {}", self.port);
        
        // Spawn a task to handle incoming registration requests
        tokio::spawn(async move {
            while *running.read().await {
                // Wait for a request from a kernel
                match socket_clone.recv().await {
                    Ok(request_data) => {
                        // Convert ZmqMessage to bytes
                        let bytes_vec = request_data.into_vec();
                        let data: Vec<u8> = bytes_vec.iter().flat_map(|b| b.to_vec()).collect();
                        
                        // Try to deserialize the request to a HandshakeRequest
                        match serde_json::from_slice::<HandshakeRequest>(&data) {
                            Ok(request) => {
                                debug!(
                                    "Received handshake request with protocol version {}",
                                    request.protocol_version
                                );
                                
                                // Check if the request is from a kernel that supports JEP 66
                                if HandshakeVersion::supports_handshaking(&request.protocol_version) {
                                    // Create a successful reply
                                    let reply = HandshakeReply {
                                        status: HandshakeStatus::Ok,
                                        error: None,
                                        capabilities: HashMap::new(),
                                    };
                                    
                                    // Serialize the reply
                                    let reply_data = serde_json::to_vec(&reply)
                                        .expect("Failed to serialize handshake reply");
                                    
                                    // Send the reply
                                    if let Err(e) = socket_clone.send(reply_data.into()).await {
                                        warn!("Failed to send handshake reply: {}", e);
                                    } else {
                                        info!("Sent successful handshake reply to kernel");
                                    }
                                } else {
                                    // Create an error reply
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
                                    
                                    // Send the reply
                                    if let Err(e) = socket_clone.send(reply_data.into()).await {
                                        warn!("Failed to send handshake reply: {}", e);
                                    } else {
                                        warn!(
                                            "Sent error handshake reply to kernel with unsupported protocol version {}",
                                            request.protocol_version
                                        );
                                    }
                                }
                            },
                            Err(e) => {
                                warn!("Failed to deserialize handshake request: {}", e);
                                
                                // Send an error reply
                                let reply = HandshakeReply {
                                    status: HandshakeStatus::Error,
                                    error: Some(format!("Failed to parse handshake request: {}", e)),
                                    capabilities: HashMap::new(),
                                };
                                
                                // Serialize the reply
                                let reply_data = serde_json::to_vec(&reply)
                                    .expect("Failed to serialize handshake reply");
                                
                                // Send the reply
                                if let Err(e) = socket_clone.send(reply_data.into()).await {
                                    warn!("Failed to send handshake reply: {}", e);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        warn!("Error receiving handshake request: {}", e);
                    }
                }
            }
        });
        
        // Spawn a task to handle outgoing handshake requests
        let running = self.running.clone();
        tokio::spawn(async move {
            Self::process_handshake_requests(&mut process_rx, running).await;
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
    
    /// Process handshake requests from the server to kernels
    async fn process_handshake_requests(
        rx: &mut mpsc::Receiver<HandshakeInitRequest>,
        running: Arc<RwLock<bool>>,
    ) {
        while *running.read().await {
            if let Some(request) = rx.recv().await {
                // Create a ZeroMQ REQ socket to connect to the kernel's registration port
                let mut req_socket = zeromq::ReqSocket::new();
                
                // Extract the connection info
                let conn_info = request.connection_info;
                
                // Build the connection address
                let address = format!("tcp://{}:{}", conn_info.ip, DEFAULT_REGISTRATION_PORT);
                
                // Connect to the kernel's registration socket
                match req_socket.connect(&address).await {
                    Ok(()) => {
                        debug!("Connected to kernel registration socket at {}", address);
                        
                        // Create the handshake request
                        let handshake_request = HandshakeRequest {
                            shell_port: conn_info.shell_port as u16,
                            iopub_port: conn_info.iopub_port as u16,
                            stdin_port: conn_info.stdin_port as u16,
                            control_port: conn_info.control_port as u16,
                            hb_port: conn_info.hb_port as u16,
                            protocol_version: JEP66_PROTOCOL_VERSION.to_string(),
                            capabilities: HashMap::new(),
                        };
                        
                        // Serialize the request
                        let request_data = match serde_json::to_vec(&handshake_request) {
                            Ok(data) => data,
                            Err(e) => {
                                warn!("Failed to serialize handshake request: {}", e);
                                if let Err(e) = request.reply_tx.send(None).await {
                                    warn!("Failed to send handshake reply: {}", e);
                                }
                                continue;
                            }
                        };
                        
                        // Send the request
                        if let Err(e) = req_socket.send(request_data.into()).await {
                            warn!("Failed to send handshake request: {}", e);
                            if let Err(e) = request.reply_tx.send(None).await {
                                warn!("Failed to send handshake reply: {}", e);
                            }
                            continue;
                        }
                        
                        // Wait for a reply with a timeout
                        match tokio::time::timeout(
                            tokio::time::Duration::from_secs(5),
                            req_socket.recv(),
                        ).await {
                            Ok(Ok(reply_data)) => {
                                // Convert ZmqMessage to bytes
                                let bytes_vec = reply_data.into_vec();
                                let data: Vec<u8> = bytes_vec.iter().flat_map(|b| b.to_vec()).collect();
                                
                                // Try to deserialize the reply
                                match serde_json::from_slice::<HandshakeReply>(&data) {
                                    Ok(reply) => {
                                        // Send the reply back to the caller
                                        if let Err(e) = request.reply_tx.send(Some(reply.clone())).await {
                                            warn!("Failed to send handshake reply: {}", e);
                                        } else {
                                            match reply.status {
                                                HandshakeStatus::Ok => {
                                                    info!("Received successful handshake reply from kernel");
                                                },
                                                HandshakeStatus::Error => {
                                                    warn!(
                                                        "Received error handshake reply from kernel: {}",
                                                        reply.error.unwrap_or_else(|| "Unknown error".to_string())
                                                    );
                                                }
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        warn!("Failed to deserialize handshake reply: {}", e);
                                        if let Err(e) = request.reply_tx.send(None).await {
                                            warn!("Failed to send handshake reply: {}", e);
                                        }
                                    }
                                }
                            },
                            Ok(Err(e)) => {
                                warn!("Error receiving handshake reply: {}", e);
                                if let Err(e) = request.reply_tx.send(None).await {
                                    warn!("Failed to send handshake reply: {}", e);
                                }
                            },
                            Err(_) => {
                                warn!("Timeout waiting for handshake reply from kernel");
                                if let Err(e) = request.reply_tx.send(None).await {
                                    warn!("Failed to send handshake reply: {}", e);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        warn!("Failed to connect to kernel registration socket: {}", e);
                        if let Err(e) = request.reply_tx.send(None).await {
                            warn!("Failed to send handshake reply: {}", e);
                        }
                    }
                }
            }
        }
    }
    
    /// Get the sender for handshake requests
    pub fn get_handshake_sender(&self) -> mpsc::Sender<HandshakeInitRequest> {
        self.handshake_tx.clone()
    }
}