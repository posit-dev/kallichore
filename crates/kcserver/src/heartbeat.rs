//
// heartbeat.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::sync::Arc;

use kallichore_api::models::Status;
use tokio::sync::RwLock;
use zeromq::ReqSocket;
use zeromq::Socket;
use zeromq::SocketRecv;
use zeromq::SocketSend;

use crate::kernel_state::KernelState;
use tokio::time::{timeout, Duration};

pub struct HeartbeatMonitor {
    state: Arc<RwLock<KernelState>>,
    session_id: String,
    address: String,
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
    pub fn new(state: Arc<RwLock<KernelState>>, session_id: String, address: String) -> Self {
        Self {
            state,
            session_id,
            address,
        }
    }

    /// Monitor the kernel's heartbeat. Returns immediately and runs the monitor
    /// job in the background.
    pub fn monitor(&self) {
        let addr = self.address.clone();
        let state = self.state.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            // Attempt to connect to the heartbeat socket. If we fail to connect, we won't be able to
            // send heartbeats, so we'll just return; the kernel can still function in this case,
            // but it won't be able to detect offline states.
            let mut hb_socket = ReqSocket::new();
            match hb_socket.connect(addr.as_str()).await {
                Err(err) => {
                    log::error!(
                        "[session {}] Failed to connect to heartbeat socket: {}.",
                        session_id,
                        err
                    );
                    return;
                }
                Ok(_) => {
                    log::info!(
                        "[session {}] Connected to heartbeat socket at {}.",
                        session_id,
                        addr
                    );
                }
            }
            let mut offline = false;
            loop {
                // Send the heartbeat payload to the server
                log::trace!("[session {}] Sending heartbeat to kernel.", session_id);
                hb_socket.send(HB_PAYLOAD.into()).await.unwrap();

                // Wait up to 5s for a response
                match timeout(Duration::from_secs(5), hb_socket.recv()).await {
                    Ok(Ok(response)) => {
                        // Got a heartbeat response
                        log::trace!(
                            "[session {}] Got heartbeat response: {:?}",
                            session_id,
                            response
                        );
                        if offline {
                            offline = false;
                            log::trace!(
                                "[session {}] Kernel was offline; marking it online.",
                                session_id
                            );
                            let mut state = state.write().await;
                            // CONSIDER: we don't actually know that the kernel
                            // is idle, just that it's back online. Should we
                            // instead cache the previous status and restore it?
                            state.set_status(Status::Idle).await;
                        }
                        response
                    }
                    Ok(Err(e)) => {
                        // We couldn't receive the heartbeat response
                        let state = state.read().await;

                        if state.status == Status::Exited {
                            // If the kernel has exited, it's normal for that to
                            // cause a receive error (the other end of the
                            // socket is gone). We can just stop the heartbeat
                            // monitor.
                            log::trace!(
                                "[session {}] Stopping heartbeat monitor (kernel exited).",
                                session_id
                            );
                            return;
                        }

                        log::error!(
                            "[session {}] Error receiving heartbeat response: {:?} (kernel is {})",
                            session_id,
                            e,
                            state.status
                        );
                        return;
                    }
                    Err(_) => {
                        // Handle the timeout error
                        log::error!(
                            "[session {}] No heartbeat response received after 5s, marking kernel as offline.", session_id
                        );
                        let mut state = state.write().await;
                        state.set_status(Status::Offline).await;
                        break;
                    }
                };

                // Wait 2s before sending the next heartbeat
                tokio::time::sleep(Duration::from_secs(2)).await;

                // Check to see if the kernel is still running; if not, stop the monitor
                let state = state.read().await;
                if state.status == Status::Exited {
                    log::trace!(
                        "[session {}] Stopping heartbeat monitor (kernel exited).",
                        session_id
                    );
                    return;
                }
            }
        });
    }
}
