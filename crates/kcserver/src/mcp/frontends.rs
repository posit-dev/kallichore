//
// frontends.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Tracks the Positron frontends registered with the MCP server.
//!
//! A frontend record owns the bearer token agents present, the cached command
//! catalog, the set of sessions the frontend holds, and the channels used to
//! broker command requests. Records outlive their channels: when a window
//! closes, the token, catalog, and session set stay valid so an agent in a
//! terminal keeps working, and reconnecting reuses the record.
//!
//! A record represents one Positron *view* of a workspace, not one window.
//! Positron stores the frontend ID in workspace-scoped state, and its session
//! set is workspace-scoped too, so two windows onto the same workspace share a
//! record and attach a channel each. Commands go to whichever of them the user
//! last focused.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use kallichore_api::models;
use kcshared::mcp_frontend::{
    AgentCommand, CommandReply, CommandRequest, FrontendMessage, ServerFrontendMessage,
};
use rand::Rng;
use tokio::sync::{mpsc, oneshot, Notify, RwLock};

/// One attached window's channel.
struct FrontendChannel {
    /// Identifies this channel for the lifetime of the record; handed back on
    /// focus and teardown so a channel that has already been replaced cannot
    /// disturb its successor.
    generation: u64,

    /// When the window behind this channel last took focus. The highest wins
    /// command brokering.
    focus_seq: u64,

    tx: mpsc::UnboundedSender<ServerFrontendMessage>,
}

/// A command request waiting for an answer.
struct Pending {
    /// Kept so the request can be re-sent to another window if the one serving
    /// it disconnects mid-flight.
    request: CommandRequest,

    /// The channel the request was handed to.
    served_by: u64,

    reply: oneshot::Sender<CommandReply>,
}

/// A registered frontend.
struct Frontend {
    display_name: String,
    token: String,
    positron_version: Option<String>,
    commands: Vec<AgentCommand>,

    /// The sessions this frontend says it holds. Sessions it created name it
    /// as their owner and need no claim; this covers the rest, such as sessions
    /// that were already running when the frontend registered.
    session_ids: Vec<String>,

    foreground_session_id: Option<String>,
    disconnected_since: Option<DateTime<Utc>>,
    next_generation: u64,
    next_focus_seq: u64,
    channels: Vec<FrontendChannel>,
    pending: HashMap<String, Pending>,
}

impl Frontend {
    fn connected(&self) -> bool {
        !self.channels.is_empty()
    }

    /// The channel commands should go to: the window the user last focused, or
    /// the most recently attached if none has ever reported focus.
    fn primary(&self) -> Option<usize> {
        self.channels
            .iter()
            .enumerate()
            .max_by_key(|(_, channel)| channel.focus_seq)
            .map(|(index, _)| index)
    }

    /// Attach a channel and return its generation.
    ///
    /// A new channel starts unfocused, and says in its `hello` whether its
    /// window has focus. Connecting must not be enough to take command
    /// brokering, or a background window reloading would pull an agent's IDE
    /// commands away from the window the user is working in.
    fn attach(&mut self, tx: mpsc::UnboundedSender<ServerFrontendMessage>) -> u64 {
        self.next_generation += 1;
        self.channels.push(FrontendChannel {
            generation: self.next_generation,
            focus_seq: 0,
            tx,
        });
        self.disconnected_since = None;
        self.next_generation
    }

    /// Make a channel's window the one commands go to.
    fn focus(&mut self, generation: u64) {
        let focus_seq = self.take_focus_seq();
        if let Some(channel) = self
            .channels
            .iter_mut()
            .find(|channel| channel.generation == generation)
        {
            channel.focus_seq = focus_seq;
        }
    }

    fn take_focus_seq(&mut self) -> u64 {
        self.next_focus_seq += 1;
        self.next_focus_seq
    }

    /// Hand a request to the window commands should go to, discarding channels
    /// that have died since we last heard from them.
    ///
    /// Returns the generation of the channel that took it.
    fn dispatch(&mut self, request: &CommandRequest) -> Option<u64> {
        loop {
            let index = self.primary()?;
            let channel = &self.channels[index];
            if channel
                .tx
                .send(ServerFrontendMessage::CommandRequest(request.clone()))
                .is_ok()
            {
                return Some(channel.generation);
            }
            self.channels.remove(index);
        }
    }
}

/// The set of frontends known to this server.
#[derive(Default)]
pub struct FrontendRegistry {
    frontends: RwLock<HashMap<String, Frontend>>,

    /// Notified whenever a frontend channel connects, so callers waiting for a
    /// window to come back can wake immediately instead of polling.
    connected: Notify,
}

/// The outcome of a brokered Positron command.
pub enum CommandOutcome {
    /// The frontend answered.
    Replied(CommandReply),

    /// No frontend channel connected within the wait.
    Disconnected {
        /// When the frontend's last channel dropped, if it was ever connected.
        since: Option<DateTime<Utc>>,
    },

    /// The frontend was connected but did not answer in time.
    TimedOut,

    /// The frontend is not registered.
    UnknownFrontend,
}

impl FrontendRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a frontend, or return the existing record when the ID is
    /// already known. Re-registration deliberately keeps the same token so
    /// terminals opened before an extension host restart keep authenticating.
    ///
    /// Returns the frontend ID and its token.
    pub async fn register(
        &self,
        registration: &models::McpFrontendRegistration,
    ) -> (String, String) {
        let mut frontends = self.frontends.write().await;

        if let Some(id) = registration.frontend_id.as_ref() {
            if let Some(existing) = frontends.get_mut(id) {
                existing.display_name = registration.display_name.clone();
                return (id.clone(), existing.token.clone());
            }
        }

        let id = registration
            .frontend_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let token = generate_token();
        frontends.insert(
            id.clone(),
            Frontend {
                display_name: registration.display_name.clone(),
                token: token.clone(),
                positron_version: None,
                commands: Vec::new(),
                session_ids: Vec::new(),
                foreground_session_id: None,
                disconnected_since: None,
                next_generation: 0,
                next_focus_seq: 0,
                channels: Vec::new(),
                pending: HashMap::new(),
            },
        );
        (id, token)
    }

    /// Remove a frontend and invalidate its token.
    ///
    /// Returns true if the frontend was registered.
    pub async fn deregister(&self, frontend_id: &str) -> bool {
        let mut frontends = self.frontends.write().await;
        frontends.remove(frontend_id).is_some()
    }

    /// Whether any frontend is registered.
    pub async fn is_empty(&self) -> bool {
        self.frontends.read().await.is_empty()
    }

    /// Whether a frontend with this ID is registered.
    pub async fn contains(&self, frontend_id: &str) -> bool {
        self.frontends.read().await.contains_key(frontend_id)
    }

    /// The IDs of every registered frontend. A session whose owner is not among
    /// them belongs to a window that has gone for good, so it is up for
    /// adoption.
    pub async fn registered_ids(&self) -> HashSet<String> {
        self.frontends.read().await.keys().cloned().collect()
    }

    /// Find the frontend a bearer token belongs to.
    ///
    /// Tokens are compared in constant time so a caller cannot learn a valid
    /// token by timing repeated guesses.
    pub async fn frontend_for_token(&self, token: &str) -> Option<String> {
        let frontends = self.frontends.read().await;
        let mut found = None;
        for (id, frontend) in frontends.iter() {
            if constant_time_eq(frontend.token.as_bytes(), token.as_bytes()) {
                found = Some(id.clone());
            }
        }
        found
    }

    /// Attach a newly connected channel.
    ///
    /// Returns the channel's generation, which must be handed back to
    /// `handle_message` and `detach_channel`, or None if the frontend is not
    /// registered.
    pub async fn attach_channel(
        &self,
        frontend_id: &str,
        channel_tx: mpsc::UnboundedSender<ServerFrontendMessage>,
    ) -> Option<u64> {
        let generation = {
            let mut frontends = self.frontends.write().await;
            frontends.get_mut(frontend_id)?.attach(channel_tx)
        };
        self.connected.notify_waiters();
        Some(generation)
    }

    /// Detach a channel that has closed.
    ///
    /// Requests the departing window was serving move to another attached
    /// window, if there is one; otherwise they fail immediately rather than
    /// being left to time out.
    pub async fn detach_channel(&self, frontend_id: &str, generation: u64) {
        let mut frontends = self.frontends.write().await;
        let Some(frontend) = frontends.get_mut(frontend_id) else {
            return;
        };
        frontend
            .channels
            .retain(|channel| channel.generation != generation);

        let orphaned: Vec<String> = frontend
            .pending
            .iter()
            .filter(|(_, pending)| pending.served_by == generation)
            .map(|(id, _)| id.clone())
            .collect();
        for id in orphaned {
            let Some(pending) = frontend.pending.remove(&id) else {
                continue;
            };
            // A request the departing window was serving moves to a sibling.
            // When there is none, dropping the reply sender resolves the
            // caller's wait, which then reports the frontend as disconnected.
            if let Some(served_by) = frontend.dispatch(&pending.request) {
                frontend.pending.insert(
                    id,
                    Pending {
                        served_by,
                        ..pending
                    },
                );
            }
        }

        if !frontend.connected() {
            frontend.disconnected_since = Some(Utc::now());
            frontend.pending.clear();
        }
    }

    /// Apply a message received from a frontend over the given channel.
    pub async fn handle_message(
        &self,
        frontend_id: &str,
        generation: u64,
        message: FrontendMessage,
    ) {
        let mut frontends = self.frontends.write().await;
        let Some(frontend) = frontends.get_mut(frontend_id) else {
            return;
        };
        match message {
            FrontendMessage::Hello(hello) => {
                log::info!(
                    "MCP frontend '{}' ({}) connected with {} command(s) and {} session(s)",
                    frontend_id,
                    hello.positron_version.as_deref().unwrap_or("unknown"),
                    hello.commands.len(),
                    hello.session_ids.len()
                );
                frontend.positron_version = hello.positron_version;
                frontend.commands = hello.commands;
                frontend.session_ids = hello.session_ids;
                frontend.foreground_session_id = hello.foreground_session_id;
                if hello.focused {
                    frontend.focus(generation);
                }
            }
            FrontendMessage::CommandsChanged(changed) => {
                log::info!(
                    "MCP frontend '{}' refreshed its catalog to {} command(s)",
                    frontend_id,
                    changed.commands.len()
                );
                frontend.commands = changed.commands;
            }
            FrontendMessage::ForegroundChanged(changed) => {
                frontend.foreground_session_id = changed.session_id;
            }
            FrontendMessage::SessionsChanged(changed) => {
                log::debug!(
                    "MCP frontend '{}' now holds {} session(s)",
                    frontend_id,
                    changed.session_ids.len()
                );
                frontend.session_ids = changed.session_ids;
            }
            FrontendMessage::Focused => frontend.focus(generation),
            FrontendMessage::CommandReply(reply) => match frontend.pending.remove(&reply.id) {
                Some(pending) => {
                    let _ = pending.reply.send(reply);
                }
                None => log::debug!(
                    "MCP frontend '{}' replied to unknown command request '{}'",
                    frontend_id,
                    reply.id
                ),
            },
        }
    }

    /// The cached command catalog for a frontend. Available whether or not the
    /// frontend is currently connected.
    pub async fn commands(&self, frontend_id: &str) -> Vec<AgentCommand> {
        self.frontends
            .read()
            .await
            .get(frontend_id)
            .map(|f| f.commands.clone())
            .unwrap_or_default()
    }

    /// The sessions a frontend says it holds, beyond those it created.
    pub async fn session_ids(&self, frontend_id: &str) -> Vec<String> {
        self.frontends
            .read()
            .await
            .get(frontend_id)
            .map(|f| f.session_ids.clone())
            .unwrap_or_default()
    }

    /// The session a frontend last reported as foreground.
    pub async fn foreground_session(&self, frontend_id: &str) -> Option<String> {
        self.frontends
            .read()
            .await
            .get(frontend_id)
            .and_then(|f| f.foreground_session_id.clone())
    }

    /// Whether any of a frontend's windows is connected, and if not, when the
    /// last one dropped.
    pub async fn connection_state(&self, frontend_id: &str) -> (bool, Option<DateTime<Utc>>) {
        match self.frontends.read().await.get(frontend_id) {
            Some(frontend) => (frontend.connected(), frontend.disconnected_since),
            None => (false, None),
        }
    }

    /// The name a frontend registered itself under, normally its workspace.
    pub async fn display_name(&self, frontend_id: &str) -> Option<String> {
        self.frontends
            .read()
            .await
            .get(frontend_id)
            .map(|f| f.display_name.clone())
    }

    /// The version of Positron hosting a frontend, if it has said hello.
    pub async fn positron_version(&self, frontend_id: &str) -> Option<String> {
        self.frontends
            .read()
            .await
            .get(frontend_id)
            .and_then(|f| f.positron_version.clone())
    }

    /// Send a command to a frontend and wait for its reply.
    ///
    /// Waits up to `connect_wait` for a channel to appear, which covers the
    /// common case of a window reload, then up to `reply_wait` for the answer.
    pub async fn run_command(
        &self,
        frontend_id: &str,
        request: CommandRequest,
        connect_wait: Duration,
        reply_wait: Duration,
    ) -> CommandOutcome {
        let deadline = tokio::time::Instant::now() + connect_wait;
        let receiver = loop {
            // Take a listener before checking, so a connection that lands
            // between the check and the wait is not missed.
            let notified = self.connected.notified();

            match self.enqueue(frontend_id, &request).await {
                Ok(Some(receiver)) => break receiver,
                Ok(None) => {}
                Err(outcome) => return outcome,
            }

            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                let (_, since) = self.connection_state(frontend_id).await;
                return CommandOutcome::Disconnected { since };
            }
        };

        match tokio::time::timeout(reply_wait, receiver).await {
            Ok(Ok(reply)) => CommandOutcome::Replied(reply),
            // The sender was dropped, which happens when the last window
            // detaches while the request is in flight.
            Ok(Err(_)) => {
                let (_, since) = self.connection_state(frontend_id).await;
                CommandOutcome::Disconnected { since }
            }
            Err(_) => {
                self.cancel_pending(frontend_id, &request.id).await;
                CommandOutcome::TimedOut
            }
        }
    }

    /// Try to hand a command request to one of a frontend's windows.
    ///
    /// Returns `Ok(None)` when the frontend is registered but not connected.
    async fn enqueue(
        &self,
        frontend_id: &str,
        request: &CommandRequest,
    ) -> Result<Option<oneshot::Receiver<CommandReply>>, CommandOutcome> {
        let mut frontends = self.frontends.write().await;
        let Some(frontend) = frontends.get_mut(frontend_id) else {
            return Err(CommandOutcome::UnknownFrontend);
        };
        let Some(served_by) = frontend.dispatch(request) else {
            frontend.disconnected_since.get_or_insert_with(Utc::now);
            return Ok(None);
        };

        let (tx, rx) = oneshot::channel();
        frontend.pending.insert(
            request.id.clone(),
            Pending {
                request: request.clone(),
                served_by,
                reply: tx,
            },
        );
        Ok(Some(rx))
    }

    /// Forget a request that timed out, so a late reply doesn't accumulate.
    async fn cancel_pending(&self, frontend_id: &str, request_id: &str) {
        let mut frontends = self.frontends.write().await;
        if let Some(frontend) = frontends.get_mut(frontend_id) {
            frontend.pending.remove(request_id);
        }
    }

    /// Summarize the registry for the server status endpoint.
    pub async fn status(&self) -> Vec<models::McpFrontendStatus> {
        let frontends = self.frontends.read().await;
        let mut status: Vec<models::McpFrontendStatus> = frontends
            .iter()
            .map(|(id, frontend)| models::McpFrontendStatus {
                id: id.clone(),
                display_name: frontend.display_name.clone(),
                connected: frontend.connected(),
            })
            .collect();
        status.sort_by(|a, b| a.id.cmp(&b.id));
        status
    }
}

/// Generate a 32-byte random token, rendered as hex.
fn generate_token() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}

/// Compare two byte strings without leaking their contents through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use kcshared::mcp_frontend::{AgentIdentity, FrontendHello, SessionsChanged};

    fn registration(display_name: &str) -> models::McpFrontendRegistration {
        models::McpFrontendRegistration::new(display_name.to_string())
    }

    fn request(id: &str) -> CommandRequest {
        CommandRequest {
            id: id.to_string(),
            command_id: "workbench.action.files.newFile".to_string(),
            args: Vec::new(),
            agent: AgentIdentity {
                name: None,
                version: None,
            },
            deadline_ms: 1000,
        }
    }

    /// Attach a channel, returning its generation and receiving end.
    async fn attach(
        registry: &FrontendRegistry,
        frontend_id: &str,
    ) -> (u64, mpsc::UnboundedReceiver<ServerFrontendMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let generation = registry.attach_channel(frontend_id, tx).await.unwrap();
        (generation, rx)
    }

    /// Hand a request to the registry, expecting it to reach a window.
    async fn enqueue(
        registry: &FrontendRegistry,
        frontend_id: &str,
        request_id: &str,
    ) -> oneshot::Receiver<CommandReply> {
        match registry.enqueue(frontend_id, &request(request_id)).await {
            Ok(Some(receiver)) => receiver,
            _ => panic!("request '{}' was not accepted", request_id),
        }
    }

    fn sent_request_id(message: ServerFrontendMessage) -> String {
        match message {
            ServerFrontendMessage::CommandRequest(request) => request.id,
        }
    }

    fn reply(id: &str) -> FrontendMessage {
        FrontendMessage::CommandReply(CommandReply {
            id: id.to_string(),
            ok: true,
            result: None,
            reason: None,
            message: None,
        })
    }

    #[test]
    fn constant_time_eq_matches_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[tokio::test]
    async fn re_registration_preserves_the_token() {
        let registry = FrontendRegistry::new();
        let mut registration = registration("Window 1");
        let (id, token) = registry.register(&registration).await;

        registration.frontend_id = Some(id.clone());
        registration.display_name = "Window 1 (reloaded)".to_string();
        let (again, same_token) = registry.register(&registration).await;

        assert_eq!(id, again);
        assert_eq!(token, same_token);
        assert_eq!(registry.frontend_for_token(&token).await, Some(id));
    }

    #[tokio::test]
    async fn deregistration_invalidates_the_token() {
        let registry = FrontendRegistry::new();
        let (id, token) = registry.register(&registration("Window 1")).await;

        assert!(registry.deregister(&id).await);
        assert!(registry.frontend_for_token(&token).await.is_none());
        assert!(registry.is_empty().await);
        assert!(!registry.deregister(&id).await);
    }

    #[tokio::test]
    async fn the_session_claim_outlives_the_window() {
        let registry = FrontendRegistry::new();
        let (id, _) = registry.register(&registration("Window 1")).await;
        let (generation, _channel) = attach(&registry, &id).await;

        registry
            .handle_message(
                &id,
                generation,
                FrontendMessage::Hello(FrontendHello {
                    positron_version: Some("2026.10.0".to_string()),
                    commands: Vec::new(),
                    session_ids: vec!["python-1".to_string()],
                    foreground_session_id: Some("python-1".to_string()),
                    history_api_enabled: false,
                    focused: true,
                }),
            )
            .await;
        assert_eq!(
            registry.session_ids(&id).await,
            vec!["python-1".to_string()]
        );

        registry
            .handle_message(
                &id,
                generation,
                FrontendMessage::SessionsChanged(SessionsChanged {
                    session_ids: vec!["python-1".to_string(), "r-1".to_string()],
                }),
            )
            .await;
        assert_eq!(registry.session_ids(&id).await.len(), 2);

        // The claim survives the window going away, so an agent in a terminal
        // that outlived it keeps reaching the same sessions.
        registry.detach_channel(&id, generation).await;
        assert_eq!(registry.session_ids(&id).await.len(), 2);
    }

    #[tokio::test]
    async fn commands_go_to_the_window_the_user_last_focused() {
        let registry = FrontendRegistry::new();
        let (id, _) = registry.register(&registration("Shared workspace")).await;
        let (first_generation, mut first) = attach(&registry, &id).await;
        let (_, mut second) = attach(&registry, &id).await;

        // Until a window reports focus, the most recent one serves.
        enqueue(&registry, &id, "a").await;
        assert_eq!(sent_request_id(second.recv().await.unwrap()), "a");

        registry
            .handle_message(&id, first_generation, FrontendMessage::Focused)
            .await;
        enqueue(&registry, &id, "b").await;
        assert_eq!(sent_request_id(first.recv().await.unwrap()), "b");
        assert!(second.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_departing_window_hands_its_request_to_a_sibling() {
        let registry = FrontendRegistry::new();
        let (id, _) = registry.register(&registration("Shared workspace")).await;
        let (first_generation, mut first) = attach(&registry, &id).await;
        let (second_generation, mut second) = attach(&registry, &id).await;

        // Focus the first window so it takes the request, then close it.
        registry
            .handle_message(&id, first_generation, FrontendMessage::Focused)
            .await;
        let receiver = enqueue(&registry, &id, "a").await;
        assert_eq!(sent_request_id(first.recv().await.unwrap()), "a");

        registry.detach_channel(&id, first_generation).await;
        assert_eq!(sent_request_id(second.recv().await.unwrap()), "a");

        registry
            .handle_message(&id, second_generation, reply("a"))
            .await;
        assert!(receiver.await.unwrap().ok);
    }

    #[tokio::test]
    async fn the_last_window_leaving_fails_pending_requests() {
        let registry = FrontendRegistry::new();
        let (id, _) = registry.register(&registration("Window 1")).await;
        let (generation, mut channel) = attach(&registry, &id).await;

        let receiver = enqueue(&registry, &id, "a").await;
        assert_eq!(sent_request_id(channel.recv().await.unwrap()), "a");

        registry.detach_channel(&id, generation).await;
        assert!(receiver.await.is_err());
        let (connected, since) = registry.connection_state(&id).await;
        assert!(!connected);
        assert!(since.is_some());
    }
}
