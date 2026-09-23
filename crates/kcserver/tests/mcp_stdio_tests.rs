//
// mcp_stdio_tests.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Integration tests for `kcserver mcp-stdio`, the bridge that lets an agent
//! start Positron's MCP server as a child process.
//!
//! Each test runs the real bridge binary, driven over its stdin and stdout,
//! against a real supervisor. The connections directory is written the way
//! Positron writes it.

#[path = "common/mod.rs"]
mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::mcp::{SimulatedFrontend, StdioAgent};
use common::test_utils::{create_session_with_client, create_test_session};
use common::TestServer;
use kallichore_api::models::{McpClient, McpWorkspace, McpWorkspaceRegistration};
use kallichore_api::{DeregisterMcpWorkspaceResponse, ServerStatusResponse};
use kcshared::mcp_frontend::{CommandReply, FrontendHello, FrontendMessage};
use kcshared::port_picker::pick_unused_tcp_port;
use serde_json::{json, Value};

/// A workspace as Positron's connections files describe it.
struct Listed<'a> {
    id: &'a str,
    url: String,
    token: &'a str,
    folders: Vec<&'a Path>,
    last_active: Option<&'a str>,
}

impl<'a> Listed<'a> {
    /// A registered workspace, listed with the given folders.
    fn registered(workspace: &'a McpWorkspace, folders: Vec<&'a Path>) -> Self {
        Self {
            id: &workspace.workspace_id,
            url: workspace.url.clone(),
            token: &workspace.token,
            folders,
            last_active: None,
        }
    }

    /// The same, last active at the given time.
    fn active_at(mut self, last_active: &'a str) -> Self {
        self.last_active = Some(last_active);
        self
    }
}

/// Write Positron's connections index and one descriptor per workspace.
fn write_connections(dir: &Path, workspaces: &[Listed]) {
    let descriptors = dir.join("workspaces");
    std::fs::create_dir_all(&descriptors).unwrap();
    let mut index = serde_json::Map::new();
    for workspace in workspaces {
        let descriptor = descriptors.join(format!("{}.json", workspace.id));
        std::fs::write(
            &descriptor,
            json!({
                "version": 1,
                "workspaceId": workspace.id,
                "displayName": workspace.id,
                "url": workspace.url,
                "token": workspace.token,
            })
            .to_string(),
        )
        .unwrap();
        index.insert(
            workspace.id.to_string(),
            json!({
                "displayName": workspace.id,
                "url": workspace.url,
                "descriptor": descriptor,
                "folders": workspace.folders,
                "lastActive": workspace.last_active,
            }),
        );
    }
    std::fs::write(
        dir.join("connections.json"),
        json!({ "version": 1, "workspaces": index }).to_string(),
    )
    .unwrap();
}

/// Create a session belonging to a workspace.
async fn create_session(server: &TestServer, session_id: &str, owner: &str) -> String {
    let client = server.create_client().await;
    let mut session = create_test_session(session_id.to_string(), "python3");
    session.workspace_id = Some(owner.to_string());
    create_session_with_client(&client, session).await
}

/// The workspace a bridge reports it is attached to, from `list_sessions`.
async fn attached_workspace(agent: &mut StdioAgent) -> Value {
    let result = agent.call_tool("list_sessions", json!({})).await;
    assert!(!result.is_error, "{:?}", result);
    result.field("workspace")["id"].clone()
}

/// The names of the clients in a list.
fn names(clients: &[McpClient]) -> Vec<&str> {
    clients
        .iter()
        .map(|client| client.name.as_deref().unwrap_or(""))
        .collect()
}

/// The clients the supervisor's status lists for its only workspace.
async fn listed_clients(server: &TestServer) -> Vec<McpClient> {
    let client = server.create_client().await;
    let status = match client.server_status().await.unwrap() {
        ServerStatusResponse::ServerStatusAndInformation(status) => status,
        other => panic!("Unexpected status response: {:?}", other),
    };
    status.mcp.unwrap().workspaces[0].clients.clone()
}

/// A token of the shape the server issues.
fn fake_token() -> String {
    "ab".repeat(32)
}

#[tokio::test]
async fn test_bridge_relays_using_the_environment() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Env Workspace", None).await;
    let session = create_session(&server, "env-session", &workspace.workspace_id).await;
    let cwd = tempfile::tempdir().unwrap();

    let mut agent = StdioAgent::spawn(
        &[],
        &[
            ("POSITRON_MCP_URL", &workspace.url),
            ("POSITRON_MCP_TOKEN", &workspace.token),
        ],
        cwd.path(),
    );
    let info = agent.initialize().await;
    assert_eq!(info["serverInfo"]["name"], "positron", "{}", info);
    assert!(
        !info["instructions"]
            .as_str()
            .unwrap()
            .contains("not reachable"),
        "The instructions should be the server's own: {}",
        info
    );

    let tools = agent.tool_names().await;
    assert_eq!(tools.len(), 8, "{:?}", tools);

    let result = agent.call_tool("list_sessions", json!({})).await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("sessions")[0]["session_id"], json!(session));

    // A response to a request of the server's is relayed, not answered.
    agent
        .send(json!({ "jsonrpc": "2.0", "id": "server-1", "result": {} }))
        .await;
    let ping = agent.send_request("ping", json!({})).await;
    assert_eq!(agent.next_message().await["id"], json!(ping));

    assert!(agent.shut_down().await.success());
}

#[tokio::test]
async fn test_bridge_finds_the_workspace_by_folder() {
    let server = TestServer::start().await;
    let first = server.register_mcp_workspace("First", None).await;
    let second = server.register_mcp_workspace("Second", None).await;

    let root = tempfile::tempdir().unwrap();
    let first_folder = root.path().join("first");
    let nested = first_folder.join("analysis").join("scripts");
    let second_folder = root.path().join("second");
    let elsewhere = root.path().join("elsewhere");
    for dir in [&nested, &second_folder, &elsewhere] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let connections = root.path().join("connections");
    write_connections(
        &connections,
        &[
            Listed::registered(&first, vec![&first_folder]),
            Listed::registered(&second, vec![&second_folder]),
        ],
    );
    let args = ["--connections", connections.to_str().unwrap()];

    // Anywhere inside a workspace's folder reaches that workspace.
    let mut agent = StdioAgent::spawn(&args, &[], &nested);
    agent.initialize().await;
    assert_eq!(
        attached_workspace(&mut agent).await,
        json!(first.workspace_id)
    );

    let mut agent = StdioAgent::spawn(&args, &[], &second_folder);
    agent.initialize().await;
    assert_eq!(
        attached_workspace(&mut agent).await,
        json!(second.workspace_id)
    );

    // An explicit workspace wins over the working directory.
    let explicit = [
        "--connections",
        connections.to_str().unwrap(),
        "--workspace",
        &second.workspace_id,
    ];
    let mut agent = StdioAgent::spawn(&explicit, &[], &nested);
    agent.initialize().await;
    assert_eq!(
        attached_workspace(&mut agent).await,
        json!(second.workspace_id)
    );

    // Outside every workspace the bridge still starts and lists its tools, and
    // says why nothing works rather than failing to start.
    let mut agent = StdioAgent::spawn(&args, &[], &elsewhere);
    agent.initialize().await;
    assert_eq!(agent.tool_names().await.len(), 8);
    let result = agent.call_tool("list_sessions", json!({})).await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("NO_WORKSPACE"));

    // Of two workspaces listing the same folder, the one used last wins.
    write_connections(
        &connections,
        &[
            Listed::registered(&first, vec![&second_folder]).active_at("2026-09-22T10:00:00.000Z"),
            Listed::registered(&second, vec![&second_folder]).active_at("2026-09-22T09:00:00.000Z"),
        ],
    );
    let mut agent = StdioAgent::spawn(&args, &[], &second_folder);
    agent.initialize().await;
    assert_eq!(
        attached_workspace(&mut agent).await,
        json!(first.workspace_id)
    );
}

#[tokio::test]
async fn test_bridge_forwards_the_agents_name_and_runs_requests_concurrently() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Busy Workspace", None).await;
    let mut channel = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("2026.09.0".to_string()),
        ..Default::default()
    }));

    let cwd = tempfile::tempdir().unwrap();
    let mut agent = StdioAgent::spawn(
        &[],
        &[
            ("POSITRON_MCP_URL", &workspace.url),
            ("POSITRON_MCP_TOKEN", &workspace.token),
        ],
        cwd.path(),
    )
    .named("claude-code");
    agent.initialize().await;

    // A command the window holds on to, then a listing behind it.
    let command = agent
        .send_request(
            "tools/call",
            json!({ "name": "run_positron_command",
                    "arguments": { "command_id": "vscode.open", "args": ["file:///x.R"] } }),
        )
        .await;
    let request = channel.next_command(Duration::from_secs(10)).await;
    assert_eq!(
        request.agent.name.as_deref(),
        Some("claude-code"),
        "The server should hear the agent's name, not the bridge's"
    );

    let listing = agent
        .send_request(
            "tools/call",
            json!({ "name": "list_sessions", "arguments": {} }),
        )
        .await;
    let first = agent.next_message().await;
    assert_eq!(
        first["id"],
        json!(listing),
        "The listing should not wait for the command: {}",
        first
    );

    channel.reply(CommandReply {
        id: request.id,
        ok: true,
        result: Some(json!({ "opened": true })),
        reason: None,
        message: None,
    });
    let reply = agent.response(command).await;
    assert_eq!(
        reply["result"]["structuredContent"]["result"],
        json!({ "opened": true }),
        "{}",
        reply
    );
}

#[tokio::test]
async fn test_bridge_survives_a_supervisor_restart() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Restarted", None).await;

    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let connections = root.path().join("connections");
    write_connections(
        &connections,
        &[Listed::registered(&workspace, vec![&folder])],
    );

    // One agent finds the workspace by folder; the other was started from a
    // terminal that still holds the old endpoint.
    let mut by_folder = StdioAgent::spawn(
        &["--connections", connections.to_str().unwrap()],
        &[],
        &folder,
    );
    by_folder.initialize().await;
    attached_workspace(&mut by_folder).await;

    let mut by_env = StdioAgent::spawn(
        &["--connections", connections.to_str().unwrap()],
        &[
            ("POSITRON_MCP_URL", &workspace.url),
            ("POSITRON_MCP_TOKEN", &workspace.token),
        ],
        root.path(),
    );
    by_env.initialize().await;
    attached_workspace(&mut by_env).await;

    // The listener goes away and comes back on another port, as it does when
    // the supervisor is replaced, and Positron rewrites the connections files.
    let client = server.create_client().await;
    match client
        .deregister_mcp_workspace(workspace.workspace_id.clone())
        .await
        .unwrap()
    {
        DeregisterMcpWorkspaceResponse::WorkspaceDeregistered => {}
        other => panic!("Unexpected deregistration response: {:?}", other),
    }
    let mut registration = McpWorkspaceRegistration::new("Restarted".to_string());
    registration.workspace_id = Some(workspace.workspace_id.clone());
    registration.token = Some(workspace.token.clone());
    registration.preferred_port = Some(pick_unused_tcp_port().unwrap() as i32);
    let moved = server.register_mcp_workspace_as(registration).await;
    assert_ne!(moved.port, workspace.port);
    write_connections(&connections, &[Listed::registered(&moved, vec![&folder])]);

    // Neither agent reconnects; the next call just works.
    assert_eq!(
        attached_workspace(&mut by_folder).await,
        json!(workspace.workspace_id)
    );
    assert_eq!(
        attached_workspace(&mut by_env).await,
        json!(workspace.workspace_id)
    );
}

#[tokio::test]
async fn test_bridge_ignores_connections_of_another_version() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Future", None).await;
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let connections = root.path().join("connections");
    write_connections(
        &connections,
        &[Listed::registered(&workspace, vec![&folder])],
    );

    // A Positron that writes a shape this bridge does not know.
    let index_path = connections.join("connections.json");
    let mut index: Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    index["version"] = json!(2);
    std::fs::write(&index_path, index.to_string()).unwrap();

    let mut agent = StdioAgent::spawn(
        &["--connections", connections.to_str().unwrap()],
        &[],
        &folder,
    );
    agent.initialize().await;
    let result = agent.call_tool("list_sessions", json!({})).await;
    assert_eq!(result.field("code"), &json!("NO_WORKSPACE"));
}

#[tokio::test]
async fn test_bridge_started_before_positron_connects_when_it_arrives() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let connections = root.path().join("connections");

    // A workspace Positron registered in an earlier run, whose endpoint is no
    // longer listening.
    let workspace_id = "late-workspace-abc123";
    let token = fake_token();
    let dead_port = pick_unused_tcp_port().unwrap();
    write_connections(
        &connections,
        &[Listed {
            id: workspace_id,
            url: format!("http://127.0.0.1:{}/mcp/w/{}", dead_port, workspace_id),
            token: &token,
            folders: vec![&folder],
            last_active: None,
        }],
    );

    let mut agent = StdioAgent::spawn(
        &["--connections", connections.to_str().unwrap()],
        &[],
        &folder,
    );
    let info = agent.initialize().await;
    assert_eq!(info["serverInfo"]["name"], "positron", "{}", info);
    assert!(
        info["instructions"]
            .as_str()
            .unwrap()
            .contains("not reachable"),
        "{}",
        info
    );
    assert_eq!(agent.tool_names().await.len(), 8);
    let result = agent.call_tool("list_sessions", json!({})).await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("POSITRON_NOT_RUNNING"));

    // Positron starts and registers the workspace again.
    let server = TestServer::start().await;
    let mut registration = McpWorkspaceRegistration::new("Late".to_string());
    registration.workspace_id = Some(workspace_id.to_string());
    registration.token = Some(token.clone());
    let workspace = server.register_mcp_workspace_as(registration).await;
    write_connections(
        &connections,
        &[Listed::registered(&workspace, vec![&folder])],
    );

    assert_eq!(attached_workspace(&mut agent).await, json!(workspace_id));
}

#[tokio::test]
async fn test_a_bridge_inside_a_kernel_cannot_run_code_in_that_kernel() {
    let server = TestServer::start().await;
    let workspace = server
        .register_mcp_workspace("Kernel Workspace", None)
        .await;
    let inside = create_session(&server, "kernel-with-client", &workspace.workspace_id).await;

    // The endpoint the supervisor puts in that kernel's environment.
    let url = format!("{}/s/{}", workspace.url, inside);
    let cwd = tempfile::tempdir().unwrap();
    let mut agent = StdioAgent::spawn(
        &[],
        &[
            ("POSITRON_MCP_URL", &url),
            ("POSITRON_MCP_TOKEN", &workspace.token),
        ],
        cwd.path(),
    );
    agent.initialize().await;

    let result = agent
        .call_tool("execute_code", json!({ "code": "1", "session_id": inside }))
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("SESSION_IS_CALLER"));
}

#[tokio::test]
async fn test_connected_bridges_are_listed_while_they_run() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Listed", None).await;
    let mut window = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    window
        .wait_for_clients(Duration::from_secs(5), |clients| clients.is_empty())
        .await;

    let cwd = tempfile::tempdir().unwrap();
    let env = [
        ("POSITRON_MCP_URL", workspace.url.as_str()),
        ("POSITRON_MCP_TOKEN", workspace.token.as_str()),
    ];
    let mut claude = StdioAgent::spawn(&[], &env, cwd.path()).named("claude-code");
    claude.initialize().await;

    let listed = window
        .wait_for_clients(Duration::from_secs(10), |clients| clients.len() == 1)
        .await;
    let expected_dir = std::fs::canonicalize(cwd.path()).unwrap();
    assert_eq!(
        (
            listed[0].name.as_deref(),
            listed[0].version.as_deref(),
            listed[0].pid,
            listed[0].working_directory.as_deref().map(Path::new),
        ),
        (
            Some("claude-code"),
            Some("1.0.0"),
            Some(claude.pid() as i32),
            Some(expected_dir.as_path()),
        )
    );
    assert_eq!(names(&listed_clients(&server).await), vec!["claude-code"]);

    let mut codex = StdioAgent::spawn(&[], &env, cwd.path()).named("codex");
    codex.initialize().await;
    let listed = window
        .wait_for_clients(Duration::from_secs(10), |clients| clients.len() == 2)
        .await;
    assert_eq!(names(&listed), vec!["claude-code", "codex"]);

    // An agent that crashes takes its bridge with it without a word. The
    // connection closing is enough to notice, well before the next keepalive.
    let killed = Instant::now();
    claude.kill().await;
    let listed = window
        .wait_for_clients(Duration::from_secs(10), |clients| clients.len() == 1)
        .await;
    assert_eq!(names(&listed), vec!["codex"]);
    assert!(
        killed.elapsed() < Duration::from_secs(5),
        "A dead bridge should be noticed promptly, took {:?}",
        killed.elapsed()
    );

    // One that shuts down cleanly leaves as it goes.
    assert!(codex.shut_down().await.success());
    window
        .wait_for_clients(Duration::from_secs(5), |clients| clients.is_empty())
        .await;
}

#[tokio::test]
async fn test_an_idle_bridge_is_listed_once_positron_arrives() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let connections = root.path().join("connections");

    let workspace_id = "idle-workspace-abc123";
    let token = fake_token();
    let dead_port = pick_unused_tcp_port().unwrap();
    write_connections(
        &connections,
        &[Listed {
            id: workspace_id,
            url: format!("http://127.0.0.1:{}/mcp/w/{}", dead_port, workspace_id),
            token: &token,
            folders: vec![&folder],
            last_active: None,
        }],
    );

    // The agent starts, shakes hands, and then does nothing at all.
    let mut agent = StdioAgent::spawn(
        &["--connections", connections.to_str().unwrap()],
        &[],
        &folder,
    );
    agent.initialize().await;

    let server = TestServer::start().await;
    let mut registration = McpWorkspaceRegistration::new("Idle".to_string());
    registration.workspace_id = Some(workspace_id.to_string());
    registration.token = Some(token.clone());
    let workspace = server.register_mcp_workspace_as(registration).await;
    write_connections(
        &connections,
        &[Listed::registered(&workspace, vec![&folder])],
    );

    let mut window = SimulatedFrontend::connect(server.base_url(), workspace_id).await;
    let listed = window
        .wait_for_clients(Duration::from_secs(45), |clients| clients.len() == 1)
        .await;
    assert_eq!(names(&listed), vec!["stdio-test-agent"]);
}

#[tokio::test]
async fn test_a_bridge_rejoins_when_its_workspace_registers_again() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Rejoined", None).await;

    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let connections = root.path().join("connections");
    write_connections(
        &connections,
        &[Listed::registered(&workspace, vec![&folder])],
    );

    let mut agent = StdioAgent::spawn(
        &["--connections", connections.to_str().unwrap()],
        &[],
        &folder,
    );
    agent.initialize().await;
    let mut window = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    window
        .wait_for_clients(Duration::from_secs(10), |clients| clients.len() == 1)
        .await;

    // Turning the feature off ends the presence stream; turning it back on
    // brings the bridge back without the agent doing anything.
    let client = server.create_client().await;
    match client
        .deregister_mcp_workspace(workspace.workspace_id.clone())
        .await
        .unwrap()
    {
        DeregisterMcpWorkspaceResponse::WorkspaceDeregistered => {}
        other => panic!("Unexpected deregistration response: {:?}", other),
    }
    let mut registration = McpWorkspaceRegistration::new("Rejoined".to_string());
    registration.workspace_id = Some(workspace.workspace_id.clone());
    registration.token = Some(workspace.token.clone());
    let again = server.register_mcp_workspace_as(registration).await;
    write_connections(&connections, &[Listed::registered(&again, vec![&folder])]);

    let mut window = SimulatedFrontend::connect(server.base_url(), &again.workspace_id).await;
    let listed = window
        .wait_for_clients(Duration::from_secs(45), |clients| clients.len() == 1)
        .await;
    assert_eq!(names(&listed), vec!["stdio-test-agent"]);
}
