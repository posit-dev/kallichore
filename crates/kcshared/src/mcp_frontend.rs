//
// mcp_frontend.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Messages exchanged over the MCP frontend channel, the WebSocket that
//! connects a registered Positron window to the supervisor's MCP server.
//!
//! The frontend pushes its command catalog and foreground session over this
//! channel; the supervisor brokers agent command requests back over it.

use serde::{Deserialize, Serialize};

/// One argument of a Positron command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommandArg {
    /// The argument name.
    pub name: String,

    /// A human-readable description of the argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether the argument must be supplied.
    #[serde(default)]
    pub required: bool,

    /// The JSON Schema describing the argument's type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// A Positron command that agents are allowed to run, as reported by
/// `positron.ai.getAgentAllowedCommands()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommand {
    /// The command identifier, e.g. `positronPackages.getPackages`.
    pub id: String,

    /// What the command does.
    pub description: String,

    /// The command's arguments, in positional order.
    #[serde(default)]
    pub args: Vec<AgentCommandArg>,

    /// A description of what the command returns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<String>,
}

/// The frontend's opening frame, sent immediately after the channel connects.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrontendHello {
    /// The version of Positron hosting the frontend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positron_version: Option<String>,

    /// The commands agents may run in this window.
    #[serde(default)]
    pub commands: Vec<AgentCommand>,

    /// The sessions this window holds. Agents reach only these, so that code
    /// never runs somewhere the user cannot see it.
    #[serde(default)]
    pub session_ids: Vec<String>,

    /// The session agents should target when they don't name one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_session_id: Option<String>,

    /// Whether the console history API is available in this window.
    #[serde(default)]
    pub history_api_enabled: bool,

    /// Whether the window had focus when it connected.
    #[serde(default)]
    pub focused: bool,
}

/// A change to the set of sessions a window holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsChanged {
    /// The complete new set; replaces the cached one.
    pub session_ids: Vec<String>,
}

/// A refreshed command catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandsChanged {
    /// The complete new catalog; replaces the cached one.
    pub commands: Vec<AgentCommand>,
}

/// A change of foreground session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForegroundChanged {
    /// The new foreground session, or None if no session has focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// The frontend's answer to a `command_request`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReply {
    /// The `id` of the request being answered.
    pub id: String,

    /// Whether the command ran successfully.
    pub ok: bool,

    /// The command's return value, when `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    /// A machine-readable failure reason, when not `ok`: `not-found`,
    /// `disabled`, or `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// A human-readable failure explanation, when not `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Messages sent from the Positron frontend to the supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrontendMessage {
    /// The frontend has connected and is announcing its capabilities.
    Hello(FrontendHello),

    /// The command catalog has changed.
    CommandsChanged(CommandsChanged),

    /// The foreground session has changed.
    ForegroundChanged(ForegroundChanged),

    /// The set of sessions this window holds has changed.
    SessionsChanged(SessionsChanged),

    /// The window took focus. Several windows of one workspace may share a
    /// frontend, and commands go to whichever of them the user last looked at.
    Focused,

    /// A reply to a previously issued command request.
    CommandReply(CommandReply),
}

/// The agent on whose behalf a command is being run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// The agent's name, from the MCP `clientInfo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The agent's version, from the MCP `clientInfo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// A request for the frontend to run a Positron command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    /// A unique ID; the frontend echoes it in its `command_reply`.
    pub id: String,

    /// The command to run.
    pub command_id: String,

    /// The command's positional arguments.
    #[serde(default)]
    pub args: Vec<serde_json::Value>,

    /// The agent that asked for the command.
    pub agent: AgentIdentity,

    /// How long the supervisor will wait for a reply, in milliseconds.
    pub deadline_ms: u64,
}

/// Messages sent from the supervisor to the Positron frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerFrontendMessage {
    /// Run a Positron command on the agent's behalf.
    CommandRequest(CommandRequest),
}
