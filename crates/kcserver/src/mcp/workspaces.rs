//
// workspaces.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Tracks the Positron workspaces registered with the MCP server.
//!
//! A workspace record owns the bearer token agents present, the cached command
//! catalog, the set of sessions the workspace holds, and the frontend channels
//! used to broker command requests. Records outlive their channels: when a
//! window closes, the token, catalog, and session set stay valid so an agent in
//! a terminal keeps working, and reconnecting reuses the record.
//!
//! A record is the workspace, not the window looking at it. Positron stores the
//! workspace ID in workspace-scoped state, and its session set is
//! workspace-scoped too, so two windows onto the same workspace share a record
//! and attach a frontend channel each; commands go to whichever of them the
//! user last focused. A window with no folder open still gets a record of its
//! own, so the mapping is close to, but not exactly, one per workspace.

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

/// A registered workspace.
struct Workspace {
    display_name: String,
    token: String,
    positron_version: Option<String>,
    commands: Vec<AgentCommand>,

    /// The sessions this workspace says it holds. Sessions it created name it
    /// as their owner and need no claim; this covers the rest, such as sessions
    /// that were already running when the workspace registered.
    session_ids: Vec<String>,

    foreground_session_id: Option<String>,
    disconnected_since: Option<DateTime<Utc>>,
    next_generation: u64,
    next_focus_seq: u64,
    channels: Vec<FrontendChannel>,
    pending: HashMap<String, Pending>,
}

impl Workspace {
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

/// The set of workspaces known to this server.
#[derive(Default)]
pub struct WorkspaceRegistry {
    workspaces: RwLock<HashMap<String, Workspace>>,

    /// Notified whenever a frontend channel connects, so callers waiting for a
    /// window to come back can wake immediately instead of polling.
    connected: Notify,
}

/// The outcome of a brokered Positron command.
pub enum CommandOutcome {
    /// A window answered.
    Replied(CommandReply),

    /// No frontend channel connected within the wait.
    Disconnected {
        /// When the workspace's last channel dropped, if it was ever connected.
        since: Option<DateTime<Utc>>,
    },

    /// A window was connected but did not answer in time.
    TimedOut,

    /// The workspace is not registered.
    UnknownWorkspace,
}

impl WorkspaceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a workspace, or return the existing record when the ID is
    /// already known. Re-registration deliberately keeps the same token so
    /// terminals opened before an extension host restart keep authenticating.
    ///
    /// An ID the caller supplies is honored only if it is well formed, since it
    /// goes into the endpoint URL agents connect to; otherwise a fresh one is
    /// minted from the display name.
    ///
    /// Returns the workspace ID and its token.
    pub async fn register(
        &self,
        registration: &models::McpWorkspaceRegistration,
    ) -> (String, String) {
        let mut workspaces = self.workspaces.write().await;

        if let Some(id) = registration.workspace_id.as_ref() {
            if let Some(existing) = workspaces.get_mut(id) {
                existing.display_name = registration.display_name.clone();
                return (id.clone(), existing.token.clone());
            }
        }

        let id = match registration.workspace_id.as_deref() {
            Some(supplied) if is_valid_id(supplied) => supplied.to_string(),
            Some(supplied) => {
                log::warn!(
                    "Ignoring malformed MCP workspace ID '{}'; issuing a new one",
                    supplied
                );
                mint_id(&registration.display_name, &workspaces)
            }
            None => mint_id(&registration.display_name, &workspaces),
        };
        let token = generate_token();
        workspaces.insert(
            id.clone(),
            Workspace {
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

    /// Remove a workspace and invalidate its token.
    ///
    /// Returns true if the workspace was registered.
    pub async fn deregister(&self, workspace_id: &str) -> bool {
        let mut workspaces = self.workspaces.write().await;
        workspaces.remove(workspace_id).is_some()
    }

    /// Whether any workspace is registered.
    pub async fn is_empty(&self) -> bool {
        self.workspaces.read().await.is_empty()
    }

    /// Whether a workspace with this ID is registered.
    pub async fn contains(&self, workspace_id: &str) -> bool {
        self.workspaces.read().await.contains_key(workspace_id)
    }

    /// The IDs of every registered workspace. A session whose owner is not
    /// among them belongs to a workspace that has gone for good, so it is up
    /// for adoption.
    pub async fn registered_ids(&self) -> HashSet<String> {
        self.workspaces.read().await.keys().cloned().collect()
    }

    /// Find the workspace a bearer token belongs to.
    ///
    /// Tokens are compared in constant time so a caller cannot learn a valid
    /// token by timing repeated guesses.
    pub async fn workspace_for_token(&self, token: &str) -> Option<String> {
        let workspaces = self.workspaces.read().await;
        let mut found = None;
        for (id, workspace) in workspaces.iter() {
            if constant_time_eq(workspace.token.as_bytes(), token.as_bytes()) {
                found = Some(id.clone());
            }
        }
        found
    }

    /// A workspace's bearer token, for handing to a process the supervisor
    /// starts on its behalf. Never logged, and never sent to an agent.
    pub async fn token(&self, workspace_id: &str) -> Option<String> {
        self.workspaces
            .read()
            .await
            .get(workspace_id)
            .map(|workspace| workspace.token.clone())
    }

    /// Attach a newly connected channel.
    ///
    /// Returns the channel's generation, which must be handed back to
    /// `handle_message` and `detach_channel`, or None if the workspace is not
    /// registered.
    pub async fn attach_channel(
        &self,
        workspace_id: &str,
        channel_tx: mpsc::UnboundedSender<ServerFrontendMessage>,
    ) -> Option<u64> {
        let generation = {
            let mut workspaces = self.workspaces.write().await;
            workspaces.get_mut(workspace_id)?.attach(channel_tx)
        };
        self.connected.notify_waiters();
        Some(generation)
    }

    /// Detach a channel that has closed.
    ///
    /// Requests the departing window was serving move to another attached
    /// window, if there is one; otherwise they fail immediately rather than
    /// being left to time out.
    pub async fn detach_channel(&self, workspace_id: &str, generation: u64) {
        let mut workspaces = self.workspaces.write().await;
        let Some(workspace) = workspaces.get_mut(workspace_id) else {
            return;
        };
        workspace
            .channels
            .retain(|channel| channel.generation != generation);

        let orphaned: Vec<String> = workspace
            .pending
            .iter()
            .filter(|(_, pending)| pending.served_by == generation)
            .map(|(id, _)| id.clone())
            .collect();
        for id in orphaned {
            let Some(pending) = workspace.pending.remove(&id) else {
                continue;
            };
            // A request the departing window was serving moves to a sibling.
            // When there is none, dropping the reply sender resolves the
            // caller's wait, which then reports the workspace as disconnected.
            if let Some(served_by) = workspace.dispatch(&pending.request) {
                workspace.pending.insert(
                    id,
                    Pending {
                        served_by,
                        ..pending
                    },
                );
            }
        }

        if !workspace.connected() {
            workspace.disconnected_since = Some(Utc::now());
            workspace.pending.clear();
        }
    }

    /// Apply a message received from a window over the given channel.
    pub async fn handle_message(
        &self,
        workspace_id: &str,
        generation: u64,
        message: FrontendMessage,
    ) {
        let mut workspaces = self.workspaces.write().await;
        let Some(workspace) = workspaces.get_mut(workspace_id) else {
            return;
        };
        match message {
            FrontendMessage::Hello(hello) => {
                log::info!(
                    "MCP workspace '{}' ({}) connected with {} command(s) and {} session(s)",
                    workspace_id,
                    hello.positron_version.as_deref().unwrap_or("unknown"),
                    hello.commands.len(),
                    hello.session_ids.len()
                );
                workspace.positron_version = hello.positron_version;
                workspace.commands = hello.commands;
                workspace.session_ids = hello.session_ids;
                workspace.foreground_session_id = hello.foreground_session_id;
                if hello.focused {
                    workspace.focus(generation);
                }
            }
            FrontendMessage::CommandsChanged(changed) => {
                log::info!(
                    "MCP workspace '{}' refreshed its catalog to {} command(s)",
                    workspace_id,
                    changed.commands.len()
                );
                workspace.commands = changed.commands;
            }
            FrontendMessage::ForegroundChanged(changed) => {
                workspace.foreground_session_id = changed.session_id;
            }
            FrontendMessage::SessionsChanged(changed) => {
                log::debug!(
                    "MCP workspace '{}' now holds {} session(s)",
                    workspace_id,
                    changed.session_ids.len()
                );
                workspace.session_ids = changed.session_ids;
            }
            FrontendMessage::Focused => workspace.focus(generation),
            FrontendMessage::CommandReply(reply) => match workspace.pending.remove(&reply.id) {
                Some(pending) => {
                    let _ = pending.reply.send(reply);
                }
                None => log::debug!(
                    "MCP workspace '{}' replied to unknown command request '{}'",
                    workspace_id,
                    reply.id
                ),
            },
        }
    }

    /// The cached command catalog for a workspace. Available whether or not the
    /// workspace is currently connected.
    pub async fn commands(&self, workspace_id: &str) -> Vec<AgentCommand> {
        self.workspaces
            .read()
            .await
            .get(workspace_id)
            .map(|f| f.commands.clone())
            .unwrap_or_default()
    }

    /// The sessions a workspace says it holds, beyond those it created.
    pub async fn session_ids(&self, workspace_id: &str) -> Vec<String> {
        self.workspaces
            .read()
            .await
            .get(workspace_id)
            .map(|f| f.session_ids.clone())
            .unwrap_or_default()
    }

    /// The session a workspace's windows last reported as foreground.
    pub async fn foreground_session(&self, workspace_id: &str) -> Option<String> {
        self.workspaces
            .read()
            .await
            .get(workspace_id)
            .and_then(|f| f.foreground_session_id.clone())
    }

    /// Whether any of a workspace's windows is connected, and if not, when the
    /// last one dropped.
    pub async fn connection_state(&self, workspace_id: &str) -> (bool, Option<DateTime<Utc>>) {
        match self.workspaces.read().await.get(workspace_id) {
            Some(workspace) => (workspace.connected(), workspace.disconnected_since),
            None => (false, None),
        }
    }

    /// The name a workspace registered itself under, normally its folder.
    pub async fn display_name(&self, workspace_id: &str) -> Option<String> {
        self.workspaces
            .read()
            .await
            .get(workspace_id)
            .map(|f| f.display_name.clone())
    }

    /// The version of Positron hosting a workspace, if a window has said hello.
    pub async fn positron_version(&self, workspace_id: &str) -> Option<String> {
        self.workspaces
            .read()
            .await
            .get(workspace_id)
            .and_then(|f| f.positron_version.clone())
    }

    /// Send a command to a workspace's focused window and wait for its reply.
    ///
    /// Waits up to `connect_wait` for a channel to appear, which covers the
    /// common case of a window reload, then up to `reply_wait` for the answer.
    pub async fn run_command(
        &self,
        workspace_id: &str,
        request: CommandRequest,
        connect_wait: Duration,
        reply_wait: Duration,
    ) -> CommandOutcome {
        let deadline = tokio::time::Instant::now() + connect_wait;
        let receiver = loop {
            // Take a listener before checking, so a connection that lands
            // between the check and the wait is not missed.
            let notified = self.connected.notified();

            match self.enqueue(workspace_id, &request).await {
                Ok(Some(receiver)) => break receiver,
                Ok(None) => {}
                Err(outcome) => return outcome,
            }

            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                let (_, since) = self.connection_state(workspace_id).await;
                return CommandOutcome::Disconnected { since };
            }
        };

        match tokio::time::timeout(reply_wait, receiver).await {
            Ok(Ok(reply)) => CommandOutcome::Replied(reply),
            // The sender was dropped, which happens when the last window
            // detaches while the request is in flight.
            Ok(Err(_)) => {
                let (_, since) = self.connection_state(workspace_id).await;
                CommandOutcome::Disconnected { since }
            }
            Err(_) => {
                self.cancel_pending(workspace_id, &request.id).await;
                CommandOutcome::TimedOut
            }
        }
    }

    /// Try to hand a command request to one of a workspace's windows.
    ///
    /// Returns `Ok(None)` when the workspace is registered but not connected.
    async fn enqueue(
        &self,
        workspace_id: &str,
        request: &CommandRequest,
    ) -> Result<Option<oneshot::Receiver<CommandReply>>, CommandOutcome> {
        let mut workspaces = self.workspaces.write().await;
        let Some(workspace) = workspaces.get_mut(workspace_id) else {
            return Err(CommandOutcome::UnknownWorkspace);
        };
        let Some(served_by) = workspace.dispatch(request) else {
            workspace.disconnected_since.get_or_insert_with(Utc::now);
            return Ok(None);
        };

        let (tx, rx) = oneshot::channel();
        workspace.pending.insert(
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
    async fn cancel_pending(&self, workspace_id: &str, request_id: &str) {
        let mut workspaces = self.workspaces.write().await;
        if let Some(workspace) = workspaces.get_mut(workspace_id) {
            workspace.pending.remove(request_id);
        }
    }

    /// Summarize the registry for the server status endpoint.
    pub async fn status(&self) -> Vec<models::McpWorkspaceStatus> {
        let workspaces = self.workspaces.read().await;
        let mut status: Vec<models::McpWorkspaceStatus> = workspaces
            .iter()
            .map(|(id, workspace)| models::McpWorkspaceStatus {
                id: id.clone(),
                display_name: workspace.display_name.clone(),
                connected: workspace.connected(),
            })
            .collect();
        status.sort_by(|a, b| a.id.cmp(&b.id));
        status
    }
}

/// The longest slug taken from a display name, before the suffix.
const MAX_SLUG_LEN: usize = 24;

/// How many random characters disambiguate two workspaces of the same name.
const SUFFIX_LEN: usize = 6;

/// The longest ID we will accept or issue.
const MAX_ID_LEN: usize = 40;

/// What a slug falls back to when a display name has nothing usable in it.
const FALLBACK_SLUG: &str = "workspace";

/// Mint an ID for a workspace: its name, made URL-safe, plus enough randomness
/// that two workspaces of the same name never collide.
///
/// The randomness is not a secret — the token is — but it does mean that a
/// window re-registering an ID saved under a previous server process cannot
/// land on a different workspace's record and inherit its token and sessions.
fn mint_id(display_name: &str, taken: &HashMap<String, Workspace>) -> String {
    let slug = slugify(display_name);
    loop {
        let id = format!("{}-{}", slug, random_suffix());
        if !taken.contains_key(&id) {
            return id;
        }
    }
}

/// Reduce a display name to the readable part of an ID: lowercase, ASCII
/// alphanumerics, single hyphens between them.
fn slugify(display_name: &str) -> String {
    let mut slug = String::with_capacity(MAX_SLUG_LEN);
    for ch in display_name.chars() {
        if slug.len() >= MAX_SLUG_LEN {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }

    // Truncating mid-word is fine; a trailing hyphen is not, and a name with no
    // ASCII in it at all leaves nothing to truncate.
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        FALLBACK_SLUG.to_string()
    } else {
        slug.to_string()
    }
}

/// Generate the random tail of an ID.
fn random_suffix() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..SUFFIX_LEN)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

/// Whether an ID is safe to route on and readable enough to print.
fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
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

    fn registration(display_name: &str) -> models::McpWorkspaceRegistration {
        models::McpWorkspaceRegistration::new(display_name.to_string())
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
        registry: &WorkspaceRegistry,
        workspace_id: &str,
    ) -> (u64, mpsc::UnboundedReceiver<ServerFrontendMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let generation = registry.attach_channel(workspace_id, tx).await.unwrap();
        (generation, rx)
    }

    /// Hand a request to the registry, expecting it to reach a window.
    async fn enqueue(
        registry: &WorkspaceRegistry,
        workspace_id: &str,
        request_id: &str,
    ) -> oneshot::Receiver<CommandReply> {
        match registry.enqueue(workspace_id, &request(request_id)).await {
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
    fn slugs_are_readable_and_url_safe() {
        assert_eq!(slugify("my-project"), "my-project");
        assert_eq!(slugify("My Project"), "my-project");
        assert_eq!(slugify("  Sales / Q3 (2026)!  "), "sales-q3-2026");
        assert_eq!(slugify("研究"), FALLBACK_SLUG);
        assert_eq!(slugify(""), FALLBACK_SLUG);
        assert_eq!(
            slugify("a-very-long-workspace-name-that-keeps-going"),
            "a-very-long-workspace-nam"[..MAX_SLUG_LEN].to_string()
        );
    }

    #[tokio::test]
    async fn ids_name_the_workspace_and_stay_distinct() {
        let registry = WorkspaceRegistry::new();
        let (first, _) = registry.register(&registration("My Project")).await;
        let (second, _) = registry.register(&registration("My Project")).await;

        assert!(first.starts_with("my-project-"), "{}", first);
        assert_eq!(first.len(), "my-project-".len() + SUFFIX_LEN);
        assert!(is_valid_id(&first));
        // Two folders of the same name must not end up sharing a record, and
        // with it a token and a session set.
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn a_malformed_supplied_id_is_replaced() {
        let registry = WorkspaceRegistry::new();
        let mut registration = registration("My Project");

        // An ID goes into the endpoint path, so one that could change the route
        // is refused rather than trusted.
        registration.workspace_id = Some("../../sessions".to_string());
        let (id, _) = registry.register(&registration).await;
        assert!(id.starts_with("my-project-"), "{}", id);

        // A well-formed one is honored, so a window that reloads keeps its URL.
        registration.workspace_id = Some("my-project-abc123".to_string());
        let (id, _) = registry.register(&registration).await;
        assert_eq!(id, "my-project-abc123");
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
        let registry = WorkspaceRegistry::new();
        let mut registration = registration("Workspace 1");
        let (id, token) = registry.register(&registration).await;

        registration.workspace_id = Some(id.clone());
        registration.display_name = "Workspace 1 (reloaded)".to_string();
        let (again, same_token) = registry.register(&registration).await;

        assert_eq!(id, again);
        assert_eq!(token, same_token);
        assert_eq!(registry.workspace_for_token(&token).await, Some(id));
    }

    #[tokio::test]
    async fn deregistration_invalidates_the_token() {
        let registry = WorkspaceRegistry::new();
        let (id, token) = registry.register(&registration("Workspace 1")).await;

        assert!(registry.deregister(&id).await);
        assert!(registry.workspace_for_token(&token).await.is_none());
        assert!(registry.is_empty().await);
        assert!(!registry.deregister(&id).await);
    }

    #[tokio::test]
    async fn the_session_claim_outlives_the_window() {
        let registry = WorkspaceRegistry::new();
        let (id, _) = registry.register(&registration("Workspace 1")).await;
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
        let registry = WorkspaceRegistry::new();
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
        let registry = WorkspaceRegistry::new();
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
        let registry = WorkspaceRegistry::new();
        let (id, _) = registry.register(&registration("Workspace 1")).await;
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
