//
// zmq_ws_proxy.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use async_channel::{Receiver, Sender};
use kcshared::jupyter_message::JupyterChannel;
use tokio::select;
use zeromq::{DealerSocket, ReqSocket, Socket, SocketRecv, SocketSend, SubSocket, ZmqMessage};

use crate::{
    connection_file::ConnectionFile,
    kernel_connection::KernelConnection,
    wire_message::{WireMessage, ZmqChannelMessage},
};

async fn forward_zmq(
    channel: JupyterChannel,
    message: ZmqMessage,
    ws_json_tx: Sender<String>,
) -> Result<(), anyhow::Error> {
    let message = WireMessage::from(message);
    let message = message.to_jupyter(channel)?;
    let payload = serde_json::to_string(&message)?;
    match ws_json_tx.send(payload).await {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "Failed to send message to websocket: {}",
            e
        )),
    }
}

pub async fn zmq_ws_proxy(
    connection: KernelConnection,
    connection_file: ConnectionFile,
    ws_json_tx: Sender<String>,
    ws_zmq_rx: Receiver<ZmqChannelMessage>,
) -> Result<(), anyhow::Error> {
    log::trace!("Connecting to kernel for session {}", connection.session_id);

    let mut shell_socket = DealerSocket::new();
    let mut hb_socket = ReqSocket::new();
    let mut iopub_socket = SubSocket::new();
    shell_socket
        .connect(
            format!(
                "tcp://{}:{}",
                connection_file.ip, connection_file.shell_port
            )
            .as_str(),
        )
        .await?;

    log::trace!(
        "Connected to shell socket on port {}",
        connection_file.shell_port
    );

    hb_socket
        .connect(format!("tcp://{}:{}", connection_file.ip, connection_file.hb_port).as_str())
        .await?;
    log::trace!(
        "Connected to heartbeat socket on port {}",
        connection_file.hb_port
    );

    iopub_socket
        .connect(
            format!(
                "tcp://{}:{}",
                connection_file.ip, connection_file.iopub_port
            )
            .as_str(),
        )
        .await?;
    log::trace!(
        "Connected to iopub socket on port {}",
        connection_file.iopub_port
    );

    // Subscribe to all messages
    iopub_socket.subscribe("").await?;

    log::trace!("Listening for messages from kernel");

    // Wait for a message from any socket
    loop {
        select! {
            shell_msg = shell_socket.recv() => {
                log::info!("Received message from shell socket");
                match shell_msg {
                    Ok(msg) => {
                        log::info!("Received message: {:?}", msg);
                        forward_zmq(JupyterChannel::Shell, msg, ws_json_tx.clone()).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from shell socket: {}", e);
                    },
                }
            },
            iopub_msg = iopub_socket.recv() => {
                match iopub_msg {
                    Ok(msg) => {
                        log::info!("Received message: {:?}", msg);
                        forward_zmq(JupyterChannel::IOPub, msg, ws_json_tx.clone()).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from iopub socket: {}", e);
                    },
                }
            },
            ws_msg = ws_zmq_rx.recv() => {
                match ws_msg {
                    Ok(msg) => {
                        log::trace!("Received message from websocket");
                        match msg.channel {
                            JupyterChannel::Shell => {
                                log::trace!("Sending message to shell socket");
                                shell_socket.send(msg.message).await?;
                                log::trace!("Sent message to shell socket");
                            },
                            _ => {
                                log::error!("Unsupported channel: {:?}", msg.channel);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to receive message from websocket: {}", e);
                    },
                }
            }
        };
    }
}
