//
// socket_forwarder.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::sync::Arc;

use futures::stream::SplitSink;
use futures::SinkExt;
use hyper::upgrade::Upgraded;
use kcshared::jupyter_message::JupyterChannel;
use tokio::sync::Mutex;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::wire_message::WireMessage;

pub async fn socket_forwarder<T: zeromq::SocketRecv>(
    channel: JupyterChannel,
    zmq_socket: Arc<Mutex<T>>,
    ws_writer: Arc<Mutex<Option<SplitSink<WebSocketStream<Upgraded>, Message>>>>,
) {
    loop {
        // TODO: handle error
        let mut socket = zmq_socket.lock().await;
        let message = socket.recv().await.unwrap();
        log::info!("Received message: {:?}", message);

        // Convert the message to a Jupyter message
        let message = WireMessage::from(message);
        let message = match message.to_jupyter(channel) {
            Ok(message) => message,
            Err(e) => {
                log::error!("Failed to convert message to Jupyter message: {}", e);
                continue;
            }
        };
        let payload = serde_json::to_string(&message).unwrap();

        // Write the message to the websocket
        let mut ws_write = ws_writer.lock().await;
        match ws_write.as_mut() {
            Some(ws_write) => {
                ws_write.send(Message::text(payload)).await.unwrap();
            }
            None => {
                log::debug!("No websocket to write to; dropping message");
            }
        }
    }
}
