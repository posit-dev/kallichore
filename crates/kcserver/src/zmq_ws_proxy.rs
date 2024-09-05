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
    kernel_state::KernelState,
    wire_message::{WireMessage, ZmqChannelMessage},
};

#[derive(Deserialize)]
struct StatusMessage {
    execution_state: String,
}

pub struct ZmqWsProxy {
    pub shell_socket: DealerSocket,
    pub hb_socket: ReqSocket,
    pub iopub_socket: SubSocket,
    pub control_socket: DealerSocket,
    pub stdin_socket: DealerSocket,
    pub connection_file: ConnectionFile,
    pub ws_json_tx: Sender<WebsocketMessage>,
    pub ws_zmq_rx: Receiver<ZmqChannelMessage>,
    pub state: Arc<RwLock<KernelState>>,
}

impl ZmqWsProxy {
    /// Create a proxy between a ZeroMQ connection and a WebSocket connection.
    ///
    /// This function forms the ZeroMQ side of the proxy, receiving messages from
    /// the ZeroMQ connection and forwarding them to a channel that delivers them to
    /// the WebSocket. It also listens for messages from the WebSocket and forwards
    /// them to the ZeroMQ connection.
    ///
    /// - `connection_file`: The connection file for the kernel (names the sockets
    ///    and ports)
    /// - `state`: The current state of the kernel
    /// - `ws_json_tx`: A channel to send JSON messages to the WebSocket
    /// - `ws_zmq_rx`: A channel to receive messages from the WebSocket
    ///
    /// Async; does not return until the connection is closed.
    pub fn new(
        connection_file: ConnectionFile,
        state: Arc<RwLock<KernelState>>,
        ws_json_tx: Sender<WebsocketMessage>,
        ws_zmq_rx: Receiver<ZmqChannelMessage>,
    ) -> Self {
        Self {
            shell_socket: DealerSocket::new(),
            hb_socket: ReqSocket::new(),
            iopub_socket: SubSocket::new(),
            control_socket: DealerSocket::new(),
            stdin_socket: DealerSocket::new(),
            connection_file,
            ws_json_tx,
            ws_zmq_rx,
            state,
        }
    }

    pub async fn connect(&mut self) -> Result<(), anyhow::Error> {
        self.shell_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.shell_port
                )
                .as_str(),
            )
            .await?;

        log::trace!(
            "Connected to shell socket on port {}",
            self.connection_file.shell_port
        );

        self.hb_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.hb_port
                )
                .as_str(),
            )
            .await?;
        log::trace!(
            "Connected to heartbeat socket on port {}",
            self.connection_file.hb_port
        );

        self.iopub_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.iopub_port
                )
                .as_str(),
            )
            .await?;
        log::trace!(
            "Connected to iopub socket on port {}",
            self.connection_file.iopub_port
        );

        // Subscribe to all messages
        self.iopub_socket.subscribe("").await?;

        self.control_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.control_port
                )
                .as_str(),
            )
            .await?;
        log::trace!(
            "Connected to control socket on port {}",
            self.connection_file.control_port
        );

        self.stdin_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.stdin_port
                )
                .as_str(),
            )
            .await?;
        log::trace!(
            "Connected to stdin socket on port {}",
            self.connection_file.stdin_port
        );

        Ok(())
    }

    pub async fn listen(&mut self) -> Result<(), anyhow::Error> {
        // Wait for a message from any socket
        loop {
            select! {
                shell_msg = self.shell_socket.recv() => {
                    match shell_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Shell, msg).await?;
                        },
                        Err(e) => {
                            log::error!("Failed to receive message from shell socket: {}", e);
                        },
                    }
                },
                iopub_msg = self.iopub_socket.recv() => {
                    match iopub_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::IOPub, msg).await?;
                        },
                        Err(e) => {
                            log::error!("Failed to receive message from iopub socket: {}", e);
                        },
                    }
                },
                control_msg = self.control_socket.recv() => {
                    match control_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Control, msg).await?;
                        },
                        Err(e) => {
                            log::error!("Failed to receive message from control socket: {}", e);
                        },
                    }
                },
                stdin_msg = self.stdin_socket.recv() => {
                    match stdin_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Stdin, msg).await?;
                        },
                        Err(e) => {
                            log::error!("Failed to receive message from iopub socket: {}", e);
                        },
                    }
                },
                ws_msg = self.ws_zmq_rx.recv() => {
                    match ws_msg {
                        Ok(msg) => {
                            self.forward_ws(msg).await?;
                            log::trace!("Received message from websocket");
                        }
                        Err(e) => {
                            log::error!("Failed to receive message from websocket: {}", e);
                        },
                    }
                }
            };
        }
    }

    async fn forward_ws(&mut self, msg: ZmqChannelMessage) -> Result<(), anyhow::Error> {
        match msg.channel {
            JupyterChannel::Shell => {
                log::trace!("Sending message to shell socket");
                self.shell_socket.send(msg.message).await?;
                log::trace!("Sent message to shell socket");
            }
            JupyterChannel::Control => {
                log::trace!("Sending message to control socket");
                // TODO: When an interrupt is sent, we need to clear the execution queue
                self.control_socket.send(msg.message).await?;
                log::trace!("Sent message to control socket");
            }
            JupyterChannel::Stdin => {
                log::trace!("Sending message to stdin socket");
                self.stdin_socket.send(msg.message).await?;
                log::trace!("Sent message to stdin socket");
            }
            _ => {
                log::error!("Unsupported channel: {:?}", msg.channel);
            }
        }
        Ok(())
    }

    /// Forward a message from a ZeroMQ socket to a WebSocket channel.
    ///
    /// - `channel`: The channel to forward the message to
    /// - `message`: The message to forward
    /// - `state`: The current state of the kernel
    /// - `ws_json_tx`: The channel to send JSON messages to the WebSocket
    async fn forward_zmq(
        &self,
        channel: JupyterChannel,
        message: ZmqMessage,
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
                    let mut state = self.state.write().await;
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
        match self.ws_json_tx.send(message).await {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow::anyhow!(
                "Failed to send message to websocket: {}",
                e
            )),
        }
    }
}
