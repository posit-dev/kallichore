//
// zmq_ws_proxy.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::sync::Arc;

use async_channel::{Receiver, Sender};
use kallichore_api::models;
use kcshared::{jupyter_message::JupyterChannel, websocket_message::WebsocketMessage};
use serde::Deserialize;
use tokio::{select, sync::RwLock};
use zeromq::{DealerSocket, ReqSocket, Socket, SocketRecv, SocketSend, SubSocket, ZmqMessage};

use crate::{
    connection_file::ConnectionFile,
    kernel_connection::KernelConnection,
    kernel_state::KernelState,
    wire_message::{WireMessage, ZmqChannelMessage},
};

#[derive(Deserialize)]
struct StatusMessage {
    execution_state: String,
}

/// Forward a message from a ZeroMQ socket to a WebSocket channel.
///
/// - `channel`: The channel to forward the message to
/// - `message`: The message to forward
/// - `state`: The current state of the kernel
/// - `ws_json_tx`: The channel to send JSON messages to the WebSocket
async fn forward_zmq(
    channel: JupyterChannel,
    message: ZmqMessage,
    state: Arc<RwLock<KernelState>>,
    ws_json_tx: Sender<WebsocketMessage>,
) -> Result<(), anyhow::Error> {
    // (1) convert the raw parts/frames of the message into a `WireMessage`.
    let message = WireMessage::from(message);

    // (2) convert it into a Jupyter message; this can fail if the message is
    // not a valid Jupyter message.
    let message = message.to_jupyter(channel)?;

    // Update the kernel state if the message is a status message
    if channel == JupyterChannel::IOPub {
        match serde_json::from_value::<StatusMessage>(message.content.clone()) {
            Ok(status_message) => {
                let mut state = state.write().await;
                match status_message.execution_state.as_str() {
                    "busy" => {
                        state.set_status(models::Status::Busy).await;
                    }
                    "idle" => {
                        state.set_status(models::Status::Idle).await;
                    }
                    _ => {}
                };
            }
            Err(_) => {}
        }
    }

    // (3) wrap the Jupyter message in a `WebsocketMessage::Jupyter` and send it
    // to the WebSocket.
    let message = WebsocketMessage::Jupyter(message);
    match ws_json_tx.send(message).await {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "Failed to send message to websocket: {}",
            e
        )),
    }
}

/// Establish a proxy between a ZeroMQ connection and a WebSocket connection.
///
/// This function forms the ZeroMQ side of the proxy, receiving messages from
/// the ZeroMQ connection and forwarding them to a channel that delivers them to
/// the WebSocket. It also listens for messages from the WebSocket and forwards
/// them to the ZeroMQ connection.
///
/// - `connection`: Metadata about the kernel connection
/// - `connection_file`: The connection file for the kernel (names the sockets
///    and ports)
/// - `state`: The current state of the kernel
/// - `ws_json_tx`: A channel to send JSON messages to the WebSocket
/// - `ws_zmq_rx`: A channel to receive messages from the WebSocket
///
/// Async; does not return until the connection is closed.
pub async fn zmq_ws_proxy(
    connection: KernelConnection,
    connection_file: ConnectionFile,
    state: Arc<RwLock<KernelState>>,
    ws_json_tx: Sender<WebsocketMessage>,
    ws_zmq_rx: Receiver<ZmqChannelMessage>,
) -> Result<(), anyhow::Error> {
    log::trace!("Connecting to kernel for session {}", connection.session_id);

    let mut shell_socket = DealerSocket::new();
    let mut hb_socket = ReqSocket::new();
    let mut iopub_socket = SubSocket::new();
    let mut control_socket = DealerSocket::new();
    let mut stdin_socket = DealerSocket::new();

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

    control_socket
        .connect(
            format!(
                "tcp://{}:{}",
                connection_file.ip, connection_file.control_port
            )
            .as_str(),
        )
        .await?;
    log::trace!(
        "Connected to control socket on port {}",
        connection_file.control_port
    );

    stdin_socket
        .connect(
            format!(
                "tcp://{}:{}",
                connection_file.ip, connection_file.stdin_port
            )
            .as_str(),
        )
        .await?;
    log::trace!(
        "Connected to stdin socket on port {}",
        connection_file.stdin_port
    );

    log::trace!("Listening for messages from kernel");

    // Wait for a message from any socket
    loop {
        select! {
            shell_msg = shell_socket.recv() => {
                match shell_msg {
                    Ok(msg) => {
                        forward_zmq(JupyterChannel::Shell, msg, state.clone(), ws_json_tx.clone()).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from shell socket: {}", e);
                    },
                }
            },
            iopub_msg = iopub_socket.recv() => {
                match iopub_msg {
                    Ok(msg) => {
                        forward_zmq(JupyterChannel::IOPub, msg, state.clone(), ws_json_tx.clone()).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from iopub socket: {}", e);
                    },
                }
            },
            control_msg = control_socket.recv() => {
                match control_msg {
                    Ok(msg) => {
                        forward_zmq(JupyterChannel::Control, msg, state.clone(), ws_json_tx.clone()).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from control socket: {}", e);
                    },
                }
            },
            stdin_msg = stdin_socket.recv() => {
                match stdin_msg {
                    Ok(msg) => {
                        forward_zmq(JupyterChannel::Stdin, msg, state.clone(), ws_json_tx.clone()).await?;
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
                            JupyterChannel::Control => {
                                log::trace!("Sending message to control socket");
                                // TODO: When an interrupt is sent, we need to clear the execution queue
                                control_socket.send(msg.message).await?;
                                log::trace!("Sent message to control socket");
                            },
                            JupyterChannel::Stdin => {
                                log::trace!("Sending message to stdin socket");
                                stdin_socket.send(msg.message).await?;
                                log::trace!("Sent message to stdin socket");
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
