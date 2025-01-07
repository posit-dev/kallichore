//
// client_session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
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
use kcshared::websocket_message::WebsocketMessage;
use once_cell::sync::Lazy;
use tokio::select;
use tokio::sync::RwLock;
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
}

// An atomic counter for generating unique client IDs
static SESSION_COUNTER: Lazy<atomic::AtomicU32> = Lazy::new(|| atomic::AtomicU32::new(0));

impl ClientSession {
    pub fn new(
        connection: KernelConnection,
        ws_json_rx: Receiver<WebsocketMessage>,
        ws_zmq_tx: Sender<JupyterMessage>,
        state: Arc<RwLock<KernelState>>,
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

    pub async fn handle_channel_ws(&self, mut ws_stream: WebSocketStream<Upgraded>) {
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
            state.set_connected(true).await
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
                    log::info!("[client {}] Disconnecting", self.client_id);
                    break;
                }
            }
        }

        // Mark the session as disconnected
        {
            let mut state = self.state.write().await;
            state.connected = false;
        }
    }
}
