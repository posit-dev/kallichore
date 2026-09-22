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
//! Positron workspace registers and stops when the last one deregisters, so no
//! TCP port is open unless someone asked for it.
//!
//! One supervisor can be shared by every window of a Positron server, so the
//! single listener gives each registered workspace an endpoint of its own at
//! `/mcp/w/<workspace_id>`, with its own token, its own protocol sessions, and
//! a view restricted to that workspace's sessions and commands. Each endpoint
//! describes itself at `/mcp/w/<workspace_id>/server-card`.
//!
//! The supervisor publishes a workspace's endpoint into the environment of the
//! kernels it starts for that workspace, because an MCP client is as likely to
//! be a library the user loaded in their console as a coding agent in a
//! terminal. Such a client is handed an endpoint that names its own session, so
//! it gets its own handler and the server can tell it apart from an agent
//! outside; see [`listener::session_endpoint_url`].
//!
//! Clients that would rather start a server than connect to one run
//! `kcserver mcp-stdio`, which finds the right workspace's endpoint and relays
//! to it; see [`stdio_bridge`].

pub mod auth;
pub mod card;
pub mod channel;
pub mod handler;
pub mod listener;
pub mod stdio_bridge;
pub mod workspaces;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kallichore_api::models;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::kernel_session::KernelSession;
use handler::PositronMcpHandler;
use workspaces::WorkspaceRegistry;

/// The variable a kernel reads its workspace's MCP endpoint from.
pub const MCP_URL_VAR: &str = "POSITRON_MCP_URL";

/// The variable a kernel reads its workspace's bearer token from.
pub const MCP_TOKEN_VAR: &str = "POSITRON_MCP_TOKEN";

/// One workspace's MCP endpoint.
type WorkspaceService = StreamableHttpService<PositronMcpHandler, LocalSessionManager>;

/// A running MCP listener.
struct ListenerHandle {
    port: u16,
    cancel: CancellationToken,
}

/// Everything the MCP server needs from the supervisor.
pub struct McpState {
    /// The registered workspaces and their tokens.
    pub registry: Arc<WorkspaceRegistry>,

    /// The supervisor's kernel sessions.
    kernel_sessions: Arc<std::sync::RwLock<Vec<KernelSession>>>,

    /// Nudged on every tool call so an active agent postpones idle shutdown.
    idle_nudge_tx: mpsc::Sender<Option<u32>>,

    /// Tool calls served since the listener started.
    request_count: AtomicU64,

    /// The listener, when one is running. Held across the bind so two
    /// registrations racing cannot each open a port.
    listener: tokio::sync::Mutex<Option<ListenerHandle>>,

    /// The HTTP service behind each endpoint, built on first use, keyed by the
    /// workspace and by the session a caller named as its own.
    services: tokio::sync::Mutex<HashMap<(String, Option<String>), WorkspaceService>>,
}

impl McpState {
    /// Create the MCP state for a supervisor.
    pub fn new(
        kernel_sessions: Arc<std::sync::RwLock<Vec<KernelSession>>>,
        idle_nudge_tx: mpsc::Sender<Option<u32>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry: Arc::new(WorkspaceRegistry::new()),
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
            // One listener serves every workspace, so a later registration's
            // preferred port cannot be honoured. Say so rather than leaving the
            // window to wonder why its setting did nothing.
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

    /// The HTTP service serving a workspace's endpoint.
    ///
    /// Each workspace gets its own service, so the handler knows which one it
    /// is answering for without inspecting every request, and protocol sessions
    /// belong to one workspace and go away with it. A caller that named the
    /// session it runs in gets a service of its own for the same reason: the
    /// handler then knows which session not to run code in.
    ///
    /// Returns None when the listener is not running.
    pub async fn service_for(
        self: &Arc<Self>,
        workspace_id: &str,
        caller_session_id: Option<&str>,
    ) -> Option<WorkspaceService> {
        let key = (
            workspace_id.to_string(),
            caller_session_id.map(str::to_string),
        );
        let mut services = self.services.lock().await;
        if let Some(existing) = services.get(&key) {
            return Some(existing.clone());
        }

        let cancel = self.listener.lock().await.as_ref()?.cancel.child_token();
        let state = self.clone();
        let id = workspace_id.to_string();
        let caller = caller_session_id.map(str::to_string);
        let service = StreamableHttpService::new(
            move || {
                Ok(PositronMcpHandler::new(
                    state.clone(),
                    id.clone(),
                    caller.clone(),
                ))
            },
            Arc::new(LocalSessionManager::default()),
            // Sessions are kept for pre-2026-07-28 clients. Those clients
            // report who they are only in the initialize handshake, and the
            // agent's name is what attributes executions in the user's console.
            StreamableHttpServerConfig::default().with_cancellation_token(cancel),
        );
        services.insert(key, service.clone());
        Some(service)
    }

    /// Drop a workspace's endpoints, ending its agents' protocol sessions.
    pub async fn drop_service(&self, workspace_id: &str) {
        self.services
            .lock()
            .await
            .retain(|(workspace, _), _| workspace != workspace_id);
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
            workspaces: self.registry.status().await,
        }
    }
}
