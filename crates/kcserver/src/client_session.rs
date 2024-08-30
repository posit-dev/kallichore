//
// client_session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::sync::Arc;

use async_channel::Receiver;
use async_channel::Sender;
use bytes::Bytes;
use futures::SinkExt;
use futures::StreamExt;
use hyper::upgrade::Upgraded;
use kcshared::jupyter_message::JupyterMessage;
use tokio::select;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use zeromq::ZmqMessage;

use crate::kernel_connection::KernelConnection;
use crate::kernel_state::KernelState;
use crate::wire_message::WireMessage;
use crate::wire_message::ZmqChannelMessage;
use crate::wire_message::MSG_DELIM;

pub struct ClientSession {
    pub connection: KernelConnection,
    ws_json_rx: Receiver<String>,
    ws_zmq_tx: Sender<ZmqChannelMessage>,
    state: Arc<RwLock<KernelState>>,
}

impl ClientSession {
    pub fn new(
        connection: KernelConnection,
        ws_json_rx: Receiver<String>,
        ws_zmq_tx: Sender<ZmqChannelMessage>,
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

        // Convert the message to a wire message
        let channel = channel_message.channel.clone();
        let wire_message = WireMessage::from_jupyter(
            channel_message,
            self.connection.session_id.clone(),
            self.connection.username.clone(),
            self.connection.hmac_key.clone(),
        )
        .unwrap();

        let mut zmq_mesage = ZmqMessage::from(MSG_DELIM.to_vec());
        for part in wire_message.parts {
            zmq_mesage.push_back(Bytes::from(part));
        }

        log::trace!("Sending message to Jupyter");
        match self
            .ws_zmq_tx
            .send(ZmqChannelMessage {
                channel,
                message: zmq_mesage,
            })
            .await
        {
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
