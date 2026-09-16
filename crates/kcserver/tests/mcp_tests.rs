//
// mcp_tests.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Integration tests for the MCP server hosted inside the supervisor.
//!
//! Everything here runs against a real `kcserver` process: registration goes
//! over the REST API, agents speak Streamable HTTP over the real listener, and
//! the simulated frontend uses a real WebSocket upgrade. Tests that need a live
//! kernel live in `mcp_execute_tests.rs`.

#![allow(unused_imports)]

#[path = "common/mod.rs"]
mod common;

use std::time::Duration;

use futures::SinkExt;

use common::mcp::{get, post, McpAgent, SimulatedFrontend};
use common::test_utils::{create_session_with_client, create_test_session};
use common::TestServer;
use kallichore_api::models::McpWorkspaceRegistration;
use kallichore_api::{
    DeregisterMcpWorkspaceResponse, RegisterMcpWorkspaceResponse, ServerStatusResponse,
};
use kcshared::mcp_frontend::{
    AgentCommand, AgentCommandArg, CommandReply, CommandsChanged, ForegroundChanged, FrontendHello,
    FrontendMessage, SessionsChanged,
};
use serde_json::{json, Value};

/// The tools the server is expected to publish.
const EXPECTED_TOOLS: [&str; 6] = [
    "list_sessions",
    "execute_code",
    "evaluate_code",
    "interrupt_session",
    "list_positron_commands",
    "run_positron_command",
];

/// Three commands standing in for a Positron catalog.
fn fake_catalog() -> Vec<AgentCommand> {
    vec![
        AgentCommand {
            id: "positronPackages.getPackages".to_string(),
            description: "List the packages installed in the session".to_string(),
            args: vec![AgentCommandArg {
                name: "sessionId".to_string(),
                description: Some("The session to inspect".to_string()),
                required: true,
                schema: Some(json!({ "type": "string" })),
            }],
            returns: Some("An array of package descriptions".to_string()),
        },
        AgentCommand {
            id: "workbench.action.language.runtime.startNewConsoleSession".to_string(),
            description: "Start a new console session".to_string(),
            args: vec![],
            returns: None,
        },
        AgentCommand {
            id: "vscode.open".to_string(),
            description: "Open a file in the editor".to_string(),
            args: vec![AgentCommandArg {
                name: "uri".to_string(),
                description: None,
                required: true,
                schema: Some(json!({ "type": "string" })),
            }],
            returns: None,
        },
    ]
}

/// The header set a well-behaved agent sends.
fn agent_headers(port: u16, token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("content-type", "application/json".to_string()),
        ("accept", "application/json, text/event-stream".to_string()),
        ("authorization", format!("Bearer {}", token)),
        ("host", format!("127.0.0.1:{}", port)),
    ]
}

/// A minimal JSON-RPC body that would reach a tool if it got that far.
fn tools_list_body() -> String {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }).to_string()
}

#[tokio::test]
async fn test_agent_discovers_and_calls_list_sessions() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;

    assert!(workspace.port > 0, "Registration should report a port");
    assert_eq!(
        workspace.url,
        format!(
            "http://127.0.0.1:{}/mcp/w/{}",
            workspace.port, workspace.workspace_id
        ),
        "Each workspace gets an endpoint of its own"
    );
    assert!(
        workspace.workspace_id.starts_with("test-workspace-"),
        "The ID should name the workspace so the URL reads well: {}",
        workspace.workspace_id
    );
    assert_eq!(workspace.token.len(), 64, "Token should be 32 random bytes");

    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    );
    let info = agent.initialize().await;
    assert_eq!(
        info["serverInfo"]["name"], "positron",
        "initialize: {}",
        info
    );
    assert!(
        info["instructions"]
            .as_str()
            .expect("Server should send instructions")
            .contains("evaluate_code"),
        "Instructions should explain the tools: {}",
        info
    );

    let tools = agent.list_tools().await;
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for expected in EXPECTED_TOOLS {
        assert!(
            names.contains(&expected),
            "Missing tool {}: {:?}",
            expected,
            names
        );
    }
    assert_eq!(
        names.len(),
        EXPECTED_TOOLS.len(),
        "Unexpected tools: {:?}",
        names
    );

    let result = agent.call_tool("list_sessions", json!({})).await;
    assert!(!result.is_error, "list_sessions failed: {:?}", result);
    assert_eq!(result.field("sessions"), &json!([]));
    assert_eq!(result.field("workspace")["name"], json!("Test Workspace"));
    assert_eq!(result.field("positron_connected"), &json!(false));
    assert!(result.structured["server_version"].is_string());
}

#[tokio::test]
async fn test_requests_without_valid_credentials_are_refused() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let port = workspace.port as u16;
    let path = format!("/mcp/w/{}", workspace.workspace_id);

    // No token at all.
    let mut headers = agent_headers(port, &workspace.token);
    headers.retain(|(name, _)| *name != "authorization");
    let response = post(port, &path, &headers, &tools_list_body()).await;
    assert_eq!(response.status, 401, "{}", response.body);

    // A token that belongs to nobody.
    let headers = agent_headers(
        port,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    let response = post(port, &path, &headers, &tools_list_body()).await;
    assert_eq!(response.status, 401, "{}", response.body);

    // A non-loopback Host, which is how DNS rebinding shows up.
    let mut headers = agent_headers(port, &workspace.token);
    headers.retain(|(name, _)| *name != "host");
    headers.push(("host", "evil.example.com".to_string()));
    let response = post(port, &path, &headers, &tools_list_body()).await;
    assert_eq!(response.status, 403, "{}", response.body);

    // A non-loopback Origin, which is how a hostile page shows up.
    let mut headers = agent_headers(port, &workspace.token);
    headers.push(("origin", "https://evil.example.com".to_string()));
    let response = post(port, &path, &headers, &tools_list_body()).await;
    assert_eq!(response.status, 403, "{}", response.body);

    // A loopback origin is fine, and a valid token reaches the protocol layer.
    let mut headers = agent_headers(port, &workspace.token);
    headers.push(("origin", format!("http://localhost:{}", port)));
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test-agent", "version": "1.0" },
        },
    });
    let response = post(port, &path, &headers, &initialize.to_string()).await;
    assert_eq!(response.status, 200, "{}", response.body);

    // Anything outside a workspace's endpoint is not served at all.
    let headers = agent_headers(port, &workspace.token);
    let response = post(port, "/sessions", &headers, "{}").await;
    assert_eq!(response.status, 404, "{}", response.body);
    let response = post(port, "/mcp", &headers, &tools_list_body()).await;
    assert_eq!(response.status, 404, "{}", response.body);

    // A valid token presented at another workspace's endpoint is refused, so a
    // stale configuration fails loudly instead of driving the wrong sessions.
    let other = server.register_mcp_workspace("Other Workspace", None).await;
    let headers = agent_headers(port, &other.token);
    let response = post(port, &path, &headers, &tools_list_body()).await;
    assert_eq!(response.status, 403, "{}", response.body);
}

/// The header set a client probing for a server card sends.
fn card_headers(port: u16) -> Vec<(&'static str, String)> {
    vec![
        ("accept", "application/mcp-server-card+json".to_string()),
        ("host", format!("127.0.0.1:{}", port)),
    ]
}

#[tokio::test]
async fn test_the_endpoint_publishes_a_server_card() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Card Workspace", None).await;
    let port = workspace.port as u16;
    let path = format!("/mcp/w/{}/server-card", workspace.workspace_id);

    // Read with no token, which is what makes the card worth serving.
    let response = get(port, &path, &card_headers(port)).await;
    assert_eq!(response.status, 200, "{}", response.body);
    assert_eq!(
        response.headers.get("content-type").unwrap(),
        "application/mcp-server-card+json"
    );
    let card = response.json();
    assert_eq!(
        card["remotes"][0]["url"], workspace.url,
        "The card should advertise the endpoint registration handed out"
    );
    assert!(
        !response.body.contains(&workspace.token),
        "A card is public metadata and must not carry the token: {}",
        response.body
    );

    // What the card claims is what the live server does.
    let mut agent = McpAgent::new(port, &workspace.workspace_id, &workspace.token);
    let initialized = agent.initialize().await;
    assert_eq!(initialized["serverInfo"]["title"], card["title"]);
    assert!(
        card["remotes"][0]["supportedProtocolVersions"]
            .as_array()
            .expect("The card should list protocol versions")
            .contains(&initialized["protocolVersion"]),
        "The negotiated version should be one the card offers: {}",
        card["remotes"][0]["supportedProtocolVersions"]
    );

    // An unchanged card is validated rather than sent again.
    let etag = response
        .headers
        .get("etag")
        .expect("The card should carry an entity tag")
        .to_str()
        .unwrap()
        .to_string();
    let mut headers = card_headers(port);
    headers.push(("if-none-match", etag));
    let revalidated = get(port, &path, &headers).await;
    assert_eq!(revalidated.status, 304, "{}", revalidated.body);
    assert!(revalidated.body.is_empty(), "{}", revalidated.body);

    // A workspace that is not registered has no card.
    let response = get(
        port,
        "/mcp/w/no-such-workspace/server-card",
        &card_headers(port),
    )
    .await;
    assert_eq!(response.status, 404, "{}", response.body);

    // The card is read with GET; the endpoint itself takes the POSTs.
    let response = post(port, &path, &card_headers(port), "").await;
    assert_eq!(response.status, 405, "{}", response.body);

    // A page in the user's browser has no business reading it, so the loopback
    // guards still apply and no CORS header invites one.
    let mut headers = card_headers(port);
    headers.push(("origin", "https://evil.example.com".to_string()));
    let response = get(port, &path, &headers).await;
    assert_eq!(response.status, 403, "{}", response.body);
    assert!(
        response
            .headers
            .get("access-control-allow-origin")
            .is_none(),
        "The card should not be offered to cross-origin readers"
    );
}

#[tokio::test]
async fn test_registration_is_idempotent_and_deregistration_closes_the_port() {
    let server = TestServer::start().await;
    let first = server.register_mcp_workspace("Test Workspace", None).await;

    // A window that reloads re-registers with its saved ID and must keep
    // working with the token its terminals already hold.
    let again = server
        .register_mcp_workspace(
            "Test Workspace (reloaded)",
            Some(first.workspace_id.clone()),
        )
        .await;
    assert_eq!(again.workspace_id, first.workspace_id);
    assert_eq!(again.token, first.token);
    assert_eq!(again.port, first.port);

    // A second workspace gets its own token on the same listener.
    let second = server
        .register_mcp_workspace("Second Workspace", None)
        .await;
    assert_ne!(second.workspace_id, first.workspace_id);
    assert_ne!(second.token, first.token);
    assert_eq!(second.port, first.port);

    let client = server.create_client().await;
    match client
        .deregister_mcp_workspace(first.workspace_id.clone())
        .await
        .expect("Deregistration failed")
    {
        DeregisterMcpWorkspaceResponse::WorkspaceDeregistered => {}
        other => panic!("Unexpected deregistration response: {:?}", other),
    }

    // The listener stays up while another workspace is registered, but the
    // deregistered token no longer works.
    let headers = agent_headers(first.port as u16, &first.token);
    let path = format!("/mcp/w/{}", first.workspace_id);
    let response = post(first.port as u16, &path, &headers, &tools_list_body()).await;
    assert_eq!(response.status, 401, "{}", response.body);

    match client
        .deregister_mcp_workspace(second.workspace_id.clone())
        .await
        .expect("Deregistration failed")
    {
        DeregisterMcpWorkspaceResponse::WorkspaceDeregistered => {}
        other => panic!("Unexpected deregistration response: {:?}", other),
    }

    // With the last workspace gone the port is released.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let connected = tokio::net::TcpStream::connect(("127.0.0.1", first.port as u16)).await;
    assert!(
        connected.is_err(),
        "The MCP port should be closed once the last workspace deregisters"
    );

    match client
        .deregister_mcp_workspace(first.workspace_id)
        .await
        .expect("Deregistration failed")
    {
        DeregisterMcpWorkspaceResponse::WorkspaceNotFound => {}
        other => panic!("Unexpected repeat deregistration response: {:?}", other),
    }
}

#[tokio::test]
async fn test_server_status_reports_the_mcp_server() {
    let server = TestServer::start().await;
    let client = server.create_client().await;

    let status = match client.server_status().await.expect("Status failed") {
        ServerStatusResponse::ServerStatusAndInformation(status) => status,
        other => panic!("Unexpected status response: {:?}", other),
    };
    let mcp = status.mcp.expect("Status should include an mcp block");
    assert!(!mcp.active, "The listener should not start on its own");
    assert_eq!(mcp.port, 0);
    assert!(mcp.workspaces.is_empty());

    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    );
    agent.initialize().await;
    agent.call_tool("list_sessions", json!({})).await;

    let status = match client.server_status().await.expect("Status failed") {
        ServerStatusResponse::ServerStatusAndInformation(status) => status,
        other => panic!("Unexpected status response: {:?}", other),
    };
    let mcp = status.mcp.expect("Status should include an mcp block");
    assert!(mcp.active);
    assert_eq!(mcp.port, workspace.port);
    assert!(mcp.request_count >= 1, "Tool calls should be counted");
    assert_eq!(mcp.workspaces.len(), 1);
    assert_eq!(mcp.workspaces[0].id, workspace.workspace_id);
    assert_eq!(mcp.workspaces[0].display_name, "Test Workspace");
    assert!(!mcp.workspaces[0].connected, "No channel is open yet");

    let _channel = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let status = match client.server_status().await.expect("Status failed") {
        ServerStatusResponse::ServerStatusAndInformation(status) => status,
        other => panic!("Unexpected status response: {:?}", other),
    };
    let mcp = status.mcp.expect("Status should include an mcp block");
    assert!(
        mcp.workspaces[0].connected,
        "The channel should be reported"
    );
}

#[tokio::test]
async fn test_command_catalog_survives_the_window_disconnecting() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    );
    agent.initialize().await;

    // Before a window says hello there is nothing to search.
    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("total"), &json!(0));
    assert_eq!(result.field("positron_connected"), &json!(false));

    let channel = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("2026.09.0".to_string()),
        commands: fake_catalog(),
        history_api_enabled: true,
        ..Default::default()
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("total"), &json!(3));
    assert_eq!(result.field("positron_connected"), &json!(true));
    assert_eq!(
        result.structured["positron_version"],
        json!("2026.09.0"),
        "{}",
        result.structured
    );

    // Matching runs over IDs, descriptions, and argument names.
    let result = agent
        .call_tool("list_positron_commands", json!({ "query": "packages" }))
        .await;
    let commands = result.field("commands").as_array().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["id"], "positronPackages.getPackages");
    assert_eq!(commands[0]["args"][0]["name"], "sessionId");
    assert_eq!(commands[0]["args"][0]["required"], json!(true));
    assert_eq!(
        commands[0]["args"][0]["schema"],
        json!({ "type": "string" })
    );
    assert_eq!(
        commands[0]["returns"],
        json!("An array of package descriptions")
    );

    let result = agent
        .call_tool("list_positron_commands", json!({ "query": "uri" }))
        .await;
    assert_eq!(result.field("matched"), &json!(1));

    let result = agent
        .call_tool("list_positron_commands", json!({ "limit": 2 }))
        .await;
    assert_eq!(result.field("commands").as_array().unwrap().len(), 2);
    assert_eq!(result.field("truncated"), &json!(true));

    // A refreshed catalog replaces the cached one.
    channel.send(FrontendMessage::CommandsChanged(CommandsChanged {
        commands: vec![fake_catalog().remove(2)],
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("total"), &json!(1));

    // Searching still works when the window is gone; only running does not.
    channel.disconnect().await;
    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("total"), &json!(1));
    assert_eq!(result.field("positron_connected"), &json!(false));
    assert!(result.structured["positron_disconnected_since"].is_string());
}

#[tokio::test]
async fn test_run_positron_command_is_brokered_to_a_window() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    )
    .named("claude-code");
    agent.initialize().await;

    let mut channel = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("2026.09.0".to_string()),
        commands: fake_catalog(),
        ..Default::default()
    }));

    // A command that succeeds.
    let call = tokio::spawn(async move {
        let result = agent
            .call_tool(
                "run_positron_command",
                json!({ "command_id": "vscode.open", "args": ["file:///tmp/x.R"] }),
            )
            .await;
        (agent, result)
    });

    let request = channel.next_command(Duration::from_secs(10)).await;
    assert_eq!(request.command_id, "vscode.open");
    assert_eq!(request.args, vec![json!("file:///tmp/x.R")]);
    assert_eq!(
        request.agent.name.as_deref(),
        Some("claude-code"),
        "The agent should identify itself from clientInfo"
    );
    assert!(request.deadline_ms > 0);
    channel.reply(CommandReply {
        id: request.id,
        ok: true,
        result: Some(json!({ "opened": true })),
        reason: None,
        message: None,
    });

    let (mut agent, result) = call.await.expect("Tool call panicked");
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("result"), &json!({ "opened": true }));

    // A command the window refuses.
    let call = tokio::spawn(async move {
        let result = agent
            .call_tool(
                "run_positron_command",
                json!({ "command_id": "positronPackages.getPackages" }),
            )
            .await;
        (agent, result)
    });

    let request = channel.next_command(Duration::from_secs(10)).await;
    assert!(request.args.is_empty());
    channel.reply(CommandReply {
        id: request.id,
        ok: false,
        result: None,
        reason: Some("disabled".to_string()),
        message: Some("No session is active".to_string()),
    });

    let (_agent, result) = call.await.expect("Tool call panicked");
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("reason"), &json!("disabled"));
    assert_eq!(result.field("message"), &json!("No session is active"));
}

#[tokio::test]
async fn test_run_positron_command_waits_for_a_reloading_window() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    );
    agent.initialize().await;

    // The window is not there yet; the call should hold rather than fail fast.
    let call = tokio::spawn(async move {
        let result = agent
            .call_tool(
                "run_positron_command",
                json!({ "command_id": "vscode.open" }),
            )
            .await;
        (agent, result)
    });

    tokio::time::sleep(Duration::from_secs(2)).await;
    let mut channel = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        commands: fake_catalog(),
        ..Default::default()
    }));

    let request = channel.next_command(Duration::from_secs(20)).await;
    channel.reply(CommandReply {
        id: request.id,
        ok: true,
        result: Some(json!("done")),
        reason: None,
        message: None,
    });

    let (_agent, result) = call.await.expect("Tool call panicked");
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("result"), &json!("done"));
}

#[tokio::test]
async fn test_run_positron_command_reports_a_disconnected_window() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    );
    agent.initialize().await;

    let started = std::time::Instant::now();
    let result = agent
        .call_tool(
            "run_positron_command",
            json!({ "command_id": "vscode.open" }),
        )
        .await;
    let elapsed = started.elapsed();

    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("reason"), &json!("POSITRON_DISCONNECTED"));
    assert!(
        result
            .field("message")
            .as_str()
            .unwrap()
            .contains("kernel tools"),
        "The error should point the agent at what still works: {:?}",
        result
    );
    assert!(
        elapsed >= Duration::from_secs(14) && elapsed < Duration::from_secs(40),
        "The call should wait about 15 seconds, not {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_frontend_channel_rejects_unknown_workspaces() {
    let server = TestServer::start().await;
    server.register_mcp_workspace("Test Workspace", None).await;

    let url = format!(
        "{}/mcp/workspaces/no-such-workspace/channel",
        server.base_url().replace("http://", "ws://")
    );
    let error = tokio_tungstenite::connect_async(&url)
        .await
        .expect_err("An unknown workspace should not get a channel");
    assert!(
        error.to_string().contains("404"),
        "Expected a 404, got: {}",
        error
    );
}

#[tokio::test]
async fn test_the_window_the_user_focused_serves_commands() {
    let server = TestServer::start().await;
    let workspace = server
        .register_mcp_workspace("Shared Workspace", None)
        .await;
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    );
    agent.initialize().await;

    // Two windows onto one workspace share a record, because Positron keeps the
    // workspace ID and its session list in workspace-scoped state.
    // Both attach; neither evicts the other.
    let mut first = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    first.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("first".to_string()),
        commands: fake_catalog(),
        ..Default::default()
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut second = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    second.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("second".to_string()),
        ..Default::default()
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    // What the record knows is whatever the window that spoke last said.
    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("positron_connected"), &json!(true));
    assert_eq!(result.structured["positron_version"], json!("second"));
    assert_eq!(result.field("total"), &json!(0));

    // The user clicks back to the first window, so that is where a command the
    // agent asks for should happen.
    first.send(FrontendMessage::Focused);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let call = tokio::spawn(async move {
        let result = agent
            .call_tool(
                "run_positron_command",
                json!({ "command_id": "vscode.open" }),
            )
            .await;
        (agent, result)
    });
    let request = first.next_command(Duration::from_secs(10)).await;
    first.reply(CommandReply {
        id: request.id,
        ok: true,
        result: Some(json!("in the focused window")),
        reason: None,
        message: None,
    });
    let (mut agent, result) = call.await.expect("Tool call panicked");
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("result"), &json!("in the focused window"));

    // The focused window closing leaves the workspace connected through its
    // sibling, which picks up the next command.
    first.disconnect().await;
    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("positron_connected"), &json!(true));

    let call = tokio::spawn(async move {
        let result = agent
            .call_tool(
                "run_positron_command",
                json!({ "command_id": "vscode.open" }),
            )
            .await;
        (agent, result)
    });
    let request = second.next_command(Duration::from_secs(10)).await;
    second.reply(CommandReply {
        id: request.id,
        ok: true,
        result: Some(json!("in the surviving window")),
        reason: None,
        message: None,
    });
    let (_agent, result) = call.await.expect("Tool call panicked");
    assert_eq!(result.field("result"), &json!("in the surviving window"));
}

/// Create a session belonging to a workspace, or to nobody when `owner` is None.
///
/// The sessions are never started: which sessions an agent may target is
/// decided before any kernel is touched.
async fn create_session(server: &TestServer, session_id: &str, owner: Option<&str>) -> String {
    let client = server.create_client().await;
    let mut session = create_test_session(session_id.to_string(), "python3");
    session.workspace_id = owner.map(|id| id.to_string());
    create_session_with_client(&client, session).await
}

#[tokio::test]
async fn test_sessions_belong_to_the_workspace_that_created_them() {
    let server = TestServer::start().await;

    // Two workspaces sharing one supervisor, as every window does on Positron
    // Server. Neither has opened a channel: a session's owner is settled when it
    // is created, so the kernel tools do not wait on Positron for it.
    let first = server.register_mcp_workspace("My Workspace", None).await;
    let second = server.register_mcp_workspace("Their Workspace", None).await;
    let mine = create_session(
        &server,
        "session-in-my-workspace",
        Some(&first.workspace_id),
    )
    .await;
    let theirs = create_session(
        &server,
        "session-in-their-workspace",
        Some(&second.workspace_id),
    )
    .await;

    let mut agent = McpAgent::new(first.port as u16, &first.workspace_id, &first.token);
    agent.initialize().await;

    let result = agent.call_tool("list_sessions", json!({})).await;
    let sessions = result.field("sessions").as_array().unwrap().clone();
    assert_eq!(sessions.len(), 1, "{:?}", result);
    assert_eq!(sessions[0]["session_id"], json!(mine));
    assert_eq!(result.field("workspace")["name"], json!("My Workspace"));
    assert_eq!(
        result.field("sessions_in_other_workspaces"),
        &json!(1),
        "The agent should know other workspaces exist without being told about them"
    );

    // Naming another workspace's session is refused in terms the agent can act
    // on, rather than running there or claiming the session does not exist.
    let result = agent
        .call_tool("interrupt_session", json!({ "session_id": theirs }))
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("SESSION_NOT_VISIBLE"));

    let result = agent
        .call_tool(
            "interrupt_session",
            json!({ "session_id": "no-such-session" }),
        )
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("SESSION_NOT_FOUND"));
}

#[tokio::test]
async fn test_a_client_inside_a_kernel_cannot_run_code_in_that_kernel() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("My Workspace", None).await;
    let inside = create_session(
        &server,
        "session-running-the-client",
        Some(&workspace.workspace_id),
    )
    .await;
    let other = create_session(&server, "another-session", Some(&workspace.workspace_id)).await;

    // The endpoint the supervisor put in that kernel's environment, which names
    // the kernel's own session.
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    )
    .in_session(&inside);
    agent.initialize().await;

    // Its own session is listed, marked, and refused: code sent there would
    // queue behind the cell still waiting for this call to return.
    let result = agent.call_tool("list_sessions", json!({})).await;
    let listed: Vec<Value> = result
        .field("sessions")
        .as_array()
        .unwrap()
        .iter()
        .map(|session| json!([session["session_id"], session["is_your_own_session"]]))
        .collect();
    assert_eq!(
        listed,
        vec![json!([inside, true]), json!([other, Value::Null])],
        "{:?}",
        result
    );

    let result = agent
        .call_tool("execute_code", json!({ "code": "1", "session_id": inside }))
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("SESSION_IS_CALLER"));

    // Only running code is refused. Everything else about its own session is
    // ordinary business, because nothing else waits on that session's turn.
    let result = agent
        .call_tool("interrupt_session", json!({ "session_id": inside }))
        .await;
    assert_ne!(
        result.structured.get("code"),
        Some(&json!("SESSION_IS_CALLER")),
        "{:?}",
        result
    );

    // The other session is reachable from inside a kernel like any other. It
    // has no kernel to answer, so this ends in the timeout; a short one keeps
    // the test quick, and what matters is which error comes back.
    let result = agent
        .call_tool(
            "execute_code",
            json!({ "code": "1", "session_id": other, "timeout_s": 1 }),
        )
        .await;
    assert_ne!(
        result.structured.get("code"),
        Some(&json!("SESSION_IS_CALLER")),
        "{:?}",
        result
    );
}

#[tokio::test]
async fn test_a_workspace_reaches_sessions_it_did_not_create_but_not_another_ones() {
    let server = TestServer::start().await;
    let first = server.register_mcp_workspace("My Workspace", None).await;
    let second = server.register_mcp_workspace("Their Workspace", None).await;

    // A session that was already running when the workspace registered names no
    // owner, so a window has to say it holds it.
    let orphan = create_session(&server, "session-from-before", None).await;
    let theirs = create_session(
        &server,
        "session-in-their-workspace",
        Some(&second.workspace_id),
    )
    .await;

    let window = SimulatedFrontend::connect(server.base_url(), &first.workspace_id).await;
    window.send(FrontendMessage::Hello(FrontendHello {
        // Claiming the other workspace's session too, which must not work.
        session_ids: vec![orphan.clone(), theirs.clone()],
        ..Default::default()
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut agent = McpAgent::new(first.port as u16, &first.workspace_id, &first.token);
    agent.initialize().await;

    let result = agent.call_tool("list_sessions", json!({})).await;
    let sessions = result.field("sessions").as_array().unwrap().clone();
    assert_eq!(sessions.len(), 1, "{:?}", result);
    assert_eq!(sessions[0]["session_id"], json!(orphan));

    // A window that gives a session up stops reaching it.
    window.send(FrontendMessage::SessionsChanged(SessionsChanged {
        session_ids: vec![],
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let result = agent.call_tool("list_sessions", json!({})).await;
    assert_eq!(result.field("sessions"), &json!([]));
    assert_eq!(result.field("sessions_in_other_workspaces"), &json!(2));
}

#[tokio::test]
async fn test_foreground_updates_reach_the_session_tools() {
    let server = TestServer::start().await;
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    );
    agent.initialize().await;

    let channel = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        foreground_session_id: Some("session-a".to_string()),
        ..Default::default()
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The foreground session is announced but has no kernel behind it, so
    // targeting falls through to "there are no sessions at all".
    let result = agent
        .call_tool("execute_code", json!({ "code": "1" }))
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("NO_SESSION_SELECTED"));

    channel.send(FrontendMessage::ForegroundChanged(ForegroundChanged {
        session_id: None,
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let result = agent.call_tool("list_sessions", json!({})).await;
    assert!(!result.is_error, "{:?}", result);
}

/// Exercise the frontend channel over whatever transport the supervisor's main
/// API is served on. The MCP listener itself is always TCP, because agents need
/// a URL, but the channel has to ride the same transport Positron already uses.
async fn exercise_frontend_channel_over<S, F, Fut>(connect: F)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = S>,
{
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let registration = json!({
        "display_name": "Transport Workspace",
        "capabilities": { "commands": true },
    });
    let response = post_over(
        connect().await,
        "/mcp/workspaces",
        &registration.to_string(),
    )
    .await;
    let workspace: Value = serde_json::from_str(&response).expect("Registration is not JSON");
    let workspace_id = workspace["workspace_id"].as_str().unwrap().to_string();
    let port = workspace["port"].as_u64().unwrap() as u16;
    let token = workspace["token"].as_str().unwrap().to_string();

    let request = format!("ws://localhost/mcp/workspaces/{}/channel", workspace_id)
        .into_client_request()
        .expect("Failed to build the upgrade request");
    let (mut ws, _) = tokio_tungstenite::client_async(request, connect().await)
        .await
        .expect("Failed to upgrade the frontend channel");

    let hello = FrontendMessage::Hello(FrontendHello {
        positron_version: Some("transport".to_string()),
        commands: fake_catalog(),
        ..Default::default()
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        serde_json::to_string(&hello).unwrap(),
    ))
    .await
    .expect("Failed to send hello");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut agent = McpAgent::new(port, &workspace_id, &token);
    agent.initialize().await;
    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("total"), &json!(3));
    assert_eq!(result.field("positron_connected"), &json!(true));
    assert_eq!(result.structured["positron_version"], json!("transport"));
}

/// POST JSON to the supervisor over an already-connected stream.
async fn post_over<S>(stream: S, path: &str, body: &str) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper_util::rt::TokioIo;

    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("Failed to start an HTTP connection");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = hyper::Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .expect("Failed to build request");

    let response = sender
        .send_request(request)
        .await
        .expect("Failed to send request");
    assert_eq!(
        response.status(),
        200,
        "Registration over the transport failed"
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("Failed to read response")
        .to_bytes();
    String::from_utf8_lossy(&body).to_string()
}

#[cfg(unix)]
#[tokio::test]
async fn test_frontend_channel_works_over_a_unix_socket() {
    use common::TestServerMode;

    let server = TestServer::start_with_mode(TestServerMode::DomainSocket).await;
    let socket_path = server
        .socket_path()
        .expect("Domain socket server should report its socket")
        .to_string();

    exercise_frontend_channel_over(|| {
        let socket_path = socket_path.clone();
        async move {
            tokio::net::UnixStream::connect(&socket_path)
                .await
                .expect("Failed to connect to the supervisor socket")
        }
    })
    .await;
}

#[cfg(windows)]
#[tokio::test]
async fn test_frontend_channel_works_over_a_named_pipe() {
    use common::TestServerMode;

    let server = TestServer::start_with_mode(TestServerMode::NamedPipe).await;
    let pipe_name = server
        .pipe_name()
        .expect("Named pipe server should report its pipe")
        .to_string();

    exercise_frontend_channel_over(|| {
        let pipe_name = pipe_name.clone();
        async move {
            tokio::net::windows::named_pipe::ClientOptions::new()
                .open(&pipe_name)
                .expect("Failed to connect to the supervisor named pipe")
        }
    })
    .await;
}
