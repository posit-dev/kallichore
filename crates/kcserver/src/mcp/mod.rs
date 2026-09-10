//
// mod.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! The Model Context Protocol server hosted inside the supervisor.
//!
//! External coding agents reach the user's live sessions through a loopback
//! Streamable HTTP endpoint served on a listener separate from the
//! supervisor's main API transport. The listener starts when the first
//! Positron frontend registers and stops when the last one deregisters, so no
//! TCP port is open unless someone asked for it.

pub mod auth;
pub mod channel;
pub mod frontends;
pub mod handler;
pub mod listener;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kallichore_api::models;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::kernel_session::KernelSession;
use frontends::FrontendRegistry;

/// The frontend whose token authorized an MCP request. Attached to the HTTP
/// request before it reaches the protocol layer, and read back by the tools to
/// scope themselves to one window.
#[derive(Clone, Debug)]
pub struct FrontendId(pub String);

/// A running MCP listener.
struct ListenerHandle {
    port: u16,
    cancel: CancellationToken,
}

/// Everything the MCP server needs from the supervisor.
pub struct McpState {
    /// The registered frontends and their tokens.
    pub registry: Arc<FrontendRegistry>,

    /// The supervisor's kernel sessions.
    kernel_sessions: Arc<std::sync::RwLock<Vec<KernelSession>>>,

    /// Nudged on every tool call so an active agent postpones idle shutdown.
    idle_nudge_tx: mpsc::Sender<Option<u32>>,

    /// Tool calls served since the listener started.
    request_count: AtomicU64,

    /// The listener, when one is running. Held across the bind so two
    /// registrations racing cannot each open a port.
    listener: tokio::sync::Mutex<Option<ListenerHandle>>,
}

impl McpState {
    /// Create the MCP state for a supervisor.
    pub fn new(
        kernel_sessions: Arc<std::sync::RwLock<Vec<KernelSession>>>,
        idle_nudge_tx: mpsc::Sender<Option<u32>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry: Arc::new(FrontendRegistry::new()),
            kernel_sessions,
            idle_nudge_tx,
            request_count: AtomicU64::new(0),
            listener: tokio::sync::Mutex::new(None),
        })
    }

    /// Start the listener if it isn't already running, and return its port.
    ///
    /// `preferred_port` keeps an agent's configured URL valid across supervisor
    /// restarts; when it is taken, the OS assigns one instead.
    pub async fn ensure_listener(
        self: &Arc<Self>,
        preferred_port: Option<u16>,
    ) -> std::io::Result<u16> {
        let mut guard = self.listener.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.port);
        }

        let (tcp_listener, port) = listener::bind(preferred_port).await?;
        let cancel = CancellationToken::new();
        listener::serve(tcp_listener, self.clone(), cancel.clone());
        *guard = Some(ListenerHandle { port, cancel });
        log::info!("MCP listener started on 127.0.0.1:{}", port);
        Ok(port)
    }

    /// Stop the listener and release its port.
    pub async fn stop_listener(&self) {
        let handle = self.listener.lock().await.take();
        if let Some(handle) = handle {
            handle.cancel.cancel();
            self.request_count.store(0, Ordering::Relaxed);
            log::info!("MCP listener on 127.0.0.1:{} stopped", handle.port);
        }
    }

    /// The port the listener is bound to, if it is running.
    pub async fn port(&self) -> Option<u16> {
        self.listener.lock().await.as_ref().map(|l| l.port)
    }

    /// Record a tool call and keep the supervisor's idle timer at bay.
    pub fn note_request(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = self.idle_nudge_tx.try_send(None) {
            log::trace!("Failed to nudge idle timer for MCP request: {}", e);
        }
    }

    /// A snapshot of the supervisor's kernel sessions.
    pub fn sessions(&self) -> Vec<KernelSession> {
        self.kernel_sessions.read().unwrap().clone()
    }

    /// The supervisor's version, reported to agents alongside session lists.
    pub fn server_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Summarize the MCP server for the status endpoint.
    pub async fn status(&self) -> models::McpStatus {
        let port = self.port().await;
        models::McpStatus {
            active: port.is_some(),
            port: port.unwrap_or(0) as i32,
            request_count: self.request_count.load(Ordering::Relaxed) as i32,
            frontends: self.registry.status().await,
        }
    }
}
