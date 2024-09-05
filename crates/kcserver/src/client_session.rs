//
// client_session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

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
    ws_json_rx: Receiver<WebsocketMessage>,
    ws_zmq_tx: Sender<JupyterMessage>,
    state: Arc<RwLock<KernelState>>,
}

impl ClientSession {
    pub fn new(
        connection: KernelConnection,
        ws_json_rx: Receiver<WebsocketMessage>,
        ws_zmq_tx: Sender<JupyterMessage>,
        state: Arc<RwLock<KernelState>>,
    ) -> Self {
        Self {
            connection,
            ws_json_rx,
            ws_zmq_tx,
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
                log::error!("Failed to parse Jupyter message: {}", e);
                return;
            }
        };

        // Log the message ID and type
        log::info!(
            "Got message {} of type {}",
            channel_message.header.msg_id.clone(),
            channel_message.header.msg_type.clone()
        );

        log::trace!("Sending message to Jupyter");
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
                            log::error!("No data from websocket");
                            break;
                        }
                    };
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
                            log::error!("Failed to receive message from websocket: {}", e);
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
