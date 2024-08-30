//
// zmq_ws_proxy.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use async_channel::{Receiver, Sender};
use kcshared::{jupyter_message::JupyterChannel, websocket_message::WebsocketMessage};
use tokio::select;
use zeromq::{DealerSocket, ReqSocket, Socket, SocketRecv, SocketSend, SubSocket, ZmqMessage};

use crate::{
    connection_file::ConnectionFile,
    kernel_connection::KernelConnection,
    wire_message::{WireMessage, ZmqChannelMessage},
};

/// Forward a message from a ZeroMQ socket to a WebSocket channel.
///
/// - `channel`: The channel to forward the message to
/// - `message`: The message to forward
/// - `ws_json_tx`: The channel to send JSON messages to the WebSocket
async fn forward_zmq(
    channel: JupyterChannel,
    message: ZmqMessage,
    ws_json_tx: Sender<String>,
) -> Result<(), anyhow::Error> {
    // (1) convert the raw parts/frames of the message into a `WireMessage`.
    let message = WireMessage::from(message);

    // (2) convert it into a Jupyter message; this can fail if the message is
    // not a valid Jupyter message.
    let message = message.to_jupyter(channel)?;

    // (3) wrap the Jupyter message in a `WebsocketMessage::Jupyter` and send it
    // to the WebSocket.
    let message = WebsocketMessage::Jupyter(message);
    let payload = serde_json::to_string(&message)?;
    match ws_json_tx.send(payload).await {
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
/// - `ws_json_tx`: A channel to send JSON messages to the WebSocket
/// - `ws_zmq_rx`: A channel to receive messages from the WebSocket
///
/// Async; does not return until the connection is closed.
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
            control_msg = control_socket.recv() => {
                match control_msg {
                    Ok(msg) => {
                        log::info!("Received message: {:?}", msg);
                        forward_zmq(JupyterChannel::Control, msg, ws_json_tx.clone()).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from control socket: {}", e);
                    },
                }
            },
            stdin_msg = stdin_socket.recv() => {
                match stdin_msg {
                    Ok(msg) => {
                        log::info!("Received message: {:?}", msg);
                        forward_zmq(JupyterChannel::Stdin, msg, ws_json_tx.clone()).await?;
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
