//
// handler.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! The MCP tools external agents can call.
//!
//! Kernel tools (`list_sessions`, `execute_code`, `evaluate_code`,
//! `interrupt_session`) are answered entirely inside the supervisor and keep
//! working when Positron is gone. Command tools (`list_positron_commands`,
//! `run_positron_command`) are brokered to a window of the workspace that owns
//! the calling agent's token; listing works from the cache while disconnected,
//! running does not.
//!
//! Every tool is scoped to one workspace: a handler is built per workspace
//! endpoint, and it sees only that workspace's sessions. A supervisor shared by
//! several Positron workspaces therefore looks to each agent like the one it
//! was launched from, and code can never run somewhere the user is not
//! looking.

use std::sync::Arc;
use std::time::Duration;

use kallichore_api::models;
use kcshared::kernel_message::ExecutionAttribution;
use kcshared::mcp_frontend::{AgentCommand, AgentIdentity, CommandRequest};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeRequestParams, InitializeResult, MetaObject, ServerCapabilities,
};
use rmcp::service::RequestContext;
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, RoleServer, ServerHandler};
use serde::Deserialize;
use serde_json::{json, Value};

use super::workspaces::CommandOutcome;
use super::McpState;
use crate::kernel_session::{ExecuteError, ExecuteOptions, KernelSession};

/// The most text one tool result may carry. Beyond this the result is trimmed
/// and flagged, so a runaway cell cannot flood the agent's context.
const MAX_TEXT_BYTES: usize = 32 * 1024;

/// The default execution timeout when the caller doesn't give one.
const DEFAULT_TIMEOUT_S: u32 = 60;

/// The longest execution timeout a caller may ask for.
const MAX_TIMEOUT_S: u32 = 600;

/// How long `run_positron_command` waits for a frontend channel to appear.
/// Long enough to cover a window reload, short enough to stay inside the
/// per-tool timeouts agents impose.
const FRONTEND_CONNECT_WAIT: Duration = Duration::from_secs(15);

/// How long a window has to answer a command request once delivered.
const FRONTEND_REPLY_WAIT: Duration = Duration::from_secs(45);

/// The implementation name agents see, which the server card's reverse-DNS name
/// qualifies rather than replaces.
pub const SERVER_NAME: &str = "positron";

/// The server's display name.
pub const SERVER_TITLE: &str = "Positron";

/// What the server offers, in one line. Reported at `initialize` and repeated
/// in the server card, which must not contradict it.
pub const SERVER_DESCRIPTION: &str = "The user's live Positron interpreter sessions and IDE";

/// Guidance sent to the agent when it connects. Kept short: it is delivered
/// once and clients truncate long instruction blocks.
pub(crate) const INSTRUCTIONS: &str = "\
You are attached to one Positron workspace's live sessions: the workspace whose \
terminal you were launched from. Other workspaces sharing this supervisor are \
invisible to you, which is deliberate. Prefer execute_code and evaluate_code \
over shelling out to Rscript or python: the user's session has their data, \
packages, and working directory already loaded. Call list_sessions first to see \
what is running and which session is in the foreground.

Everything you run is visible in the user's console, attributed to you. Use \
evaluate_code for inspection: it does not enter the session's history or \
advance its execution counter, so it leaves the user's numbering alone. Use \
execute_code for anything with side effects, which the user should be able to \
find in their history afterwards. Output can legitimately be empty, so never \
retry a state-changing call just because nothing came back.

For IDE actions (opening files, starting sessions, installing packages, \
changing settings), call list_positron_commands to find a command, then \
run_positron_command. These need Positron connected; the kernel tools do not. \
No tool ever starts an interpreter on its own.";

/// Added for a client running inside one of the workspace's own kernels, which
/// is the one thing about its situation it cannot work out for itself.
const CALLER_INSTRUCTIONS: &str = "\n\n\
You are running inside one of these sessions yourself, and list_sessions marks \
it is_your_own_session. Every tool works from there except execute_code and \
evaluate_code aimed at that one session: it is busy waiting for whatever you do \
here, so code sent to it would never run. Do that work inline in the code you \
are already running instead.";

/// Serves MCP tool calls for one registered workspace.
#[derive(Clone)]
pub struct PositronMcpHandler {
    state: Arc<McpState>,

    /// The workspace every call is answered for. Fixed when the handler is
    /// built, so no tool can reach another workspace by mistake.
    workspace_id: String,

    /// The session the client is running in, when it reached us at an endpoint
    /// that named one. Set for a client inside one of the workspace's kernels,
    /// such as an `ellmer` or `chatlas` agent the user started in their
    /// console; never set for an agent outside.
    caller_session_id: Option<String>,
}

/// Arguments accepted by `execute_code` and `evaluate_code`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteCodeParams {
    /// The code to run, in the session's language.
    pub code: String,

    /// The session to run in. Defaults to the foreground session, or the only
    /// session when there is exactly one.
    #[serde(default)]
    pub session_id: Option<String>,

    /// How long to wait before giving up and interrupting the kernel, in
    /// seconds. Defaults to 60, maximum 600.
    #[serde(default)]
    pub timeout_s: Option<u32>,

    /// For evaluate_code only: queue behind code that is already running
    /// instead of failing when the session is busy.
    #[serde(default)]
    pub wait: Option<bool>,
}

/// Arguments accepted by `interrupt_session`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InterruptSessionParams {
    /// The session to interrupt. Defaults to the foreground session, or the
    /// only session when there is exactly one.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Arguments accepted by `list_positron_commands`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListCommandsParams {
    /// Case-insensitive text matched against command IDs, descriptions, and
    /// argument names. Omit to list every command.
    #[serde(default)]
    pub query: Option<String>,

    /// The most commands to return.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Arguments accepted by `run_positron_command`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunCommandParams {
    /// The command ID, exactly as reported by list_positron_commands.
    pub command_id: String,

    /// The command's arguments, in the order list_positron_commands gives them.
    #[serde(default)]
    pub args: Option<Vec<Value>>,
}

/// An empty argument list, for tools that take none.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NoParams {}

#[tool_router]
impl PositronMcpHandler {
    /// Create a handler answering for one workspace, and optionally for one
    /// caller within it.
    pub fn new(
        state: Arc<McpState>,
        workspace_id: String,
        caller_session_id: Option<String>,
    ) -> Self {
        Self {
            state,
            workspace_id,
            caller_session_id,
        }
    }

    #[tool(
        name = "list_sessions",
        description = "List the interpreter sessions running in the Positron workspace you are \
                       attached to, with their language, status, working directory, and which one \
                       is in the foreground. Call this before running code so you target the \
                       right session. Sessions belonging to the user's other workspaces are not \
                       listed and cannot be reached. A session marked is_your_own_session is the \
                       one you are running inside, which is the one session you cannot run code \
                       in. Never starts a session.",
        annotations(title = "List sessions", read_only_hint = true)
    )]
    async fn list_sessions(
        &self,
        _context: RequestContext<RoleServer>,
        _params: Parameters<NoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.state.note_request();

        let foreground = self
            .state
            .registry
            .foreground_session(&self.workspace_id)
            .await;
        let (sessions, elsewhere) = self.visible_sessions().await;

        let mut entries = Vec::with_capacity(sessions.len());
        for session in sessions.iter() {
            let active = session.as_active_session().await;
            let mut entry = json!({
                "session_id": active.session_id,
                "display_name": active.display_name,
                "language": active.language,
                "mode": active.session_mode.to_string(),
                "status": active.status.to_string(),
                "working_directory": active.working_directory,
                "queue_length": active.execution_queue.length,
                "is_foreground": Some(&active.session_id) == foreground.as_ref(),
            });
            // Marked only on the one session it applies to, so a listing for an
            // agent outside the kernels looks exactly as it always has.
            if self.is_caller(&active.session_id) {
                entry["is_your_own_session"] = json!(true);
            }
            entries.push(entry);
        }

        let mut body = json!({
            "sessions": entries,
            "workspace": self.workspace().await,
        });
        // Say that other workspaces exist without naming their sessions: an
        // agent told "there should be a session" needs a true answer, and the
        // user's other projects are none of its business.
        if elsewhere > 0 {
            body["sessions_in_other_workspaces"] = json!(elsewhere);
        }
        Ok(self.finish(body, Vec::new()).await)
    }

    #[tool(
        name = "execute_code",
        description = "Run code in one of the user's live interpreter sessions. The code and its \
                       output appear in the user's console and enter their history, exactly as if \
                       they had typed it. Use this for anything with side effects: loading data, \
                       fitting models, writing files, drawing plots. Output can legitimately be \
                       empty, so do not retry on an empty result. Queues behind code that is \
                       already running.",
        annotations(
            title = "Run code in the console",
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    async fn execute_code(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<ExecuteCodeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run_code(context, params, false).await
    }

    #[tool(
        name = "evaluate_code",
        description = "Evaluate an expression in one of the user's live interpreter sessions to \
                       inspect state: variable values, data frame shapes, package versions. The \
                       user sees the code and its output in their console, but it does not enter \
                       the session's history or advance its execution counter, so their numbering \
                       is undisturbed. For anything with side effects use execute_code instead. \
                       Fails when the session is busy unless you pass wait=true.",
        annotations(
            title = "Evaluate code for inspection",
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    async fn evaluate_code(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<ExecuteCodeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run_code(context, params, true).await
    }

    #[tool(
        name = "interrupt_session",
        description = "Interrupt whatever is currently running in a session, as if the user had \
                       pressed the interrupt button. Use this when code you started is taking too \
                       long.",
        annotations(title = "Interrupt a session", destructive_hint = true)
    )]
    async fn interrupt_session(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(params): Parameters<InterruptSessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.state.note_request();

        let session = match self.resolve_session(params.session_id).await {
            Ok(session) => session,
            Err(body) => return Ok(self.error(body).await),
        };
        let session_id = session.connection.session_id.clone();

        match session.interrupt().await {
            Ok(_) => {
                let body = json!({ "status": "ok", "session_id": session_id });
                Ok(self.finish(body, Vec::new()).await)
            }
            Err(e) => {
                let body = json!({
                    "status": "error",
                    "code": "INTERRUPT_FAILED",
                    "session_id": session_id,
                    "message": e.to_string(),
                });
                Ok(self.error(body).await)
            }
        }
    }

    #[tool(
        name = "list_positron_commands",
        description = "Search the Positron IDE commands the user has allowed agents to run: \
                       opening files, starting and restarting sessions, listing and installing \
                       packages, reading settings, focusing panes. Returns each command's ID, \
                       description, and argument schema, which you then pass to \
                       run_positron_command. Works from cache even when Positron is disconnected.",
        annotations(title = "Search Positron commands", read_only_hint = true)
    )]
    async fn list_positron_commands(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(params): Parameters<ListCommandsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.state.note_request();

        let catalog = self.state.registry.commands(&self.workspace_id).await;
        let total = catalog.len();
        let matched: Vec<&AgentCommand> = match params.query.as_deref() {
            Some(query) if !query.trim().is_empty() => catalog
                .iter()
                .filter(|command| matches_query(command, query.trim()))
                .collect(),
            _ => catalog.iter().collect(),
        };

        let returned = matched.len();
        let limit = params.limit.unwrap_or(u32::MAX) as usize;
        let commands: Vec<Value> = matched
            .into_iter()
            .take(limit)
            .map(|command| {
                json!({
                    "id": command.id,
                    "description": command.description,
                    "args": command.args.iter().map(|arg| json!({
                        "name": arg.name,
                        "description": arg.description,
                        "required": arg.required,
                        "schema": arg.schema,
                    })).collect::<Vec<_>>(),
                    "returns": command.returns,
                })
            })
            .collect();

        let body = json!({
            "commands": commands,
            "matched": returned,
            "total": total,
            "truncated": commands.len() < returned,
        });
        Ok(self.finish(body, Vec::new()).await)
    }

    #[tool(
        name = "run_positron_command",
        description = "Run one of the IDE commands returned by list_positron_commands. Requires \
                       Positron to be connected; waits briefly for a window that is reloading. \
                       Use this for IDE actions such as starting a session, which the kernel \
                       tools deliberately never do.",
        annotations(title = "Run a Positron command", open_world_hint = true)
    )]
    async fn run_positron_command(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<RunCommandParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.state.note_request();

        let request = CommandRequest {
            id: uuid::Uuid::new_v4().to_string(),
            command_id: params.command_id.clone(),
            args: params.args.unwrap_or_default(),
            agent: agent_identity(&context),
            deadline_ms: FRONTEND_REPLY_WAIT.as_millis() as u64,
        };

        log::info!(
            "MCP run_positron_command '{}' on workspace '{}'",
            params.command_id,
            self.workspace_id
        );

        let started = std::time::Instant::now();
        let outcome = self
            .state
            .registry
            .run_command(
                &self.workspace_id,
                request,
                FRONTEND_CONNECT_WAIT,
                FRONTEND_REPLY_WAIT,
            )
            .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let (body, is_error) = match outcome {
            CommandOutcome::Replied(reply) if reply.ok => (
                json!({
                    "status": "ok",
                    "command_id": params.command_id,
                    "result": reply.result,
                    "elapsed_ms": elapsed_ms,
                }),
                false,
            ),
            CommandOutcome::Replied(reply) => (
                json!({
                    "status": "error",
                    "command_id": params.command_id,
                    "reason": reply.reason.unwrap_or_else(|| "error".to_string()),
                    "message": reply.message,
                    "elapsed_ms": elapsed_ms,
                }),
                true,
            ),
            CommandOutcome::Disconnected { since } => (
                json!({
                    "status": "error",
                    "command_id": params.command_id,
                    "reason": "POSITRON_DISCONNECTED",
                    "message": "Positron is not connected, so IDE commands cannot run. The kernel \
                                tools (list_sessions, execute_code, evaluate_code, \
                                interrupt_session) still work. Ask the user to reopen Positron if \
                                you need this command.",
                    "positron_disconnected_since": since,
                    "elapsed_ms": elapsed_ms,
                }),
                true,
            ),
            CommandOutcome::TimedOut => (
                json!({
                    "status": "error",
                    "command_id": params.command_id,
                    "reason": "timeout",
                    "message": format!(
                        "Positron did not answer within {} seconds",
                        FRONTEND_REPLY_WAIT.as_secs()
                    ),
                    "elapsed_ms": elapsed_ms,
                }),
                true,
            ),
            CommandOutcome::UnknownWorkspace => (
                json!({
                    "status": "error",
                    "command_id": params.command_id,
                    "reason": "POSITRON_DISCONNECTED",
                    "message": "The Positron workspace this token belongs to is no longer registered.",
                    "elapsed_ms": elapsed_ms,
                }),
                true,
            ),
        };

        Ok(if is_error {
            self.error(body).await
        } else {
            self.finish(body, Vec::new()).await
        })
    }
}

impl PositronMcpHandler {
    /// Shared body of `execute_code` and `evaluate_code`.
    ///
    /// The two differ only in `store_history`: an evaluation stays out of the
    /// session's history and leaves its execution counter alone. Neither uses
    /// the Jupyter `silent` flag, which would suppress the whole iopub stream
    /// -- both the `execute_result` the agent asked for and the echo the user
    /// needs in order to see what the agent is doing in their session.
    async fn run_code(
        &self,
        context: RequestContext<RoleServer>,
        params: ExecuteCodeParams,
        inspecting: bool,
    ) -> Result<CallToolResult, ErrorData> {
        self.state.note_request();

        let tool = if inspecting {
            "evaluate_code"
        } else {
            "execute_code"
        };
        let session = match self.resolve_session(params.session_id).await {
            Ok(session) => session,
            Err(body) => return Ok(self.error(body).await),
        };
        let session_id = session.connection.session_id.clone();

        // Running code is the one thing a client inside a kernel cannot ask of
        // its own session: that session is executing the call it is waiting on,
        // so the code would queue behind it and the only way out would be the
        // timeout interrupting the client's own work. Every other tool is
        // fine from there, including interrupting.
        if self.is_caller(&session_id) {
            let body = json!({
                "status": "error",
                "code": "SESSION_IS_CALLER",
                "session_id": session_id,
                "message": format!(
                    "Session '{}' is the one you are running in, so it is busy waiting for this \
                     call and cannot run your code. Run the code inline instead, or name another \
                     session; list_sessions marks this one is_your_own_session.",
                    session_id
                ),
            });
            return Ok(self.error(body).await);
        }

        // A queued evaluation looks like a hang to an agent, which then
        // retries. Refuse instead, unless it asked to wait.
        if inspecting && !params.wait.unwrap_or(false) {
            let status = { session.state.read().await.status };
            if status == models::Status::Busy {
                let body = json!({
                    "status": "error",
                    "code": "RUNTIME_BUSY",
                    "session_id": session_id,
                    "message": "The session is busy. Pass wait=true to queue behind the running \
                                code, or use execute_code, which always queues.",
                });
                return Ok(self.error(body).await);
            }
        }

        let agent = agent_identity(&context);
        let timeout_s = params
            .timeout_s
            .unwrap_or(DEFAULT_TIMEOUT_S)
            .clamp(1, MAX_TIMEOUT_S);

        log::info!(
            "MCP {} on session '{}' ({} bytes of code)",
            tool,
            session_id,
            params.code.len()
        );
        log::debug!("MCP {} code: {}", tool, params.code);

        let options = ExecuteOptions {
            code: params.code,
            silent: false,
            store_history: !inspecting,
            stop_on_error: true,
            timeout: Some(Duration::from_secs(timeout_s as u64)),
            attribution: Some(ExecutionAttribution {
                source: "agent".to_string(),
                agent_name: agent.name.clone(),
                agent_version: agent.version.clone(),
                workspace_id: self.workspace_id.clone(),
                tool: tool.to_string(),
            }),
        };

        let started = std::time::Instant::now();
        let result = session.execute_collect(options).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let (mut body, images, is_error) = match result {
            Ok(reply) => {
                let (rendered, images) = render_reply(&reply);
                let is_error = reply.status == models::ExecuteReplyStatus::Error;
                (rendered, images, is_error)
            }
            Err(ExecuteError::Timeout) => (
                json!({
                    "status": "timed_out",
                    "message": format!(
                        "Execution did not finish within {} seconds; the session was interrupted.",
                        timeout_s
                    ),
                }),
                Vec::new(),
                true,
            ),
            Err(ExecuteError::NotReady(message)) => (
                json!({ "status": "error", "code": "SESSION_NOT_READY", "message": message }),
                Vec::new(),
                true,
            ),
            Err(ExecuteError::SendFailed(message)) => (
                json!({ "status": "error", "code": "SEND_FAILED", "message": message }),
                Vec::new(),
                true,
            ),
            Err(ExecuteError::ChannelClosed) => (
                json!({
                    "status": "interrupted",
                    "code": "CHANNEL_CLOSED",
                    "message": "The kernel's message channel closed while the code was running.",
                }),
                Vec::new(),
                true,
            ),
        };

        if let Some(object) = body.as_object_mut() {
            object.insert("session_id".into(), json!(session_id));
            object.insert("elapsed_ms".into(), json!(elapsed_ms));
        }

        Ok(if is_error {
            let mut result = self.error(body).await;
            result.content.extend(images);
            result
        } else {
            self.finish(body, images).await
        })
    }

    /// Pick the session a tool call should target.
    ///
    /// An explicit ID wins; otherwise the foreground session Positron
    /// reported; otherwise the only session, if there is exactly one. Sessions
    /// are never started implicitly, and only the calling workspace's own
    /// sessions are ever candidates.
    ///
    async fn resolve_session(&self, requested: Option<String>) -> Result<KernelSession, Value> {
        let (sessions, elsewhere) = self.visible_sessions().await;

        if let Some(session_id) = requested {
            if let Some(session) = sessions
                .into_iter()
                .find(|s| s.connection.session_id == session_id)
            {
                return Ok(session);
            }
            // Tell an agent that named another workspace's session why it can't
            // have it, so it stops rather than retrying the same ID.
            let exists = self
                .state
                .sessions()
                .iter()
                .any(|s| s.connection.session_id == session_id);
            return Err(if exists {
                json!({
                    "status": "error",
                    "code": "SESSION_NOT_VISIBLE",
                    "message": format!(
                        "Session '{}' belongs to one of the user's other Positron workspaces. \
                         You can only reach the sessions of the workspace you were launched \
                         from.",
                        session_id
                    ),
                })
            } else {
                json!({
                    "status": "error",
                    "code": "SESSION_NOT_FOUND",
                    "message": format!("No session with ID '{}'", session_id),
                })
            });
        }

        if let Some(foreground) = self
            .state
            .registry
            .foreground_session(&self.workspace_id)
            .await
        {
            if let Some(session) = sessions
                .iter()
                .find(|s| s.connection.session_id == foreground)
            {
                return Ok(session.clone());
            }
        }

        match sessions.len() {
            1 => Ok(sessions.into_iter().next().unwrap()),
            0 => Err(json!({
                "status": "error",
                "code": "NO_SESSION_SELECTED",
                "message": if elsewhere > 0 {
                    "This Positron workspace has no interpreter sessions running; the ones in \
                     the user's other workspaces are not yours to use. Start one with \
                     run_positron_command using \
                     workbench.action.language.runtime.startNewConsoleSession."
                } else {
                    "No interpreter sessions are running. Start one with run_positron_command \
                     using workbench.action.language.runtime.startNewConsoleSession."
                },
                "candidates": [],
            })),
            _ => {
                let mut candidates = Vec::with_capacity(sessions.len());
                for session in sessions.iter() {
                    let active = session.as_active_session().await;
                    candidates.push(json!({
                        "session_id": active.session_id,
                        "display_name": active.display_name,
                        "language": active.language,
                    }));
                }
                Err(json!({
                    "status": "error",
                    "code": "NO_SESSION_SELECTED",
                    "message": "Several sessions are running and none is in the foreground. Pass \
                                session_id explicitly.",
                    "candidates": candidates,
                }))
            }
        }
    }

    /// Whether a session is the one this client is running in.
    fn is_caller(&self, session_id: &str) -> bool {
        self.caller_session_id.as_deref() == Some(session_id)
    }

    /// The sessions the calling workspace can reach, and how many of the
    /// supervisor's other sessions belong to the user's other workspaces.
    ///
    /// A session belongs to the workspace that created it, which names itself
    /// when it asks for the session. A workspace also reaches sessions it
    /// reports holding as long as no other registered workspace owns them,
    /// which covers sessions that were already running when it registered, and
    /// sessions left behind by a workspace that has gone for good.
    async fn visible_sessions(&self) -> (Vec<KernelSession>, usize) {
        let claimed = self.state.registry.session_ids(&self.workspace_id).await;
        let registered = self.state.registry.registered_ids().await;
        let sessions = self.state.sessions();
        let total = sessions.len();
        let visible: Vec<KernelSession> = sessions
            .into_iter()
            .filter(|session| {
                let owner = session.model.workspace_id.as_deref();
                if owner == Some(self.workspace_id.as_str()) {
                    return true;
                }
                let unowned = owner.is_none_or(|owner| !registered.contains(owner));
                unowned
                    && claimed
                        .iter()
                        .any(|id| *id == session.connection.session_id)
            })
            .collect();
        let elsewhere = total - visible.len();
        (visible, elsewhere)
    }

    /// The workspace this handler answers for, as agents and their users see
    /// it.
    async fn workspace(&self) -> Value {
        json!({
            "id": self.workspace_id,
            "name": self.state.registry.display_name(&self.workspace_id).await,
        })
    }

    /// Add the connection state every tool result carries.
    async fn decorate(&self, body: &mut Value) {
        let (connected, since) = self
            .state
            .registry
            .connection_state(&self.workspace_id)
            .await;
        let Some(object) = body.as_object_mut() else {
            return;
        };
        object.insert("positron_connected".into(), json!(connected));
        if !connected {
            object.insert("positron_disconnected_since".into(), json!(since));
        }
        object.insert(
            "positron_version".into(),
            json!(
                self.state
                    .registry
                    .positron_version(&self.workspace_id)
                    .await
            ),
        );
        object.insert("server_version".into(), json!(self.state.server_version()));
    }

    /// Build a successful result.
    async fn finish(&self, mut body: Value, images: Vec<ContentBlock>) -> CallToolResult {
        self.decorate(&mut body).await;
        let mut result = CallToolResult::structured(body);
        result.content.extend(images);
        result.meta = Some(self.meta().await);
        result
    }

    /// Build a tool-level error result.
    async fn error(&self, mut body: Value) -> CallToolResult {
        self.decorate(&mut body).await;
        let mut result = CallToolResult::structured_error(body);
        result.meta = Some(self.meta().await);
        result
    }

    /// The `_meta` block attached to every result.
    async fn meta(&self) -> MetaObject {
        let (connected, since) = self
            .state
            .registry
            .connection_state(&self.workspace_id)
            .await;
        let mut meta = serde_json::Map::new();
        meta.insert("positron_connected".into(), json!(connected));
        if !connected {
            meta.insert("positron_disconnected_since".into(), json!(since));
        }
        meta.insert("positron_workspace".into(), self.workspace().await);
        MetaObject(meta)
    }
}

impl PositronMcpHandler {
    /// Every tool the server publishes, available without a handler, so that
    /// the stdio bridge can list them while Positron is not running.
    pub fn tool_list() -> Vec<rmcp::model::Tool> {
        Self::tool_router().list_all()
    }
}

/// What the server reports about itself at `initialize`, with the given
/// instructions.
pub fn server_info(instructions: String) -> InitializeResult {
    InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(
            Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION"))
                .with_title(SERVER_TITLE)
                .with_description(SERVER_DESCRIPTION),
        )
        .with_instructions(instructions)
}

#[tool_handler]
impl ServerHandler for PositronMcpHandler {
    fn get_info(&self) -> InitializeResult {
        server_info(match &self.caller_session_id {
            Some(_) => format!("{}{}", INSTRUCTIONS, CALLER_INSTRUCTIONS),
            None => INSTRUCTIONS.to_string(),
        })
    }

    /// Announce the agent, then negotiate as the default implementation does.
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        log::info!(
            "MCP client '{}' {} connected (protocol {})",
            request.client_info.name,
            request.client_info.version,
            request.protocol_version
        );
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }

    /// Every tool call passes through here, so this is where a request and its
    /// outcome are logged. Replaces the dispatcher `#[tool_handler]` would
    /// otherwise generate; the body is that dispatcher plus the logging.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let tool = request.name.clone();
        let agent = agent_identity(&context);
        let agent = agent.name.unwrap_or_else(|| "unknown agent".to_string());
        log::info!("MCP {} requested by {}", tool, agent);

        let started = std::time::Instant::now();
        let response = Self::tool_router()
            .call(ToolCallContext::new(self, request, context))
            .await;
        let elapsed_ms = started.elapsed().as_millis();

        match &response {
            Ok(CallToolResponse::Complete(result)) if result.is_error.unwrap_or(false) => {
                log::info!(
                    "MCP {} for {} returned an error in {}ms: {}",
                    tool,
                    agent,
                    elapsed_ms,
                    result
                        .structured_content
                        .as_ref()
                        .map(|body| body.to_string())
                        .unwrap_or_default()
                );
            }
            Ok(CallToolResponse::Complete(_)) => {
                log::info!("MCP {} for {} succeeded in {}ms", tool, agent, elapsed_ms);
            }
            Ok(_) => log::info!(
                "MCP {} for {} needs more from the client after {}ms",
                tool,
                agent,
                elapsed_ms
            ),
            Err(e) => log::warn!(
                "MCP {} for {} failed in {}ms: {}",
                tool,
                agent,
                elapsed_ms,
                e
            ),
        }

        response
    }
}

/// The agent behind a request, from the MCP `clientInfo`.
fn agent_identity(context: &RequestContext<RoleServer>) -> AgentIdentity {
    match context.client_info() {
        Some(info) => AgentIdentity {
            name: Some(info.name.clone()),
            version: Some(info.version.clone()),
        },
        None => AgentIdentity {
            name: None,
            version: None,
        },
    }
}

/// Case-insensitive substring match over a command's ID, description, and
/// argument names. At catalog sizes of a few dozen entries this beats fuzzy
/// ranking for predictability.
fn matches_query(command: &AgentCommand, query: &str) -> bool {
    let query = query.to_lowercase();
    command.id.to_lowercase().contains(&query)
        || command.description.to_lowercase().contains(&query)
        || command
            .args
            .iter()
            .any(|arg| arg.name.to_lowercase().contains(&query))
}

/// Turn an execute reply into the structured result and image blocks an agent
/// sees.
fn render_reply(reply: &models::ExecuteReply) -> (Value, Vec<ContentBlock>) {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut images = Vec::new();
    let mut error: Option<Value> = None;

    for output in &reply.output {
        match output.r#type {
            models::ExecuteOutputType::Stream => {
                let text = output.text.as_deref().unwrap_or("");
                if output.stream_name.as_deref() == Some("stderr") {
                    stderr.push_str(text);
                } else {
                    stdout.push_str(text);
                }
            }
            models::ExecuteOutputType::DisplayData => {
                if let Some(data) = &output.data {
                    collect_images(data, &mut images);
                }
            }
            models::ExecuteOutputType::Error => {
                error = Some(json!({
                    "name": output.error_name,
                    "message": output.error_message,
                    "traceback": output.error_traceback,
                }));
            }
        }
    }

    if error.is_none() && reply.error_name.is_some() {
        error = Some(json!({
            "name": reply.error_name,
            "message": reply.error_message,
            "traceback": reply.error_traceback,
        }));
    }

    let mut result: Option<serde_json::Map<String, Value>> = None;
    if let Some(data) = &reply.data {
        collect_images(data, &mut images);
        result = Some(
            data.iter()
                .map(|(mime, value)| (mime.clone(), json!(value)))
                .collect(),
        );
    }

    let mut budget = MAX_TEXT_BYTES;
    let (stdout, cut_stdout) = take_budget(stdout, &mut budget);
    let (stderr, cut_stderr) = take_budget(stderr, &mut budget);
    let mut cut_result = false;
    if let Some(result) = result.as_mut() {
        for value in result.values_mut() {
            if let Some(text) = value.as_str() {
                let (trimmed, cut) = take_budget(text.to_string(), &mut budget);
                cut_result |= cut;
                *value = json!(trimmed);
            }
        }
    }

    let body = json!({
        "status": match reply.status {
            models::ExecuteReplyStatus::Ok => "ok",
            models::ExecuteReplyStatus::Error => "error",
        },
        "execution_count": reply.execution_count,
        "stdout": stdout,
        "stderr": stderr,
        "result": result,
        "images": images.len(),
        "error": error,
        "truncated": cut_stdout || cut_stderr || cut_result,
    });

    (body, images)
}

/// Pull any renderable images out of a MIME bundle.
fn collect_images(
    data: &std::collections::HashMap<String, String>,
    images: &mut Vec<ContentBlock>,
) {
    for (mime, value) in data {
        if mime == "image/png" || mime == "image/jpeg" {
            images.push(ContentBlock::image(value.clone(), mime.clone()));
        }
    }
}

/// Trim `text` to what is left of the budget, reporting whether it was cut.
fn take_budget(text: String, budget: &mut usize) -> (String, bool) {
    if text.len() <= *budget {
        *budget -= text.len();
        return (text, false);
    }
    // Cut on a character boundary so the result stays valid UTF-8.
    let mut end = *budget;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let trimmed = text[..end].to_string();
    *budget = 0;
    (trimmed, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kcshared::mcp_frontend::AgentCommandArg;

    fn command(id: &str, description: &str, arg: &str) -> AgentCommand {
        AgentCommand {
            id: id.to_string(),
            description: description.to_string(),
            args: vec![AgentCommandArg {
                name: arg.to_string(),
                description: None,
                required: true,
                schema: None,
            }],
            returns: None,
        }
    }

    #[test]
    fn query_matches_id_description_and_arg_names() {
        let cmd = command(
            "positronPackages.getPackages",
            "List installed packages",
            "filter",
        );
        assert!(matches_query(&cmd, "packages"));
        assert!(matches_query(&cmd, "PACKAGES"));
        assert!(matches_query(&cmd, "installed"));
        assert!(matches_query(&cmd, "filter"));
        assert!(!matches_query(&cmd, "notebook"));
    }

    #[test]
    fn budget_trims_on_character_boundaries() {
        let mut budget = 4;
        let (text, cut) = take_budget("aé…".to_string(), &mut budget);
        assert!(cut);
        assert_eq!(text, "aé");
        assert_eq!(budget, 0);
    }

    #[test]
    fn budget_is_shared_across_fields() {
        let mut budget = 10;
        let (first, cut) = take_budget("12345".to_string(), &mut budget);
        assert_eq!(first, "12345");
        assert!(!cut);
        let (second, cut) = take_budget("1234567".to_string(), &mut budget);
        assert_eq!(second, "12345");
        assert!(cut);
    }
}
