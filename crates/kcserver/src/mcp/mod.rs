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
//!
//! One supervisor can be shared by every window of a Positron server, so the
//! single listener gives each registered frontend an endpoint of its own at
//! `/mcp/w/<frontend_id>`, with its own token, its own protocol sessions, and a
//! view restricted to that frontend's sessions and commands.

pub mod auth;
pub mod channel;
pub mod frontends;
pub mod handler;
pub mod listener;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kallichore_api::models;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::kernel_session::KernelSession;
use frontends::FrontendRegistry;
use handler::PositronMcpHandler;

/// One frontend's MCP endpoint.
type FrontendService = StreamableHttpService<PositronMcpHandler, LocalSessionManager>;

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

    /// The HTTP service behind each frontend's endpoint, built on first use.
    services: tokio::sync::Mutex<HashMap<String, FrontendService>>,
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
            services: tokio::sync::Mutex::new(HashMap::new()),
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
            // One listener serves every frontend, so a later registration's
            // preferred port cannot be honoured. Say so rather than leaving the
            // frontend to wonder why its setting did nothing.
            if let Some(preferred) = preferred_port.filter(|p| *p != 0 && *p != existing.port) {
                log::info!(
                    "Ignoring preferred MCP port {}; the listener is already on {}",
                    preferred,
                    existing.port
                );
            }
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
            self.services.lock().await.clear();
            self.request_count.store(0, Ordering::Relaxed);
            log::info!("MCP listener on 127.0.0.1:{} stopped", handle.port);
        }
    }

    /// The HTTP service serving a frontend's endpoint.
    ///
    /// Each frontend gets its own service, so the handler knows which window it
    /// is answering for without inspecting every request, and protocol sessions
    /// belong to one window and go away with it.
    ///
    /// Returns None when the listener is not running.
    pub async fn service_for(self: &Arc<Self>, frontend_id: &str) -> Option<FrontendService> {
        let mut services = self.services.lock().await;
        if let Some(existing) = services.get(frontend_id) {
            return Some(existing.clone());
        }

        let cancel = self.listener.lock().await.as_ref()?.cancel.child_token();
        let state = self.clone();
        let id = frontend_id.to_string();
        let service = StreamableHttpService::new(
            move || Ok(PositronMcpHandler::new(state.clone(), id.clone())),
            Arc::new(LocalSessionManager::default()),
            // Sessions are kept for pre-2026-07-28 clients. Those clients
            // report who they are only in the initialize handshake, and the
            // agent's name is what attributes executions in the user's console.
            StreamableHttpServerConfig::default().with_cancellation_token(cancel),
        );
        services.insert(frontend_id.to_string(), service.clone());
        Some(service)
    }

    /// Drop a frontend's endpoint, ending its agents' protocol sessions.
    pub async fn drop_service(&self, frontend_id: &str) {
        self.services.lock().await.remove(frontend_id);
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
