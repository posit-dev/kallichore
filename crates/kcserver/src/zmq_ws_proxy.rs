//
// zmq_ws_proxy.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::sync::Arc;

use async_channel::{Receiver, Sender};
use kallichore_api::models;
use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage},
    kernel_message::KernelMessage,
    websocket_message::WebsocketMessage,
};
use tokio::{select, sync::RwLock};
use zeromq::{DealerSocket, Socket, SocketRecv, SocketSend, SubSocket, ZmqMessage};

use crate::{
    connection_file::ConnectionFile,
    heartbeat::HeartbeatMonitor,
    jupyter_messages::{ExecutionState, JupyterMsg},
    kernel_connection::KernelConnection,
    kernel_state::KernelState,
    wire_message::WireMessage,
};

pub struct ZmqWsProxy {
    pub shell_socket: DealerSocket,
    pub iopub_socket: SubSocket,
    pub control_socket: DealerSocket,
    pub stdin_socket: DealerSocket,
    pub connection_file: ConnectionFile,
    pub connection: KernelConnection,
    pub heartbeat: HeartbeatMonitor,
    pub ws_json_tx: Sender<WebsocketMessage>,
    pub ws_zmq_rx: Receiver<JupyterMessage>,
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
    /// - `connection`: The connection information for the kernel
    /// - `state`: The current state of the kernel
    /// - `ws_json_tx`: A channel to send JSON messages to the WebSocket
    /// - `ws_zmq_rx`: A channel to receive messages from the WebSocket
    pub fn new(
        connection_file: ConnectionFile,
        connection: KernelConnection,
        state: Arc<RwLock<KernelState>>,
        ws_json_tx: Sender<WebsocketMessage>,
        ws_zmq_rx: Receiver<JupyterMessage>,
    ) -> Self {
        let session_id = connection.session_id.clone();
        let hb_address = format!("tcp://{}:{}", connection_file.ip, connection_file.hb_port);
        Self {
            shell_socket: DealerSocket::new(),
            iopub_socket: SubSocket::new(),
            control_socket: DealerSocket::new(),
            stdin_socket: DealerSocket::new(),
            heartbeat: HeartbeatMonitor::new(state.clone(), session_id, hb_address),
            connection_file,
            connection,
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
            "[session {}] Connected to shell socket on port {}",
            self.connection.session_id,
            self.connection_file.shell_port
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
            "[session {}] Connected to iopub socket on port {}",
            self.connection.session_id,
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
            "[session {}] Connected to control socket on port {}",
            self.connection.session_id,
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
            "[session {}] Connected to stdin socket on port {}",
            self.connection.session_id,
            self.connection_file.stdin_port
        );

        // Sockets are connected; start the heartbeat monitor
        self.heartbeat.monitor();

        Ok(())
    }

    pub async fn listen(&mut self) -> Result<(), anyhow::Error> {
        let session_id = self.connection.session_id.clone();
        // Wait for a message from any socket
        loop {
            select! {
                shell_msg = self.shell_socket.recv() => {
                    match shell_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Shell, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from shell socket: {}", session_id, e);
                        },
                    }
                },
                iopub_msg = self.iopub_socket.recv() => {
                    match iopub_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::IOPub, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from iopub socket: {}", session_id, e);
                        },
                    }
                },
                control_msg = self.control_socket.recv() => {
                    match control_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Control, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from control socket: {}", session_id, e);
                        },
                    }
                },
                stdin_msg = self.stdin_socket.recv() => {
                    match stdin_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Stdin, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from iopub socket: {}", session_id, e);
                        },
                    }
                },
                ws_msg = self.ws_zmq_rx.recv() => {
                    match ws_msg {
                        Ok(msg) => {
                            self.forward_ws(msg).await?;
                            log::trace!("[session {}] Received message from websocket", session_id);
                        }
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from websocket: {}", session_id, e);
                        },
                    }
                }
            };
        }
    }

    async fn forward_ws(&mut self, msg: JupyterMessage) -> Result<(), anyhow::Error> {
        let jupyter = JupyterMsg::from(msg.clone());
        match jupyter {
            JupyterMsg::ExecuteRequest(_) => {
                // Queue the message for execution
                let mut state = self.state.write().await;
                if !state.execution_queue.process_request(msg.clone()) {
                    // The request was queued for later execution and should not
                    // be processed right now; don't deliver it to the socket.
                    // Instead, just tell the client that we have queued the
                    // request.
                    let queued = KernelMessage::ExecutionQueued(msg.header.msg_id.clone());
                    self.ws_json_tx
                        .send(WebsocketMessage::Kernel(queued))
                        .await?;
                    return Ok(());
                }
            }
            JupyterMsg::InterruptRequest => {
                // Clear the execution queue; an interrupt should cancel any
                // pending requests
                log::debug!(
                    "[session {}] Interrupting kernel",
                    self.connection.session_id
                );
                let mut state = self.state.write().await;
                state.execution_queue.clear();
            }
            JupyterMsg::ShutdownRequest => {
                // Clear the execution queue and shut down the kernel
                log::debug!(
                    "[session {}] Shutting down kernel",
                    self.connection.session_id
                );
                let mut state = self.state.write().await;
                state.execution_queue.clear();
            }
            _ => {
                // Do nothing for other message types
            }
        }
        // Convert the message to a wire message
        let channel = msg.channel.clone();
        let wire_message = WireMessage::from_jupyter(msg, self.connection.clone())?;
        let zmq_message: ZmqMessage = wire_message.into();
        match channel {
            JupyterChannel::Shell => {
                log::trace!("Sending message to shell socket");
                self.shell_socket.send(zmq_message).await?;
                log::trace!("Sent message to shell socket");
            }
            JupyterChannel::Control => {
                log::trace!("Sending message to control socket");
                self.control_socket.send(zmq_message).await?;
                log::trace!("Sent message to control socket");
            }
            JupyterChannel::Stdin => {
                log::trace!("Sending message to stdin socket");
                self.stdin_socket.send(zmq_message).await?;
                log::trace!("Sent message to stdin socket");
            }
            _ => {
                log::error!("Unsupported channel: {:?}", channel);
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
        &mut self,
        channel: JupyterChannel,
        message: ZmqMessage,
    ) -> Result<(), anyhow::Error> {
        // (1) convert the raw parts/frames of the message into a `WireMessage`.
        let message = WireMessage::from(message);

        // (2) convert it into a Jupyter message; this can fail if the message is
        // not a valid Jupyter message.
        let message = message.to_jupyter(channel)?;

        let jupyter = JupyterMsg::from(message.clone());
        match jupyter {
            JupyterMsg::Status(status) => {
                // Write the new status to the kernel state
                let state = {
                    let mut state = self.state.write().await;
                    match status.execution_state {
                        ExecutionState::Busy => {
                            state.set_status(models::Status::Busy).await;
                        }
                        ExecutionState::Idle => {
                            state.set_status(models::Status::Idle).await;
                        }
                    };
                    status.execution_state
                };
                // If the kernel is now idle, process the next message in the queue
                match state {
                    ExecutionState::Idle => {
                        let mut state = self.state.write().await;
                        match state.execution_queue.next_request() {
                            Some(request) => {
                                // Send the next request to the kernel
                                let message =
                                    WireMessage::from_jupyter(request, self.connection.clone())?;
                                self.shell_socket.send(message.into()).await?;
                            }
                            None => {
                                // No more messages in the queue
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                // Do nothing for other message types (let the message pass through)
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
