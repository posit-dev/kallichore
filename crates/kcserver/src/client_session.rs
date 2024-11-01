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
use tokio::select;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::kernel_connection::KernelConnection;
use crate::kernel_state::KernelState;

pub struct ClientSession {
    pub connection: KernelConnection,
    pub client_id: String,
    ws_json_rx: Receiver<WebsocketMessage>,
    ws_zmq_tx: Sender<JupyterMessage>,
    state: Arc<RwLock<KernelState>>,
}

// A client session counter
static mut SESSION_COUNTER: atomic::AtomicU32 = atomic::AtomicU32::new(0);

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
        let session_id = format!("{}-{}", connection.session_id.clone(), unsafe {
            SESSION_COUNTER.fetch_add(1, atomic::Ordering::SeqCst)
        });
        Self {
            connection,
            ws_json_rx,
            ws_zmq_tx,
            client_id: session_id,
            state,
        }
    }

    async fn handle_ws_message(&self, data: Vec<u8>) {
        // parse the message into a JupyterMessage
        let channel_message = serde_json::from_slice::<JupyterMessage>(&data);

        // if the message is not a Jupyter message, log an error and return
        let channel_message = match channel_message {
            Ok(channel_message) => channel_message,
            Err(e) => {
                // Convert the vector to a string for logging
                let data = match String::from_utf8(data) {
                    Ok(data) => data,
                    Err(e) => {
                        log::error!("Failed to convert message to string: {}", e);
                        return;
                    }
                };
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
            }
            state.connected = true;
        }

        // Loop to handle messages from the websocket and the ZMQ channel
        loop {
            select! {
                from_socket = ws_stream.next() => {
                    let data = match from_socket {
                        Some(data) => match data {
                            Ok(data) => data.into_data(),
                            Err(e) => {
                                log::error!("Failed to read data from websocket: {}", e);
                                break;
                            }
                        },
                        None => {
                            log::info!("No data from websocket; closing");
                            break;
                        }
                    };
                    if data.is_empty() {
                        log::info!("Empty message from websocket; closing");
                        break;
                    }
                    self.handle_ws_message(data).await;
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
