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

use common::mcp::{post, McpAgent, SimulatedFrontend};
use common::TestServer;
use kallichore_api::models::McpFrontendRegistration;
use kallichore_api::{
    DeregisterMcpFrontendResponse, RegisterMcpFrontendResponse, ServerStatusResponse,
};
use kcshared::mcp_frontend::{
    AgentCommand, AgentCommandArg, CommandReply, CommandsChanged, ForegroundChanged, FrontendHello,
    FrontendMessage,
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
    let frontend = server.register_mcp_frontend("Test Window", None).await;

    assert!(frontend.port > 0, "Registration should report a port");
    assert_eq!(
        frontend.url,
        format!("http://127.0.0.1:{}/mcp", frontend.port)
    );
    assert_eq!(frontend.token.len(), 64, "Token should be 32 random bytes");

    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token);
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
    assert_eq!(result.field("positron_connected"), &json!(false));
    assert!(result.structured["server_version"].is_string());
}

#[tokio::test]
async fn test_requests_without_valid_credentials_are_refused() {
    let server = TestServer::start().await;
    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let port = frontend.port as u16;

    // No token at all.
    let mut headers = agent_headers(port, &frontend.token);
    headers.retain(|(name, _)| *name != "authorization");
    let response = post(port, "/mcp", &headers, &tools_list_body()).await;
    assert_eq!(response.status, 401, "{}", response.body);

    // A token that belongs to nobody.
    let headers = agent_headers(
        port,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    let response = post(port, "/mcp", &headers, &tools_list_body()).await;
    assert_eq!(response.status, 401, "{}", response.body);

    // A non-loopback Host, which is how DNS rebinding shows up.
    let mut headers = agent_headers(port, &frontend.token);
    headers.retain(|(name, _)| *name != "host");
    headers.push(("host", "evil.example.com".to_string()));
    let response = post(port, "/mcp", &headers, &tools_list_body()).await;
    assert_eq!(response.status, 403, "{}", response.body);

    // A non-loopback Origin, which is how a hostile page shows up.
    let mut headers = agent_headers(port, &frontend.token);
    headers.push(("origin", "https://evil.example.com".to_string()));
    let response = post(port, "/mcp", &headers, &tools_list_body()).await;
    assert_eq!(response.status, 403, "{}", response.body);

    // A loopback origin is fine, and a valid token reaches the protocol layer.
    let mut headers = agent_headers(port, &frontend.token);
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
    let response = post(port, "/mcp", &headers, &initialize.to_string()).await;
    assert_eq!(response.status, 200, "{}", response.body);

    // Anything outside /mcp is not served at all.
    let headers = agent_headers(port, &frontend.token);
    let response = post(port, "/sessions", &headers, "{}").await;
    assert_eq!(response.status, 404, "{}", response.body);
}

#[tokio::test]
async fn test_registration_is_idempotent_and_deregistration_closes_the_port() {
    let server = TestServer::start().await;
    let first = server.register_mcp_frontend("Test Window", None).await;

    // A window that reloads re-registers with its saved ID and must keep
    // working with the token its terminals already hold.
    let again = server
        .register_mcp_frontend("Test Window (reloaded)", Some(first.frontend_id.clone()))
        .await;
    assert_eq!(again.frontend_id, first.frontend_id);
    assert_eq!(again.token, first.token);
    assert_eq!(again.port, first.port);

    // A second window gets its own token on the same listener.
    let second = server.register_mcp_frontend("Second Window", None).await;
    assert_ne!(second.frontend_id, first.frontend_id);
    assert_ne!(second.token, first.token);
    assert_eq!(second.port, first.port);

    let client = server.create_client().await;
    match client
        .deregister_mcp_frontend(first.frontend_id.clone())
        .await
        .expect("Deregistration failed")
    {
        DeregisterMcpFrontendResponse::FrontendDeregistered => {}
        other => panic!("Unexpected deregistration response: {:?}", other),
    }

    // The listener stays up while another frontend is registered, but the
    // deregistered token no longer works.
    let headers = agent_headers(first.port as u16, &first.token);
    let response = post(first.port as u16, "/mcp", &headers, &tools_list_body()).await;
    assert_eq!(response.status, 401, "{}", response.body);

    match client
        .deregister_mcp_frontend(second.frontend_id.clone())
        .await
        .expect("Deregistration failed")
    {
        DeregisterMcpFrontendResponse::FrontendDeregistered => {}
        other => panic!("Unexpected deregistration response: {:?}", other),
    }

    // With the last frontend gone the port is released.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let connected = tokio::net::TcpStream::connect(("127.0.0.1", first.port as u16)).await;
    assert!(
        connected.is_err(),
        "The MCP port should be closed once the last frontend deregisters"
    );

    match client
        .deregister_mcp_frontend(first.frontend_id)
        .await
        .expect("Deregistration failed")
    {
        DeregisterMcpFrontendResponse::FrontendNotFound => {}
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
    assert!(mcp.frontends.is_empty());

    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token);
    agent.initialize().await;
    agent.call_tool("list_sessions", json!({})).await;

    let status = match client.server_status().await.expect("Status failed") {
        ServerStatusResponse::ServerStatusAndInformation(status) => status,
        other => panic!("Unexpected status response: {:?}", other),
    };
    let mcp = status.mcp.expect("Status should include an mcp block");
    assert!(mcp.active);
    assert_eq!(mcp.port, frontend.port);
    assert!(mcp.request_count >= 1, "Tool calls should be counted");
    assert_eq!(mcp.frontends.len(), 1);
    assert_eq!(mcp.frontends[0].id, frontend.frontend_id);
    assert_eq!(mcp.frontends[0].display_name, "Test Window");
    assert!(!mcp.frontends[0].connected, "No channel is open yet");

    let _channel = SimulatedFrontend::connect(server.base_url(), &frontend.frontend_id).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let status = match client.server_status().await.expect("Status failed") {
        ServerStatusResponse::ServerStatusAndInformation(status) => status,
        other => panic!("Unexpected status response: {:?}", other),
    };
    let mcp = status.mcp.expect("Status should include an mcp block");
    assert!(mcp.frontends[0].connected, "The channel should be reported");
}

#[tokio::test]
async fn test_command_catalog_survives_the_frontend_disconnecting() {
    let server = TestServer::start().await;
    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token);
    agent.initialize().await;

    // Before the frontend says hello there is nothing to search.
    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("total"), &json!(0));
    assert_eq!(result.field("positron_connected"), &json!(false));

    let channel = SimulatedFrontend::connect(server.base_url(), &frontend.frontend_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("2026.09.0".to_string()),
        commands: fake_catalog(),
        foreground_session_id: None,
        history_api_enabled: true,
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
async fn test_run_positron_command_is_brokered_to_the_frontend() {
    let server = TestServer::start().await;
    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token).named("claude-code");
    agent.initialize().await;

    let mut channel = SimulatedFrontend::connect(server.base_url(), &frontend.frontend_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("2026.09.0".to_string()),
        commands: fake_catalog(),
        foreground_session_id: None,
        history_api_enabled: false,
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

    // A command the frontend refuses.
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
    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token);
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
    let mut channel = SimulatedFrontend::connect(server.base_url(), &frontend.frontend_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        positron_version: None,
        commands: fake_catalog(),
        foreground_session_id: None,
        history_api_enabled: false,
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
    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token);
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
async fn test_frontend_channel_rejects_unknown_frontends() {
    let server = TestServer::start().await;
    server.register_mcp_frontend("Test Window", None).await;

    let url = format!(
        "{}/mcp/frontends/no-such-frontend/channel",
        server.base_url().replace("http://", "ws://")
    );
    let error = tokio_tungstenite::connect_async(&url)
        .await
        .expect_err("An unknown frontend should not get a channel");
    assert!(
        error.to_string().contains("404"),
        "Expected a 404, got: {}",
        error
    );
}

#[tokio::test]
async fn test_reconnecting_replaces_the_frontend_channel() {
    let server = TestServer::start().await;
    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token);
    agent.initialize().await;

    let first = SimulatedFrontend::connect(server.base_url(), &frontend.frontend_id).await;
    first.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("first".to_string()),
        commands: fake_catalog(),
        foreground_session_id: None,
        history_api_enabled: false,
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    // A second window for the same frontend ID takes over, and the first
    // channel closing must not mark the frontend disconnected.
    let second = SimulatedFrontend::connect(server.base_url(), &frontend.frontend_id).await;
    second.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("second".to_string()),
        commands: vec![],
        foreground_session_id: Some("session-from-second".to_string()),
        history_api_enabled: false,
    }));
    first.disconnect().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let result = agent.call_tool("list_positron_commands", json!({})).await;
    assert_eq!(result.field("positron_connected"), &json!(true));
    assert_eq!(result.structured["positron_version"], json!("second"));
    assert_eq!(result.field("total"), &json!(0));
}

#[tokio::test]
async fn test_foreground_updates_reach_the_session_tools() {
    let server = TestServer::start().await;
    let frontend = server.register_mcp_frontend("Test Window", None).await;
    let mut agent = McpAgent::new(frontend.port as u16, &frontend.token);
    agent.initialize().await;

    let channel = SimulatedFrontend::connect(server.base_url(), &frontend.frontend_id).await;
    channel.send(FrontendMessage::Hello(FrontendHello {
        positron_version: None,
        commands: vec![],
        foreground_session_id: Some("session-a".to_string()),
        history_api_enabled: false,
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
        "display_name": "Transport Window",
        "capabilities": { "commands": true },
    });
    let response = post_over(connect().await, "/mcp/frontends", &registration.to_string()).await;
    let frontend: Value = serde_json::from_str(&response).expect("Registration is not JSON");
    let frontend_id = frontend["frontend_id"].as_str().unwrap().to_string();
    let port = frontend["port"].as_u64().unwrap() as u16;
    let token = frontend["token"].as_str().unwrap().to_string();

    let request = format!("ws://localhost/mcp/frontends/{}/channel", frontend_id)
        .into_client_request()
        .expect("Failed to build the upgrade request");
    let (mut ws, _) = tokio_tungstenite::client_async(request, connect().await)
        .await
        .expect("Failed to upgrade the frontend channel");

    let hello = FrontendMessage::Hello(FrontendHello {
        positron_version: Some("transport".to_string()),
        commands: fake_catalog(),
        foreground_session_id: None,
        history_api_enabled: false,
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        serde_json::to_string(&hello).unwrap(),
    ))
    .await
    .expect("Failed to send hello");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut agent = McpAgent::new(port, &token);
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
