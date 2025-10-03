//
// heartbeat.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

use std::sync::Arc;
use std::usize;

use kallichore_api::models::Status;
use tokio::sync::RwLock;
use zeromq::ReqSocket;
use zeromq::Socket;
use zeromq::SocketRecv;
use zeromq::SocketSend;

use crate::kernel_state::KernelState;
use event_listener::Event;
use tokio::time::{timeout, Duration};

pub struct HeartbeatMonitor {
    state: Arc<RwLock<KernelState>>,
    session_id: String,
    address: String,
    exit_event: Arc<Event>,
    disconnected_event: Arc<Event>,
}

const HB_PAYLOAD: &str = "kallichore-heartbeat";

/// A heartbeat monitor for a kernel session.
impl HeartbeatMonitor {
    /// Create a new heartbeat monitor.
    ///
    /// # Arguments
    ///
    /// - `state`: The kernel state to monitor.
    /// - `session_id`: The ID of the session to monitor.
    /// - `address`: The address of the heartbeat socket.
    pub fn new(
        state: Arc<RwLock<KernelState>>,
        session_id: String,
        address: String,
        exit_event: Arc<Event>,
        disconnected_event: Arc<Event>,
    ) -> Self {
        Self {
            state,
            session_id,
            address,
            exit_event,
            disconnected_event,
        }
    }

    /// Monitor the kernel's heartbeat. Returns immediately and runs the monitor
    /// job in the background.
    pub fn monitor(&self) {
        let addr = self.address.clone();
        let state = self.state.clone();
        let session_id = self.session_id.clone();
        let exit_event = self.exit_event.clone();
        let disconnected_event = self.disconnected_event.clone();

        tokio::spawn(async move {
            let mut hb_socket = match Self::connect_heartbeat_socket(&addr, &session_id).await {
                Some(socket) => socket,
                None => return,
            };

            let mut offline = false;
            let mut initial = true;

            loop {
                // Check if we should stop the monitor
                if Self::should_stop_monitor(&state, &session_id).await {
                    return;
                }

                // Send heartbeat
                log::trace!("[session {}] Sending heartbeat to kernel.", session_id);
                hb_socket.send(HB_PAYLOAD.into()).await.unwrap();

                // Handle heartbeat response or exit event
                let should_continue = Self::handle_heartbeat_response(
                    &mut hb_socket,
                    &exit_event,
                    &state,
                    &disconnected_event,
                    &session_id,
                    &mut offline,
                    &mut initial,
                )
                .await;

                if !should_continue {
                    hb_socket.close().await;
                    return;
                }

                // Wait before next heartbeat or exit if signaled
                if Self::wait_or_exit(&exit_event, &session_id, &mut hb_socket).await {
                    return;
                }
            }
        });
    }

    /// Connect to the heartbeat socket
    async fn connect_heartbeat_socket(addr: &str, session_id: &str) -> Option<ReqSocket> {
        let mut hb_socket = ReqSocket::new();
        match hb_socket.connect(addr).await {
            Err(err) => {
                log::error!(
                    "[session {}] Failed to connect to heartbeat socket: {}.",
                    session_id,
                    err
                );
                None
            }
            Ok(_) => {
                log::info!(
                    "[session {}] Connected to heartbeat socket at {}.",
                    session_id,
                    addr
                );
                Some(hb_socket)
            }
        }
    }

    /// Check if the monitor should stop based on kernel state
    async fn should_stop_monitor(state: &Arc<RwLock<KernelState>>, session_id: &str) -> bool {
        let current_state = state.read().await;
        if current_state.status == Status::Exited {
            log::debug!(
                "[session {}] Stopping heartbeat monitor (kernel exited).",
                session_id
            );
            true
        } else {
            false
        }
    }

    /// Handle the heartbeat response or exit event
    async fn handle_heartbeat_response(
        hb_socket: &mut ReqSocket,
        exit_event: &Arc<Event>,
        state: &Arc<RwLock<KernelState>>,
        disconnected_event: &Arc<Event>,
        session_id: &str,
        offline: &mut bool,
        initial: &mut bool,
    ) -> bool {
        let exit_listener = exit_event.listen();
        tokio::select! {
            _ = exit_listener => {
                log::debug!(
                    "[session {}] Stopping heartbeat monitor (exit event signaled).",
                    session_id
                );
                false
            }
            result = timeout(Duration::from_secs(5), hb_socket.recv()) => {
                Self::process_heartbeat_result(
                    result,
                    state,
                    disconnected_event,
                    session_id,
                    offline,
                    initial,
                ).await
            }
        }
    }

    /// Process the result of a heartbeat attempt
    async fn process_heartbeat_result(
        result: Result<Result<zeromq::ZmqMessage, zeromq::ZmqError>, tokio::time::error::Elapsed>,
        state: &Arc<RwLock<KernelState>>,
        disconnected_event: &Arc<Event>,
        session_id: &str,
        offline: &mut bool,
        initial: &mut bool,
    ) -> bool {
        match result {
            Ok(Ok(response)) => {
                Self::handle_heartbeat_success(response, state, session_id, offline, initial).await;
                true
            }
            Ok(Err(e)) => {
                Self::handle_heartbeat_error(e, state, disconnected_event, session_id).await
            }
            Err(_) => {
                Self::handle_heartbeat_timeout(state, session_id, offline).await;
                true
            }
        }
    }

    /// Handle successful heartbeat response
    async fn handle_heartbeat_success(
        response: zeromq::ZmqMessage,
        state: &Arc<RwLock<KernelState>>,
        session_id: &str,
        offline: &mut bool,
        initial: &mut bool,
    ) {
        log::trace!(
            "[session {}] Got heartbeat response: {:?}",
            session_id,
            response
        );

        // If the kernel was offline, mark it as online
        if *offline {
            *offline = false;
            log::trace!(
                "[session {}] Kernel was offline; marking it online.",
                session_id
            );
            let mut state = state.write().await;
            state
                .set_status(
                    Status::Idle,
                    Some(String::from("heartbeat detected after offline")),
                )
                .await;
        }

        // If this is the first heartbeat, log that
        if *initial {
            *initial = false;
            log::info!(
                "[session {}] Received initial heartbeat from kernel",
                session_id
            );
        }
    }

    /// Handle heartbeat error
    async fn handle_heartbeat_error(
        e: zeromq::ZmqError,
        state: &Arc<RwLock<KernelState>>,
        disconnected_event: &Arc<Event>,
        session_id: &str,
    ) -> bool {
        let current = state.read().await;

        if current.status == Status::Exited {
            log::debug!(
                "[session {}] Stopping heartbeat monitor (kernel exited).",
                session_id
            );
            false
        } else {
            log::info!(
                "[session {}] Error receiving heartbeat response: {:?} (kernel is {}). Marking kernel disconnected.",
                session_id,
                e,
                current.status
            );
            disconnected_event.notify(usize::MAX);
            false
        }
    }

    /// Handle heartbeat timeout
    async fn handle_heartbeat_timeout(
        state: &Arc<RwLock<KernelState>>,
        session_id: &str,
        offline: &mut bool,
    ) {
        if !*offline {
            *offline = true;
            log::error!(
                "[session {}] No heartbeat response received after 5s, marking kernel as offline.",
                session_id
            );
            let mut state = state.write().await;
            state
                .set_status(Status::Offline, Some(String::from("lost heartbeat")))
                .await;
        }
    }

    /// Wait for the next heartbeat interval or exit if signaled
    async fn wait_or_exit(
        exit_event: &Arc<Event>,
        session_id: &str,
        _hb_socket: &mut ReqSocket,
    ) -> bool {
        let exit_listener = exit_event.listen();
        tokio::select! {
            _ = exit_listener => {
                log::debug!(
                    "[session {}] Stopping heartbeat monitor (exit event signaled).",
                    session_id
                );
                true
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => false
        }
    }
}
