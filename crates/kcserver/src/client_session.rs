//
// client_session.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

use std::sync::atomic;
use std::sync::Arc;

use async_channel::Receiver;
use async_channel::Sender;
use futures::SinkExt;
use futures::StreamExt;
use hyper::upgrade::Upgraded;
use kcshared::jupyter_message::JupyterMessage;
use kcshared::kernel_message::KernelMessage;
use kcshared::websocket_message::WebsocketMessage;
use once_cell::sync::Lazy;
use tokio::select;
use tokio::sync::RwLock;
#[cfg(unix)]
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::kernel_connection::KernelConnection;
use crate::kernel_state::KernelState;
use event_listener::Event;

#[derive(Clone)]
pub struct ClientSession {
    /// Metadata about the connection to the Jupyter side of the kernel
    pub connection: KernelConnection,

    /// The unique ID of the client session
    pub client_id: String,

    /// The receiver for messages to be sent to the websocket
    ws_json_rx: Receiver<WebsocketMessage>,

    /// The sender for messages to be sent to the Jupyter kernel
    ws_zmq_tx: Sender<JupyterMessage>,

    /// The current state of the kernel
    state: Arc<RwLock<KernelState>>,

    /// An event that can be triggered to disconnect the session; used when we
    /// need to reconnect a new client to the same kernel.
    pub disconnect: Arc<Event>,

    /// The Unix domain socket path associated with this session, if any
    pub socket_path: Option<String>,
}

// An atomic counter for generating unique client IDs
static SESSION_COUNTER: Lazy<atomic::AtomicU32> = Lazy::new(|| atomic::AtomicU32::new(0));

impl ClientSession {
    pub fn new(
        connection: KernelConnection,
        ws_json_rx: Receiver<WebsocketMessage>,
        ws_zmq_tx: Sender<JupyterMessage>,
        state: Arc<RwLock<KernelState>>,
        socket_path: Option<String>,
    ) -> Self {
        // Derive a unique client ID for this connection by combining the
        // session ID and a counter
        #[allow(unsafe_code)]
        let session_id = format!(
            "{}-{}",
            connection.session_id.clone(),
            SESSION_COUNTER.fetch_add(1, atomic::Ordering::SeqCst)
        );
        Self {
            connection,
            ws_json_rx,
            ws_zmq_tx,
            client_id: session_id,
            state,
            disconnect: Arc::new(Event::new()),
            socket_path,
        }
    }

    async fn handle_ws_message(&self, data: String) {
        // parse the message into a JupyterMessage
        let channel_message = serde_json::from_str::<JupyterMessage>(&data);

        // if the message is not a Jupyter message, log an error and return
        let channel_message = match channel_message {
            Ok(channel_message) => channel_message,
            Err(e) => {
                log::error!(
                    "Failed to parse Jupyter message: {}. Raw message: {:?}",
                    e,
                    data
                );
                return;
            }
        };

        // Log the message ID and type
        log::info!(
            "[client {}] Got message {} of type {}; sending to Jupyter socket {:?}",
            self.client_id,
            channel_message.header.msg_id.clone(),
            channel_message.header.msg_type.clone(),
            channel_message.channel
        );

        match self.ws_zmq_tx.send(channel_message).await {
            Ok(_) => {
                log::trace!("Sent message to Jupyter");
            }
            Err(e) => {
                log::error!("Failed to send message to Jupyter: {}", e);
            }
        }
    }

    pub async fn handle_channel_ws(&self, ws_stream: WebSocketStream<Upgraded>) {
        self.handle_websocket_stream(ws_stream).await;
    }

    #[cfg(unix)]
    async fn handle_websocket_unix_stream(
        &self,
        ws_stream: WebSocketStream<tokio::net::UnixStream>,
    ) {
        self.handle_websocket_stream(ws_stream).await;
    }

    // Generic method to handle WebSocket streams regardless of the underlying transport
    async fn handle_websocket_stream<S>(&self, mut ws_stream: WebSocketStream<S>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        // Mark the session as connected
        {
            let mut state = self.state.write().await;

            // We should never receive a connection request for an
            // already-connected session.
            if state.connected {
                log::warn!(
                    "[client {}] Received connection request for already-connected session.",
                    self.client_id
                );
            } else {
                log::info!("[client {}] Connecting to websocket", self.client_id);
            }
            state.set_connected(true).await;
            // Set the client socket path in the kernel state
            state.set_client_socket_path(self.socket_path.clone());
        }

        // Interval timer for client pings
        let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(10));

        // Ping counters
        let mut ping_outbound: u64 = 0;
        let mut pong_inbound: u64 = 0;

        // Loop to handle messages from the websocket and the ZMQ channel
        loop {
            select! {
                from_socket = ws_stream.next() => {
                    let message = match from_socket {
                        Some(out) => match out {
                            Ok(m) => m,
                            Err(e) => {
                                log::error!("[client {}] Failed to read data from websocket: {}", self.client_id, e);
                                break;
                            }
                        },
                        None => {
                            log::info!("[client {}] No data from websocket; closing", self.client_id);
                            break;
                        }
                    };
                    match message {
                        Message::Text(data) => {
                            if data.is_empty() {
                                log::info!("[client {}] Empty message from websocket; closing", self.client_id);
                                break;
                            }
                            self.handle_ws_message(data).await;
                        },
                        Message::Ping(data) => {
                            // Tungstenite should handle the pong response, so
                            // we just log the ping
                            log::trace!("[client {}] Got ping from websocket ({} bytes)", self.client_id, data.len());
                        },
                        Message::Pong(data) => {
                            // Sanity check data size
                            if data.len() != 8 {
                                log::warn!("[client {}] Got pong with invalid data size ({} bytes); ignoring", self.client_id, data.len());
                                continue;
                            }

                            // Log the pong and update the counter
                            let last_pong = pong_inbound;
                            pong_inbound = u64::from_be_bytes(data.as_slice().try_into().unwrap());

                            // We expect the pong to be one more than the last pong
                            if pong_inbound != last_pong + 1 {
                                log::warn!("[client {}] Got pong {} from websocket; expected {}", self.client_id, pong_inbound, last_pong + 1);
                            }

                            log::trace!("[client {}] Got pong {} from websocket", self.client_id, pong_inbound);
                        },
                        Message::Binary(data) => {
                            // Ignore binary messages for now
                            log::warn!("[client {}] Got binary message from websocket ({} bytes); ignoring", self.client_id, data.len());
                        },
                        Message::Frame(_) => {
                            // Ignore frame messages; these are not received by socket reads.
                        },
                        Message::Close(_) => {
                            log::info!("[client {}] Websocket closed by client", self.client_id);
                            break;
                        },
                    }
                },
                json = self.ws_json_rx.recv() => {
                    match json {
                        Ok(json) => {
                            let json = serde_json::to_string(&json).unwrap();
                            match ws_stream.send(Message::text(json)).await {
                                Ok(_) => {}
                                Err(e) => {
                                    log::error!("Failed to send message to websocket: {}", e);
                                }
                            }
                        },
                        Err(e) => {
                            log::error!("Failed to receive websocket message: {}", e);
                        }
                    }
                },
                _ = tick.tick() => {
                    // Check to see how far behind the pong counter is
                    let diff = ping_outbound - pong_inbound;
                    if diff > 3 {
                        log::warn!("[client {}] Lost connection with client; websocket pong counter is behind by {} pings", self.client_id, diff);
                        break;
                    }

                    // Send a ping
                    ping_outbound += 1;
                    let ping_data = ping_outbound.to_be_bytes().to_vec();
                    match ws_stream.send(Message::Ping(ping_data)).await {
                        Ok(_) => {
                            log::trace!("[client {}] Ping {} / Pong {}", self.client_id, ping_outbound, pong_inbound);
                        }
                        Err(e) => {
                            log::error!("[client {}] Failed to send ping to websocket: {}", self.client_id, e);
                            break;
                        }
                    }
                },
                _ = self.disconnect.listen() => {
                    // This event is fired when we need to disconnect the
                    // session because another client is connecting to the same
                    // kernel. We need to send a message to the client and close
                    // the websocket.
                    log::info!("[client {}] Disconnecting", self.client_id);

                    // Create a message to send to the client
                    let close_msg = WebsocketMessage::Kernel(KernelMessage::ClientDisconnected("Another client is connecting to this session.".to_string()));
                    let close_msg = serde_json::to_string(&close_msg).unwrap();
                    ws_stream.send(Message::Text(close_msg)).await.unwrap();

                    // Send a close message to the websocket
                    ws_stream.send(Message::Close(None)).await.unwrap();

                    break;
                }
            }
        }

        // Mark the session as disconnected
        {
            let mut state = self.state.write().await;
            state.connected = false;
            // Clear the client socket path when disconnecting
            state.set_client_socket_path(None);
        }
    }

    /// Detect if incoming stream data starts with HTTP WebSocket upgrade request
    /// Returns true if HTTP request detected, false if timeout or raw protocol detected
    async fn detect_http_websocket_request<T>(
        reader: &mut tokio::io::BufReader<T>,
        client_id: &str,
        transport_name: &str,
    ) -> Option<bool>
    where
        T: tokio::io::AsyncRead + Unpin,
    {
        use tokio::io::AsyncBufReadExt;
        let timeout_duration = tokio::time::Duration::from_millis(5000); // 5 second timeout
        match tokio::time::timeout(timeout_duration, async {
            loop {
                match reader.fill_buf().await {
                    Ok(buf) if buf.len() >= 4 => {
                        // We have enough data to check for HTTP request
                        break Some(buf.starts_with(b"GET "));
                    }
                    Ok(buf) if buf.is_empty() => {
                        // Connection closed
                        return None;
                    }
                    Ok(_) => {
                        // Need more data, continue waiting
                        continue;
                    }
                    Err(e) => {
                        log::error!(
                            "[client {}] Failed to peek at {} data: {}",
                            client_id,
                            transport_name,
                            e
                        );
                        return None;
                    }
                }
            }
        }).await {
            Ok(Some(is_http)) => Some(is_http),
            Ok(None) => {
                // Connection closed or error occurred
                None
            }
            Err(_) => {
                // Timeout occurred - assume non-HTTP protocol
                log::warn!(
                    "[client {}] Timeout waiting for HTTP request data on {}, assuming non-HTTP protocol",
                    client_id,
                    transport_name
                );
                Some(false)
            }
        }
    }

    /// Handle a Unix domain socket connection with WebSocket protocol support
    #[cfg(unix)]
    pub async fn handle_domain_socket_connection(
        &self,
        stream: tokio::net::UnixStream,
        _session_id: String,
    ) {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

        log::info!(
            "[client {}] Handling Unix domain socket connection with WebSocket protocol",
            self.client_id
        );

        // The client may send either a raw WebSocket connection or an HTTP
        // WebSocket upgrade request. We need to peek at the first few bytes
        // when they arrive to determine which it is.
        let mut reader = BufReader::new(stream);
        let is_http_request = match Self::detect_http_websocket_request(&mut reader, &self.client_id, "Unix socket").await {
            Some(is_http) => is_http,
            None => {
                // Connection closed or error occurred
                return;
            }
        };

        if is_http_request {
            // This looks like an HTTP WebSocket upgrade request; honor it and
            // perform the WebSocket handshake
            log::debug!(
                "[client {}] Detected WebSocket handshake request",
                self.client_id
            );

            // Now read the first line (which we know is there)
            let mut first_line = String::new();
            if reader.read_line(&mut first_line).await.is_err() {
                return;
            }

            // Read the rest of the headers
            let mut headers = std::collections::HashMap::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_err() {
                    break;
                }
                let line = line.trim();
                if line.is_empty() {
                    break; // End of headers
                }
                if let Some((key, value)) = line.split_once(':') {
                    headers.insert(key.trim().to_lowercase(), value.trim().to_string());
                }
            }

            // Generate WebSocket accept key
            if let Some(websocket_key) = headers.get("sec-websocket-key") {
                let accept_key = derive_accept_key(websocket_key.as_bytes());

                // Send WebSocket handshake response
                let response = format!(
                    "HTTP/1.1 101 Switching Protocols\r\n\
                     Upgrade: websocket\r\n\
                     Connection: Upgrade\r\n\
                     Sec-WebSocket-Accept: {}\r\n\r\n",
                    accept_key
                );

                let stream = reader.into_inner();
                if let Err(e) = stream.writable().await {
                    log::error!("[client {}] Stream not writable: {}", self.client_id, e);
                    return;
                }

                if let Err(e) = stream.try_write(response.as_bytes()) {
                    log::error!(
                        "[client {}] Failed to send WebSocket handshake response: {}",
                        self.client_id,
                        e
                    );
                    return;
                }

                log::debug!(
                    "[client {}] Sent WebSocket handshake response",
                    self.client_id
                );

                // Now treat it as a raw WebSocket connection (handshake complete)
                let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;

                log::info!(
                    "[client {}] Successfully created WebSocket stream from Unix domain socket (with handshake)",
                    self.client_id
                );

                self.handle_websocket_unix_stream(ws_stream).await;
            } else {
                log::error!(
                    "[client {}] WebSocket upgrade request missing Sec-WebSocket-Key header",
                    self.client_id
                );
            }
        } else {
            // No HTTP request detected, treat as raw WebSocket connection
            log::debug!(
                "[client {}] No WebSocket handshake detected, using raw WebSocket protocol",
                self.client_id
            );

            // Convert back to the original stream - any buffered data will be preserved
            let stream = reader.into_inner();
            let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;

            log::info!(
                "[client {}] Successfully created WebSocket stream from Unix domain socket (raw)",
                self.client_id
            );

            self.handle_websocket_unix_stream(ws_stream).await;
        }
    }

    /// Handle a Windows named pipe stream connection with WebSocket protocol support
    #[cfg(windows)]
    pub async fn handle_named_pipe_stream<T>(&self, stream: T, session_id: String)
    where
        T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
        use tokio_tungstenite::tungstenite::protocol::Role;

        log::info!(
            "[client {}] Handling named pipe connection with WebSocket protocol for session {}",
            self.client_id,
            session_id
        );

        // Use BufReader to peek at data without consuming it initially
        let mut reader = BufReader::new(stream);

        // Peek at the buffered data to check for HTTP request without consuming it
        let is_http_request = match Self::detect_http_websocket_request(&mut reader, &self.client_id, "named pipe").await {
            Some(is_http) => is_http,
            None => {
                // Connection closed or error occurred
                return;
            }
        };

        if is_http_request {
            // This looks like an HTTP WebSocket upgrade request
            log::debug!(
                "[client {}] Detected WebSocket handshake request on named pipe",
                self.client_id
            );

            // Now read the first line (which we know is there)
            let mut first_line = String::new();
            if reader.read_line(&mut first_line).await.is_err() {
                return;
            }

            // Read the rest of the headers
            let mut headers = std::collections::HashMap::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_err() {
                    break;
                }
                let line = line.trim();
                if line.is_empty() {
                    break; // End of headers
                }
                if let Some((key, value)) = line.split_once(':') {
                    headers.insert(key.trim().to_lowercase(), value.trim().to_string());
                }
            }

            // Generate WebSocket accept key
            if let Some(websocket_key) = headers.get("sec-websocket-key") {
                let accept_key = derive_accept_key(websocket_key.as_bytes());

                // Send WebSocket handshake response
                let response = format!(
                    "HTTP/1.1 101 Switching Protocols\r\n\
                     Upgrade: websocket\r\n\
                     Connection: Upgrade\r\n\
                     Sec-WebSocket-Accept: {}\r\n\r\n",
                    accept_key
                );

                let mut stream = reader.into_inner();
                use tokio::io::AsyncWriteExt;
                if let Err(e) = stream.write_all(response.as_bytes()).await {
                    log::error!(
                        "[client {}] Failed to send WebSocket handshake response on named pipe: {}",
                        self.client_id,
                        e
                    );
                    return;
                }

                if let Err(e) = stream.flush().await {
                    log::error!(
                        "[client {}] Failed to flush WebSocket handshake response on named pipe: {}",
                        self.client_id,
                        e
                    );
                    return;
                }

                log::debug!(
                    "[client {}] Sent WebSocket handshake response on named pipe",
                    self.client_id
                );

                // Now treat it as a raw WebSocket connection (handshake complete)
                let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;

                log::info!(
                    "[client {}] Successfully created WebSocket stream from named pipe (with handshake)",
                    self.client_id
                );

                self.handle_websocket_stream(ws_stream).await;
            } else {
                log::error!(
                    "[client {}] WebSocket upgrade request missing Sec-WebSocket-Key header on named pipe",
                    self.client_id
                );
            }
        } else {
            // No HTTP request detected, fall back to raw JSON protocol
            log::debug!(
                "[client {}] No WebSocket handshake detected on named pipe, using raw JSON protocol",
                self.client_id
            );

            // Convert back to the original stream - any buffered data will be preserved
            let stream = reader.into_inner();
            self.handle_named_pipe_stream_raw(stream, session_id).await;
        }
    }

    /// Handle a Windows named pipe stream connection (raw JSON protocol)
    #[cfg(windows)]
    pub async fn handle_named_pipe_stream_raw<T>(&self, mut stream: T, session_id: String)
    where
        T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        log::info!(
            "[client {}] Named pipe stream handler started for session {} (raw mode)",
            self.client_id,
            session_id
        );

        // Mark the session as connected
        {
            let mut state = self.state.write().await;
            state.set_connected(true).await;
            // Set the client socket path in the kernel state (None for named pipes)
            state.set_client_socket_path(self.socket_path.clone());
        } // Create channels for bidirectional communication
        let (tx_to_pipe, mut rx_from_kernel) = tokio::sync::mpsc::channel::<String>(100);
        let (tx_to_kernel, rx_from_pipe) = tokio::sync::mpsc::channel::<String>(100);

        // Spawn task to handle kernel messages and send them to the pipe
        let client_id_clone = self.client_id.clone();
        let ws_json_rx_clone = self.ws_json_rx.clone();
        tokio::spawn(async move {
            let receiver = ws_json_rx_clone;
            while let Ok(message) = receiver.recv().await {
                log::debug!(
                    "[client {}] Sending kernel message to named pipe: {:?}",
                    client_id_clone,
                    message
                );
                let json_str = serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string());
                // Add newline delimiter for proper message framing
                let json_with_newline = format!("{}\n", json_str);
                if tx_to_pipe.send(json_with_newline).await.is_err() {
                    log::debug!(
                        "[client {}] Named pipe channel closed, stopping kernel message forwarder",
                        client_id_clone
                    );
                    break;
                }
            }
        });

        // Spawn task to handle pipe messages and send them to the kernel
        let client_id_clone = self.client_id.clone();
        let ws_zmq_tx_clone = self.ws_zmq_tx.clone();
        let mut rx_from_pipe = rx_from_pipe;
        tokio::spawn(async move {
            while let Some(message_str) = rx_from_pipe.recv().await {
                log::debug!(
                    "[client {}] Received message from named pipe: {}",
                    client_id_clone,
                    message_str
                );

                // Parse and forward to kernel
                match serde_json::from_str::<WebsocketMessage>(&message_str) {
                    Ok(ws_message) => match ws_message {
                        WebsocketMessage::Jupyter(jupyter_message) => {
                            if let Err(e) = ws_zmq_tx_clone.send(jupyter_message).await {
                                log::error!(
                                    "[client {}] Failed to send message to kernel: {}",
                                    client_id_clone,
                                    e
                                );
                                break;
                            }
                        }
                        WebsocketMessage::Kernel(_) => {
                            log::debug!(
                                "[client {}] Received kernel message on named pipe (ignoring)",
                                client_id_clone
                            );
                        }
                    },
                    Err(e) => {
                        log::error!(
                            "[client {}] Failed to parse named pipe message: {} - {}",
                            client_id_clone,
                            e,
                            message_str
                        );
                    }
                }
            }
        });

        // Main I/O loop
        let mut buffer = [0u8; 4096];
        loop {
            tokio::select! {
                // Read from named pipe
                read_result = stream.read(&mut buffer) => {
                    match read_result {
                        Ok(0) => {
                            log::info!("[client {}] Named pipe client disconnected", self.client_id);
                            break;
                        },
                        Ok(bytes_read) => {
                            let received_data = String::from_utf8_lossy(&buffer[..bytes_read]);
                            log::debug!("[client {}] Received from named pipe: {}", self.client_id, received_data);

                            // Send to kernel handler
                            if tx_to_kernel.send(received_data.to_string()).await.is_err() {
                                log::debug!("[client {}] Kernel channel closed", self.client_id);
                                break;
                            }
                        },
                        Err(e) => {
                            log::error!("[client {}] Failed to read from named pipe: {}", self.client_id, e);
                            break;
                        }
                    }
                },
                // Write to named pipe
                message = rx_from_kernel.recv() => {
                    match message {
                        Some(msg) => {
                            if let Err(e) = stream.write_all(msg.as_bytes()).await {
                                log::error!("[client {}] Failed to write to named pipe: {}", self.client_id, e);
                                break;
                            }
                            if let Err(e) = stream.flush().await {
                                log::error!("[client {}] Failed to flush named pipe: {}", self.client_id, e);
                                break;
                            }
                        },
                        None => {
                            log::debug!("[client {}] Kernel message channel closed", self.client_id);
                            break;
                        }
                    }
                }
            }
        }

        // Mark the session as disconnected
        {
            let mut state = self.state.write().await;
            state.set_connected(false).await;
            // Clear the client socket path when disconnecting
            state.set_client_socket_path(None);
        }

        // Notify that this client is disconnecting
        self.disconnect.notify(usize::MAX);

        log::info!("[client {}] Named pipe connection closed", self.client_id);
    }
}
