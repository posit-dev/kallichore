//
// mcp_execute_tests.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Integration tests for the MCP kernel tools, against a real ipykernel.
//!
//! Skipped when Python or ipykernel is unavailable, like `execute_code_test.rs`.

#![allow(unused_imports)]

#[path = "common/mod.rs"]
mod common;

use std::time::Duration;

use common::mcp::{McpAgent, SimulatedFrontend};
use common::test_utils::{
    create_session_with_client, create_test_session, get_python_executable, is_ipykernel_available,
    ClientContext,
};
use common::TestServer;
use futures::{SinkExt, StreamExt};
use kallichore_api::models::Status;
use kallichore_api::{ApiNoContext, GetSessionResponse, StartSessionResponse};
use kcshared::kernel_message::KernelMessage;
use kcshared::mcp_frontend::{ForegroundChanged, FrontendHello, FrontendMessage, SessionsChanged};
use kcshared::websocket_message::WebsocketMessage;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// A 1x1 transparent PNG, so image handling can be tested without matplotlib.
const TINY_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";

/// Register a frontend holding the given sessions, connect its channel, and
/// return an initialized agent for it.
///
/// Claiming the sessions is what makes them reachable: an agent only ever sees
/// the sessions the window it was launched from reports holding.
async fn agent_for(
    server: &TestServer,
    session_ids: &[&str],
) -> (String, SimulatedFrontend, McpAgent) {
    let workspace = server.register_mcp_workspace("Test Workspace", None).await;
    let window = SimulatedFrontend::connect(server.base_url(), &workspace.workspace_id).await;
    window.send(FrontendMessage::Hello(FrontendHello {
        positron_version: Some("2026.09.0".to_string()),
        session_ids: session_ids.iter().map(|id| id.to_string()).collect(),
        focused: true,
        ..Default::default()
    }));
    // Let the claim land before the agent asks what it can reach.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut agent = McpAgent::new(
        workspace.port as u16,
        &workspace.workspace_id,
        &workspace.token,
    )
    .named("claude-code");
    agent.initialize().await;
    (workspace.workspace_id, window, agent)
}

/// Create and start a Python session, waiting until the kernel is runnable.
///
/// Retried because the supervisor picks kernel ports by binding and releasing a
/// probe socket, so another process can take the port before the kernel binds
/// it. That race is unrelated to anything under test here.
async fn start_session(
    client: &Box<dyn ApiNoContext<ClientContext> + Send + Sync>,
    python_cmd: &str,
) -> String {
    for attempt in 1..=3 {
        let session_id = format!("mcp-exec-{}", Uuid::new_v4());
        create_session_with_client(client, create_test_session(session_id.clone(), python_cmd))
            .await;

        match client
            .start_session(session_id.clone())
            .await
            .expect("Failed to start session")
        {
            StartSessionResponse::Started(_) => {}
            other => {
                println!("Kernel start attempt {} failed: {:?}", attempt, other);
                continue;
            }
        }

        for _ in 0..200 {
            if let GetSessionResponse::SessionDetails(details) = client
                .get_session(session_id.clone())
                .await
                .expect("Failed to get session")
            {
                if details.status == Status::Idle || details.status == Status::Busy {
                    return session_id;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        println!("Kernel for session {} never became ready", session_id);
    }
    panic!("Could not start a Python kernel in three attempts");
}

/// Wait for a session to reach the given status.
async fn wait_for_status(
    client: &Box<dyn ApiNoContext<ClientContext> + Send + Sync>,
    session_id: &str,
    wanted: Status,
    timeout: Duration,
) {
    let deadline = std::time::Instant::now() + timeout;
    let mut last = None;
    while std::time::Instant::now() < deadline {
        if let GetSessionResponse::SessionDetails(details) = client
            .get_session(session_id.to_string())
            .await
            .expect("Failed to get session")
        {
            last = Some(details.status);
            if details.status == wanted {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "Session {} never reached {:?}; last status {:?}",
        session_id, wanted, last
    );
}

/// Skip the test body when there is no ipykernel to run it against.
macro_rules! require_ipykernel {
    () => {
        match get_python_executable().await {
            Some(cmd) if is_ipykernel_available().await => cmd,
            _ => {
                println!("Skipping: no Python with ipykernel found");
                return;
            }
        }
    };
}

/// Open the session's WebSocket and return a stream of decoded messages.
async fn connect_session_ws(
    server: &TestServer,
    session_id: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!(
        "{}/sessions/{}/channels",
        server.base_url().replace("http://", "ws://"),
        session_id
    );
    let (stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("Failed to open the session WebSocket");
    stream
}

/// Read decoded WebSocket messages until the predicate is satisfied or time
/// runs out, returning everything read.
async fn collect_until(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    timeout: Duration,
    mut done: impl FnMut(&WebsocketMessage) -> bool,
) -> Vec<WebsocketMessage> {
    let mut collected = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let next = tokio::time::timeout_at(deadline, stream.next()).await;
        match next {
            Ok(Some(Ok(Message::Text(text)))) => {
                let Ok(message) = serde_json::from_str::<WebsocketMessage>(&text) else {
                    continue;
                };
                let finished = done(&message);
                collected.push(message);
                if finished {
                    return collected;
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(e))) => panic!("Session WebSocket error: {}", e),
            Ok(None) => return collected,
            Err(_) => return collected,
        }
    }
}

#[tokio::test]
async fn test_execute_code_returns_output_results_images_and_errors() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let session_id = start_session(&client, &python_cmd).await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[&session_id]).await;

    let result = agent
        .call_tool(
            "execute_code",
            json!({
                "code": "print('hello from the agent')\nimport sys; print('to stderr', file=sys.stderr)\n21 * 2",
                "session_id": session_id,
            }),
        )
        .await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("status"), &json!("ok"));
    assert!(
        result
            .field("stdout")
            .as_str()
            .unwrap()
            .contains("hello from the agent"),
        "{:?}",
        result
    );
    assert!(
        result
            .field("stderr")
            .as_str()
            .unwrap()
            .contains("to stderr"),
        "{:?}",
        result
    );
    assert_eq!(result.structured["result"]["text/plain"], json!("42"));
    assert_eq!(result.field("truncated"), &json!(false));
    assert!(result.structured["elapsed_ms"].is_number());
    let first_count = result.field("execution_count").as_i64().unwrap();
    assert!(first_count >= 1, "{:?}", result);

    // A second visible execution advances the counter.
    let result = agent
        .call_tool(
            "execute_code",
            json!({ "code": "'second'", "session_id": session_id }),
        )
        .await;
    assert_eq!(
        result.field("execution_count").as_i64().unwrap(),
        first_count + 1
    );

    // Images come back as content blocks the agent can actually look at.
    let result = agent
        .call_tool(
            "execute_code",
            json!({
                "code": format!(
                    "from IPython.display import publish_display_data\npublish_display_data({{'image/png': '{}'}})",
                    TINY_PNG
                ),
                "session_id": session_id,
            }),
        )
        .await;
    assert!(!result.is_error, "{:?}", result);
    let images = result.blocks("image");
    assert_eq!(images.len(), 1, "{:?}", result);
    assert_eq!(images[0]["mimeType"], json!("image/png"));
    assert_eq!(images[0]["data"], json!(TINY_PNG));
    assert_eq!(result.field("images"), &json!(1));

    // A raising cell is a tool error carrying a structured traceback.
    let result = agent
        .call_tool(
            "execute_code",
            json!({ "code": "raise ValueError('boom')", "session_id": session_id }),
        )
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("status"), &json!("error"));
    assert_eq!(result.structured["error"]["name"], json!("ValueError"));
    assert_eq!(result.structured["error"]["message"], json!("boom"));
    assert!(
        result.structured["error"]["traceback"]
            .as_array()
            .expect("A traceback should be present")
            .len()
            > 0
    );
}

#[tokio::test]
async fn test_evaluate_code_returns_a_value_without_touching_history() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let session_id = start_session(&client, &python_cmd).await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[&session_id]).await;

    // Establish the execution counter with a stored execution.
    let stored = agent
        .call_tool(
            "execute_code",
            json!({ "code": "1", "session_id": session_id }),
        )
        .await;
    let count = stored.field("execution_count").as_i64().unwrap();

    let mut ws = connect_session_ws(&server, &session_id).await;

    let result = agent
        .call_tool(
            "evaluate_code",
            json!({ "code": "1 + 1", "session_id": session_id }),
        )
        .await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.structured["result"]["text/plain"], json!("2"));
    assert_eq!(
        result.field("execution_count").as_i64().unwrap(),
        count,
        "An evaluation must not advance the execution counter"
    );

    // A later stored execution continues from the same counter, so the
    // evaluation really did stay out of the kernel's history.
    let after = agent
        .call_tool(
            "execute_code",
            json!({ "code": "2", "session_id": session_id }),
        )
        .await;
    assert_eq!(after.field("execution_count").as_i64().unwrap(), count + 1);

    // An evaluation is still the agent working in the user's session, so the
    // user sees it: the echo reaches the mirror, preceded by the event that
    // says who asked for it.
    let messages = collect_until(&mut ws, Duration::from_secs(10), |message| {
        matches!(message, WebsocketMessage::Jupyter(msg)
            if msg.header.msg_type == "execute_input"
                && msg.content.get("code").and_then(|c| c.as_str()) == Some("1 + 1"))
    })
    .await;
    let announced_at = messages
        .iter()
        .position(|message| {
            matches!(message, WebsocketMessage::Kernel(KernelMessage::ExecutionRequested(e))
                if e.code == "1 + 1" && e.attribution.tool == "evaluate_code")
        })
        .unwrap_or_else(|| panic!("The evaluation should be announced: {:?}", messages));
    let echoed_at = messages.len() - 1;
    assert!(
        announced_at < echoed_at,
        "The attribution event must arrive before the echo the user sees"
    );
}

#[tokio::test]
async fn test_execution_is_announced_before_its_output() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let session_id = start_session(&client, &python_cmd).await;
    let (workspace_id, _window, mut agent) = agent_for(&server, &[&session_id]).await;

    let mut ws = connect_session_ws(&server, &session_id).await;

    let result = agent
        .call_tool(
            "execute_code",
            json!({ "code": "print('mirrored')", "session_id": session_id }),
        )
        .await;
    assert!(!result.is_error, "{:?}", result);

    let messages = collect_until(&mut ws, Duration::from_secs(10), |message| {
        matches!(message, WebsocketMessage::Jupyter(msg) if msg.header.msg_type == "execute_input")
    })
    .await;

    let announced_at = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                WebsocketMessage::Kernel(KernelMessage::ExecutionRequested(_))
            )
        })
        .expect("The execution should be announced to the connected client");
    let echoed_at = messages
        .iter()
        .position(|message| {
            matches!(message, WebsocketMessage::Jupyter(msg) if msg.header.msg_type == "execute_input")
        })
        .expect("The kernel should echo execute_input");
    assert!(
        announced_at < echoed_at,
        "The attribution event must arrive before the echo it explains"
    );

    let WebsocketMessage::Kernel(KernelMessage::ExecutionRequested(event)) =
        &messages[announced_at]
    else {
        unreachable!()
    };
    assert_eq!(event.code, "print('mirrored')");
    assert_eq!(event.attribution.source, "agent");
    assert_eq!(event.attribution.agent_name.as_deref(), Some("claude-code"));
    assert_eq!(event.attribution.tool, "execute_code");
    assert_eq!(event.attribution.workspace_id, workspace_id);

    // The event's msg_id is the parent of the iopub traffic it explains.
    let WebsocketMessage::Jupyter(echo) = &messages[echoed_at] else {
        unreachable!()
    };
    assert_eq!(
        echo.parent_header.as_ref().map(|h| h.msg_id.as_str()),
        Some(event.msg_id.as_str())
    );
}

#[tokio::test]
async fn test_execution_events_are_buffered_until_the_client_reconnects() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let session_id = start_session(&client, &python_cmd).await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[&session_id]).await;

    // Nothing is connected: this is the "browser tab closed" case.
    let result = agent
        .call_tool(
            "execute_code",
            json!({ "code": "print('while you were out')", "session_id": session_id }),
        )
        .await;
    assert!(!result.is_error, "{:?}", result);

    let mut ws = connect_session_ws(&server, &session_id).await;
    let messages = collect_until(&mut ws, Duration::from_secs(10), |message| {
        matches!(message, WebsocketMessage::Jupyter(msg) if msg.header.msg_type == "stream")
    })
    .await;

    let announced_at = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                WebsocketMessage::Kernel(KernelMessage::ExecutionRequested(_))
            )
        })
        .expect("The buffered attribution event should arrive on reconnect");
    let streamed_at = messages
        .iter()
        .position(|message| {
            matches!(message, WebsocketMessage::Jupyter(msg) if msg.header.msg_type == "stream")
        })
        .expect("The buffered output should arrive on reconnect");
    assert!(
        announced_at < streamed_at,
        "Buffered messages should be replayed in order"
    );

    let WebsocketMessage::Jupyter(stream) = &messages[streamed_at] else {
        unreachable!()
    };
    assert!(stream.content["text"]
        .as_str()
        .unwrap()
        .contains("while you were out"));
}

#[tokio::test]
async fn test_execute_code_times_out_and_interrupts_the_kernel() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let session_id = start_session(&client, &python_cmd).await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[&session_id]).await;

    let result = agent
        .call_tool(
            "execute_code",
            json!({
                "code": "import time; time.sleep(30)",
                "session_id": session_id,
                "timeout_s": 2,
            }),
        )
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("status"), &json!("timed_out"));

    // The kernel is interrupted, so it comes back rather than staying stuck.
    wait_for_status(&client, &session_id, Status::Idle, Duration::from_secs(15)).await;

    let result = agent
        .call_tool(
            "execute_code",
            json!({ "code": "'still here'", "session_id": session_id }),
        )
        .await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(
        result.structured["result"]["text/plain"],
        json!("'still here'")
    );
}

#[tokio::test]
async fn test_interrupt_session_stops_a_running_cell() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let session_id = start_session(&client, &python_cmd).await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[&session_id]).await;

    let mut runner = agent.another();
    runner.initialize().await;
    let running_session = session_id.clone();
    let long_running = tokio::spawn(async move {
        runner
            .call_tool(
                "execute_code",
                json!({
                    "code": "import time; time.sleep(30)",
                    "session_id": running_session,
                    "timeout_s": 60,
                }),
            )
            .await
    });

    wait_for_status(&client, &session_id, Status::Busy, Duration::from_secs(15)).await;

    let result = agent
        .call_tool("interrupt_session", json!({ "session_id": session_id }))
        .await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("status"), &json!("ok"));

    let interrupted = tokio::time::timeout(Duration::from_secs(20), long_running)
        .await
        .expect("The interrupted execution should return promptly")
        .expect("Tool call panicked");
    assert!(interrupted.is_error, "{:?}", interrupted);
    wait_for_status(&client, &session_id, Status::Idle, Duration::from_secs(15)).await;
}

#[tokio::test]
async fn test_evaluate_code_refuses_a_busy_session_unless_asked_to_wait() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let session_id = start_session(&client, &python_cmd).await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[&session_id]).await;

    let mut runner = agent.another();
    runner.initialize().await;
    let running_session = session_id.clone();
    let long_running = tokio::spawn(async move {
        runner
            .call_tool(
                "execute_code",
                json!({
                    "code": "import time; time.sleep(4)",
                    "session_id": running_session,
                    "timeout_s": 30,
                }),
            )
            .await
    });

    wait_for_status(&client, &session_id, Status::Busy, Duration::from_secs(15)).await;

    let result = agent
        .call_tool(
            "evaluate_code",
            json!({ "code": "1 + 1", "session_id": session_id }),
        )
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("RUNTIME_BUSY"));

    // With wait=true the evaluation queues behind the running cell instead.
    let result = agent
        .call_tool(
            "evaluate_code",
            json!({ "code": "1 + 1", "session_id": session_id, "wait": true, "timeout_s": 30 }),
        )
        .await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.structured["result"]["text/plain"], json!("2"));

    long_running.await.expect("Tool call panicked");
}

#[tokio::test]
async fn test_session_targeting_uses_the_foreground_session() {
    let python_cmd = require_ipykernel!();
    let server = TestServer::start().await;
    let client = server.create_client().await;
    let first = start_session(&client, &python_cmd).await;
    let (_workspace_id, window, mut agent) = agent_for(&server, &[&first]).await;

    // One session and no foreground: the only session the window holds is
    // unambiguous.
    let result = agent
        .call_tool("execute_code", json!({ "code": "'only one'" }))
        .await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("session_id"), &json!(first));

    // Two sessions and no foreground: refuse and say which are available.
    let second = start_session(&client, &python_cmd).await;
    window.send(FrontendMessage::SessionsChanged(SessionsChanged {
        session_ids: vec![first.clone(), second.clone()],
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let result = agent
        .call_tool("execute_code", json!({ "code": "'ambiguous'" }))
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("NO_SESSION_SELECTED"));
    let candidates = result.field("candidates").as_array().unwrap().clone();
    assert_eq!(candidates.len(), 2, "{:?}", result);

    // Once the window names its foreground session, targeting resolves again.
    window.send(FrontendMessage::ForegroundChanged(ForegroundChanged {
        session_id: Some(second.clone()),
    }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let result = agent
        .call_tool("execute_code", json!({ "code": "'foreground'" }))
        .await;
    assert!(!result.is_error, "{:?}", result);
    assert_eq!(result.field("session_id"), &json!(second));

    // list_sessions reports which one has focus.
    let result = agent.call_tool("list_sessions", json!({})).await;
    let sessions = result.field("sessions").as_array().unwrap().clone();
    assert_eq!(sessions.len(), 2);
    let foreground: Vec<&Value> = sessions
        .iter()
        .filter(|s| s["is_foreground"] == json!(true))
        .collect();
    assert_eq!(foreground.len(), 1);
    assert_eq!(foreground[0]["session_id"], json!(second));
    assert_eq!(foreground[0]["language"], json!("python"));
    assert_eq!(foreground[0]["mode"], json!("console"));
}

#[tokio::test]
async fn test_unknown_session_is_reported_not_guessed() {
    let server = TestServer::start().await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[]).await;

    let result = agent
        .call_tool(
            "execute_code",
            json!({ "code": "1", "session_id": "no-such-session" }),
        )
        .await;
    assert!(result.is_error, "{:?}", result);
    assert_eq!(result.field("code"), &json!("SESSION_NOT_FOUND"));
}

#[tokio::test]
async fn test_mcp_tool_calls_postpone_idle_shutdown() {
    // With `--idle-shutdown-hours 0` the supervisor exits after 30 seconds of
    // inactivity. A tool call inside that window has to reset the timer, or an
    // agent working against a closed Positron would lose its sessions.
    let mut server = TestServer::start_http_with_args(&["--idle-shutdown-hours", "0"]).await;
    let (_workspace_id, _window, mut agent) = agent_for(&server, &[]).await;

    tokio::time::sleep(Duration::from_secs(20)).await;
    assert!(
        server.is_running(),
        "The supervisor should not have shut down yet"
    );

    let result = agent.call_tool("list_sessions", json!({})).await;
    assert!(!result.is_error, "{:?}", result);

    tokio::time::sleep(Duration::from_secs(20)).await;
    assert!(
        server.is_running(),
        "The tool call should have reset the idle timer"
    );
}
