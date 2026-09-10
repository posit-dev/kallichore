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
//! catalog, and the channel used to broker command requests. Records outlive
//! the channel: when a window closes, the token and catalog stay valid so an
//! agent in a terminal keeps working, and reconnecting reuses the record.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use kallichore_api::models;
use kcshared::mcp_frontend::{
    AgentCommand, CommandReply, CommandRequest, FrontendMessage, ServerFrontendMessage,
};
use rand::Rng;
use tokio::sync::{mpsc, oneshot, Notify, RwLock};

/// A registered frontend.
struct Frontend {
    display_name: String,
    token: String,
    positron_version: Option<String>,
    commands: Vec<AgentCommand>,
    foreground_session_id: Option<String>,
    disconnected_since: Option<DateTime<Utc>>,
    /// Incremented on every connect, so a channel that has been replaced can
    /// tell that its own teardown must not clear the new one's state.
    channel_generation: u64,
    channel_tx: Option<mpsc::UnboundedSender<ServerFrontendMessage>>,
    pending: HashMap<String, oneshot::Sender<CommandReply>>,
}

impl Frontend {
    fn connected(&self) -> bool {
        self.channel_tx.is_some()
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
        /// When the frontend's channel dropped, if it was ever connected.
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
                foreground_session_id: None,
                disconnected_since: None,
                channel_generation: 0,
                channel_tx: None,
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

    /// Attach a newly connected channel, replacing any existing one.
    ///
    /// Returns the channel's generation, which must be handed back to
    /// `detach_channel`, or None if the frontend is not registered.
    pub async fn attach_channel(
        &self,
        frontend_id: &str,
        channel_tx: mpsc::UnboundedSender<ServerFrontendMessage>,
    ) -> Option<u64> {
        let generation = {
            let mut frontends = self.frontends.write().await;
            let frontend = frontends.get_mut(frontend_id)?;
            frontend.channel_generation += 1;
            frontend.channel_tx = Some(channel_tx);
            frontend.disconnected_since = None;
            frontend.channel_generation
        };
        self.connected.notify_waiters();
        Some(generation)
    }

    /// Detach a channel that has closed. Pending command requests are failed
    /// immediately rather than left to time out.
    ///
    /// Does nothing if a newer channel has since taken over.
    pub async fn detach_channel(&self, frontend_id: &str, generation: u64) {
        let mut frontends = self.frontends.write().await;
        let Some(frontend) = frontends.get_mut(frontend_id) else {
            return;
        };
        if frontend.channel_generation != generation {
            return;
        }
        frontend.channel_tx = None;
        frontend.disconnected_since = Some(Utc::now());
        frontend.pending.clear();
    }

    /// Apply a message received from a frontend.
    pub async fn handle_message(&self, frontend_id: &str, message: FrontendMessage) {
        let mut frontends = self.frontends.write().await;
        let Some(frontend) = frontends.get_mut(frontend_id) else {
            return;
        };
        match message {
            FrontendMessage::Hello(hello) => {
                log::info!(
                    "MCP frontend '{}' ({}) connected with {} command(s)",
                    frontend_id,
                    hello.positron_version.as_deref().unwrap_or("unknown"),
                    hello.commands.len()
                );
                frontend.positron_version = hello.positron_version;
                frontend.commands = hello.commands;
                frontend.foreground_session_id = hello.foreground_session_id;
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
            FrontendMessage::CommandReply(reply) => match frontend.pending.remove(&reply.id) {
                Some(sender) => {
                    let _ = sender.send(reply);
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

    /// The session a frontend last reported as foreground.
    pub async fn foreground_session(&self, frontend_id: &str) -> Option<String> {
        self.frontends
            .read()
            .await
            .get(frontend_id)
            .and_then(|f| f.foreground_session_id.clone())
    }

    /// Whether a frontend's channel is connected, and if not, when it dropped.
    pub async fn connection_state(&self, frontend_id: &str) -> (bool, Option<DateTime<Utc>>) {
        match self.frontends.read().await.get(frontend_id) {
            Some(frontend) => (frontend.connected(), frontend.disconnected_since),
            None => (false, None),
        }
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
            // The sender was dropped, which happens when the channel detaches
            // while the request is in flight.
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

    /// Try to hand a command request to a connected frontend.
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
        let Some(channel_tx) = frontend.channel_tx.clone() else {
            return Ok(None);
        };

        let (tx, rx) = oneshot::channel();
        frontend.pending.insert(request.id.clone(), tx);
        if channel_tx
            .send(ServerFrontendMessage::CommandRequest(request.clone()))
            .is_err()
        {
            frontend.pending.remove(&request.id);
            frontend.channel_tx = None;
            frontend.disconnected_since = Some(Utc::now());
            return Ok(None);
        }
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
        let mut registration = models::McpFrontendRegistration::new("Window 1".to_string());
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
        let registration = models::McpFrontendRegistration::new("Window 1".to_string());
        let (id, token) = registry.register(&registration).await;

        assert!(registry.deregister(&id).await);
        assert!(registry.frontend_for_token(&token).await.is_none());
        assert!(registry.is_empty().await);
        assert!(!registry.deregister(&id).await);
    }
}
