//
// zmq_ws_proxy.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

use async_channel::{Receiver, Sender};
use event_listener::Event;
use futures::{stream, StreamExt};
use kallichore_api::models;
use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    kernel_message::KernelMessage,
    websocket_message::WebsocketMessage,
};
use std::{str::FromStr, sync::Arc};
use tokio::{select, sync::RwLock};
use zeromq::{
    util::PeerIdentity, DealerSocket, Socket, SocketEvent, SocketOptions, SocketRecv, SocketSend,
    SubSocket, ZmqMessage,
};

use crate::{
    connection_file::ConnectionFile,
    heartbeat::HeartbeatMonitor,
    jupyter_messages::{ExecutionState, JupyterMsg},
    kernel_connection::KernelConnection,
    kernel_session::make_message_id,
    kernel_state::KernelState,
    wire_message::WireMessage,
};

pub struct ZmqWsProxy {
    pub shell_socket: Option<DealerSocket>,
    pub iopub_socket: Option<SubSocket>,
    pub control_socket: Option<DealerSocket>,
    pub stdin_socket: Option<DealerSocket>,
    pub connection_file: ConnectionFile,
    pub connection: KernelConnection,
    pub heartbeat: HeartbeatMonitor,
    pub session_id: String,
    pub closed: bool,
    pub ws_json_tx: Sender<WebsocketMessage>,
    pub ws_zmq_rx: Receiver<JupyterMessage>,
    pub exit_event: Arc<Event>,
    pub disconnected_event: Arc<Event>,
    pub state: Arc<RwLock<KernelState>>,
    pub pending_iopub_messages: Vec<ZmqMessage>,
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
    /// - `exit_event`: An event listener that notifies the proxy when the
    ///    kernel has exited
    pub fn new(
        connection_file: ConnectionFile,
        connection: KernelConnection,
        state: Arc<RwLock<KernelState>>,
        ws_json_tx: Sender<WebsocketMessage>,
        ws_zmq_rx: Receiver<JupyterMessage>,
        exit_event: Arc<Event>,
    ) -> Self {
        let session_id = connection.session_id.clone();
        let disconnected_event = Arc::new(Event::new());

        Self {
            shell_socket: Some(DealerSocket::with_options(ZmqWsProxy::dealer_peer_opts(
                session_id.clone(),
            ))),
            iopub_socket: Some(SubSocket::new()),
            control_socket: Some(DealerSocket::with_options(ZmqWsProxy::dealer_peer_opts(
                session_id.clone(),
            ))),
            stdin_socket: Some(DealerSocket::with_options(ZmqWsProxy::dealer_peer_opts(
                session_id.clone(),
            ))),
            heartbeat: HeartbeatMonitor::new(
                state.clone(),
                session_id.clone(),
                format!(
                    "tcp://{}:{}",
                    connection_file.info.ip, connection_file.info.hb_port
                ),
                exit_event.clone(),
                disconnected_event.clone(),
            ),
            connection_file,
            connection,
            ws_json_tx,
            ws_zmq_rx,
            exit_event,
            disconnected_event,
            state,
            session_id: session_id.clone(),
            closed: false,
            pending_iopub_messages: Vec::new(),
        }
    }

    /// Creates the socket options for DEALER sockets to set the peer identity
    /// to the session ID.
    fn dealer_peer_opts(session_id: String) -> SocketOptions {
        let mut peer_opts = SocketOptions::default();
        let peer_id = PeerIdentity::from_str(session_id.as_str()).unwrap();
        peer_opts.peer_identity(peer_id);
        peer_opts
    }

    pub async fn connect(&mut self) -> Result<(), anyhow::Error> {
        // Ensure we're not closed before forwarding the message; this makes it
        // safe to unwrap the sockets below.
        if self.closed {
            anyhow::bail!("Cannot connect; proxy is closed.");
        }

        // Ensure we have a connection file before connecting
        log::trace!(
            "[session {}] Connecting to sockets on ip {} (shell = {}, iopub = {}, control = {}, stdin = {})",
            self.connection.session_id,
            self.connection_file.info.ip,
            self.connection_file.info.shell_port,
            self.connection_file.info.iopub_port,
            self.connection_file.info.control_port,
            self.connection_file.info.stdin_port,
        );

        self.shell_socket
            .as_mut()
            .unwrap()
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.info.ip, self.connection_file.info.shell_port
                )
                .as_str(),
            )
            .await?;

        log::trace!(
            "[session {}] Connected to shell socket on port {}",
            self.connection.session_id,
            self.connection_file.info.shell_port
        );

        self.iopub_socket
            .as_mut()
            .unwrap()
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.info.ip, self.connection_file.info.iopub_port
                )
                .as_str(),
            )
            .await?;
        log::trace!(
            "[session {}] Connected to iopub socket on port {}",
            self.connection.session_id,
            self.connection_file.info.iopub_port
        );

        // Subscribe to all messages
        self.iopub_socket.as_mut().unwrap().subscribe("").await?;

        self.control_socket
            .as_mut()
            .unwrap()
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.info.ip, self.connection_file.info.control_port
                )
                .as_str(),
            )
            .await?;
        log::trace!(
            "[session {}] Connected to control socket on port {}",
            self.connection.session_id,
            self.connection_file.info.control_port
        );

        self.stdin_socket
            .as_mut()
            .unwrap()
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.info.ip, self.connection_file.info.stdin_port
                )
                .as_str(),
            )
            .await?;
        log::trace!(
            "[session {}] Connected to stdin socket on port {}",
            self.connection.session_id,
            self.connection_file.info.stdin_port
        );

        // Sockets are connected; start the heartbeat monitor
        self.heartbeat.monitor();

        Ok(())
    }

    /// Gets the kernel info by sending a kernel_info_request message to the
    /// kernel and waiting for the reply. Returns the kernel info as a JSON
    /// object.
    pub async fn get_kernel_info(&mut self) -> Result<serde_json::Value, anyhow::Error> {
        // Create a random message ID for the kernel info request
        let msg_id = make_message_id();

        // Form the kernel_info_request message
        let request = JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: msg_id.clone(),
                msg_type: "kernel_info_request".to_string(),
            },
            parent_header: None,
            channel: JupyterChannel::Shell,
            content: serde_json::json!({}),
            metadata: serde_json::json!({}),
            buffers: vec![],
        };

        // Translate it into a wire message and send it to the shell socket
        log::trace!(
            "[session {}] Sending initial kernel_info_request message to kernel",
            self.connection.session_id
        );

        // Use the protocol version for the initial message
        let wire_message = WireMessage::from_jupyter(request, self.connection.clone())?;
        let zmq_message: ZmqMessage = wire_message.into();
        self.shell_socket
            .as_mut()
            .unwrap()
            .send(zmq_message)
            .await?;
        log::trace!(
            "[session {}] Waiting for kernel_info_reply message from kernel",
            self.connection.session_id
        );
        // Wait for the reply
        let reply = self.wait_for_shell_reply(msg_id.clone()).await?;
        log::trace!(
            "[session {}] Got kernel_info_reply message from kernel",
            self.connection.session_id
        );

        Ok(reply.content)
    }

    async fn wait_for_shell_reply(
        &mut self,
        msg_id: String,
    ) -> Result<JupyterMessage, anyhow::Error> {
        let session_id = self.connection.session_id.clone();
        loop {
            select! {
                shell_msg = self.shell_socket.as_mut().unwrap().recv() => {
                    match shell_msg {
                        Ok(msg) => {
                            let wire_message = WireMessage::from_zmq(self.session_id.clone(), JupyterChannel::Shell, msg);
                            let jupyter_message = wire_message.to_jupyter(JupyterChannel::Shell)?;
                            let parent = match jupyter_message.parent_header {
                                None => {
                                    log::warn!("[session {}] Discarding message with no parent header: {}", session_id, jupyter_message.header.msg_id);
                                    continue;
                                },
                                Some(ref parent_header) => parent_header,
                            };
                            if parent.msg_id == msg_id {
                                return Ok(jupyter_message);
                            } else {
                                log::warn!("[session {}] Discarding message with unexpected parent msg_id: {}", session_id, jupyter_message.header.msg_id);
                            }
                        },
                        Err(e) => {
                            return Err(anyhow::anyhow!("Failed to receive message from shell socket: {}", e));
                        },
                    }
                },
                // While waiting for the kernel info reply, we need to discard
                // the iopub busy/idle status messages that are emitted while
                // processing the kernel info request; since the client did not
                // initiate the request and can't see the reply, it may be
                // confused by the status messages. All other outgoing iopub
                // messages are queued and delivered later.
                iopub_msg = self.iopub_socket.as_mut().unwrap().recv() => {
                    match iopub_msg {
                        Ok(msg) => {
                            // Parse into a Jupyter message
                            let wire_message = WireMessage::from_zmq(self.session_id.clone(), JupyterChannel::IOPub, msg.clone());
                            let jupyter_message = wire_message.to_jupyter(JupyterChannel::IOPub)?;
                            if let Some(ref parent_header) =  jupyter_message.parent_header {
                                // If the message's parent header matches the msg_id we
                                // are waiting for, we need to check if it is a status
                                // message. If it is, we need to discard it.
                                if parent_header.msg_id == msg_id {
                                    if let JupyterMsg::Status(_) = JupyterMsg::from(jupyter_message.clone()) {
                                            log::trace!(
                                                "[session {}] Received kernel status message",
                                                session_id,
                                            );
                                            continue;
                                        }
                                }
                            };

                            // If we got here, save the message for later delivery.
                            self.pending_iopub_messages.push(msg);
                        },
                        Err(e) => {
                            return Err(anyhow::anyhow!("Failed to receive message from iopub socket: {}", e));
                        }
                    }
                }
            }
        }
    }

    pub async fn listen(&mut self) -> Result<(), anyhow::Error> {
        let session_id = self.connection.session_id.clone();
        log::debug!(
            "[session {}] Starting ZeroMQ-WebSocket proxy",
            self.connection.session_id
        );

        // Start by flushing the queue of pending iopub messages and sending
        // them to the WebSocket.
        let pending_messages = self.pending_iopub_messages.drain(..).collect::<Vec<_>>();
        if !pending_messages.is_empty() {
            log::debug!(
                "[session {}] Forwarding {} pending iopub messages to WebSocket",
                session_id,
                pending_messages.len()
            );
        }
        for message in pending_messages {
            if let Err(err) = self.forward_zmq(JupyterChannel::IOPub, message).await {
                log::error!(
                    "[session {}] Failed to forward pending iopub message to WebSocket: {}",
                    session_id,
                    err
                );
            }
        }

        // Create monitors for each socket.
        //
        // NOTE: This is currently just defensive programming; the underlying library zmq.rs does
        // not actually report Closed socket events as of 0.4.1.
        let shell_monitor = self.shell_socket.as_mut().unwrap().monitor();
        let iopub_monitor = self.iopub_socket.as_mut().unwrap().monitor();
        let control_monitor = self.control_socket.as_mut().unwrap().monitor();
        let stdin_monitor = self.stdin_socket.as_mut().unwrap().monitor();

        // Combine the streams into a single stream that we can select from
        let all_monitors = vec![shell_monitor, iopub_monitor, control_monitor, stdin_monitor];
        let mut sockets_connected = all_monitors.len();
        let mut combined_stream = stream::select_all(all_monitors);

        // Wait for a message from any socket, or for all sockets to close
        loop {
            select! {
                evt = combined_stream.next() => {
                    log::trace!("[session {}] Received event from socket monitor {:?}", session_id, evt);
                    match evt {
                        Some(SocketEvent::Closed) => {
                            sockets_connected -= 1;
                            log::info!("[session {}] Socket closed ({} remaining)", session_id, sockets_connected);
                        },
                        _ => {
                            // Ignore other events
                        }
                    }
                    if sockets_connected == 0 {
                        // When all sockets are closed, exit the loop, ending the proxy
                        break;
                    }
                },
                shell_msg = self.shell_socket.as_mut().unwrap().recv() => {
                    match shell_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Shell, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from shell socket: {}", session_id, e);
                            break;
                        },
                    }
                },
                iopub_msg = self.iopub_socket.as_mut().unwrap().recv() => {
                    match iopub_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::IOPub, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from iopub socket: {}", session_id, e);
                            break;
                        },
                    }
                },
                control_msg = self.control_socket.as_mut().unwrap().recv() => {
                    match control_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Control, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from control socket: {}", session_id, e);
                            break;
                        },
                    }
                },
                stdin_msg = self.stdin_socket.as_mut().unwrap().recv() => {
                    match stdin_msg {
                        Ok(msg) => {
                            self.forward_zmq(JupyterChannel::Stdin, msg).await?;
                        },
                        Err(e) => {
                            log::error!("[session {}] Failed to receive message from stdin socket: {}", session_id, e);
                            break;
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
                            break;
                        },
                    }
                },
                _ = self.exit_event.listen() => {
                    // The kernel has exited; close the proxy
                    break;
                },
                _ = self.disconnected_event.listen() => {
                    // The heartbeat monitor has detected a disconnect; close the proxy
                    break;
                },
            };
        }
        log::debug!(
            "[session {}] Ending ZeroMQ-WebSocket proxy",
            self.connection.session_id
        );

        // Close the sockets. This consumes the socket, so we need to take() it.
        self.closed = true;
        self.shell_socket.take().unwrap().close().await;
        self.iopub_socket.take().unwrap().close().await;
        self.control_socket.take().unwrap().close().await;
        self.stdin_socket.take().unwrap().close().await;

        Ok(())
    }

    async fn forward_ws(&mut self, msg: JupyterMessage) -> Result<(), anyhow::Error> {
        // Ensure we're not closed before forwarding the message; this makes it
        // safe to unwrap the sockets below.
        if self.closed {
            anyhow::bail!("Cannot forward WebSocket message; proxy is closed.");
        }

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

        // Convert the message to a wire message using the protocol version
        let channel = msg.channel.clone();
        let wire_message = WireMessage::from_jupyter(msg, self.connection.clone())?;

        let zmq_message: ZmqMessage = wire_message.into();
        match channel {
            JupyterChannel::Shell => {
                log::trace!("Sending message to shell socket");
                self.shell_socket
                    .as_mut()
                    .unwrap()
                    .send(zmq_message)
                    .await?;
                log::trace!("Sent message to shell socket");
            }
            JupyterChannel::Control => {
                log::trace!("Sending message to control socket");
                self.control_socket
                    .as_mut()
                    .unwrap()
                    .send(zmq_message)
                    .await?;
                log::trace!("Sent message to control socket");
            }
            JupyterChannel::Stdin => {
                log::trace!("Sending message to stdin socket");
                self.stdin_socket
                    .as_mut()
                    .unwrap()
                    .send(zmq_message)
                    .await?;
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
        // Ensure we're not closed before forwarding the message; this makes it
        // safe to unwrap the sockets below.
        if self.closed {
            anyhow::bail!("Cannot forward ZMQ message; proxy is closed.");
        }

        // (1) convert the raw parts/frames of the message into a `WireMessage`.
        let message = WireMessage::from_zmq(self.session_id.clone(), channel, message);

        // (2) convert it into a Jupyter message; this can fail if the message is
        // not a valid Jupyter message, in which case we will discard it.
        let message = match message.to_jupyter(channel) {
            Ok(msg) => msg,
            Err(e) => {
                log::warn!(
                    "[session {}] Discarding invalid message from 0MQ on '{:?}': {}",
                    self.session_id,
                    channel,
                    e,
                );
                return Ok(());
            }
        };

        log::debug!(
            "[session {}] Forward from '{:?}' to WebSocket: {}",
            self.session_id,
            channel,
            message.header.msg_type
        );

        let jupyter = JupyterMsg::from(message.clone());
        match jupyter {
            JupyterMsg::Status(status) => {
                // Write the new status to the kernel state
                let state = {
                    let mut state = self.state.write().await;
                    // Deliver the status update with the parent message type's
                    // name if known
                    let reason = match message.parent_header {
                        Some(ref parent) => Some(parent.msg_type.clone()),
                        None => None,
                    };
                    match status.execution_state {
                        ExecutionState::Busy => {
                            state.set_status(models::Status::Busy, reason).await;
                        }
                        ExecutionState::Idle => {
                            state.set_status(models::Status::Idle, reason).await;
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
                                // Send the next request to the kernel with the protocol version
                                let wire_message =
                                    WireMessage::from_jupyter(request, self.connection.clone())?;

                                self.shell_socket
                                    .as_mut()
                                    .unwrap()
                                    .send(wire_message.into())
                                    .await?;
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
