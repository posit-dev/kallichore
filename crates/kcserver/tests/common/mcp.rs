//
// mcp.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Test doubles for the two clients of the MCP server: an external agent
//! speaking Streamable HTTP, and a Positron frontend on the command channel.
//!
//! Both go over the wire, so the tests exercise the real HTTP stack, the real
//! auth guards, and the real WebSocket upgrade.

#![allow(dead_code)]

use std::time::Duration;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use kallichore_api::models::McpClient;
use kcshared::mcp_frontend::{
    CommandReply, CommandRequest, FrontendMessage, ServerFrontendMessage,
};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// A raw HTTP response, before any MCP interpretation.
pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: hyper::HeaderMap,
    pub body: String,
}

impl HttpResponse {
    /// The JSON-RPC payload, whether it arrived as JSON or as a single SSE
    /// event.
    pub fn json(&self) -> Value {
        let content_type = self
            .headers
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if content_type.starts_with("text/event-stream") {
            // The stream opens with an empty priming event, so take the first
            // data line that actually carries a message.
            return self
                .body
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .find_map(|payload| serde_json::from_str(payload.trim()).ok())
                .unwrap_or_else(|| panic!("No JSON-RPC message in SSE body: {}", self.body));
        }
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("Response body is not JSON ({}): {}", e, self.body))
    }
}

/// POST a body to the MCP listener with exactly the headers given.
pub async fn post(port: u16, path: &str, headers: &[(&str, String)], body: &str) -> HttpResponse {
    send("POST", port, path, headers, body).await
}

/// GET a path from the MCP listener with exactly the headers given.
pub async fn get(port: u16, path: &str, headers: &[(&str, String)]) -> HttpResponse {
    send("GET", port, path, headers, "").await
}

/// Make one request to the MCP listener, with no headers but the ones given.
async fn send(
    method: &str,
    port: u16,
    path: &str,
    headers: &[(&str, String)],
    body: &str,
) -> HttpResponse {
    let stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("Failed to connect to the MCP listener");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("Failed to start an HTTP connection");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(
            HeaderName::from_bytes(name.as_bytes()).expect("Invalid header name"),
            HeaderValue::from_str(value).expect("Invalid header value"),
        );
    }
    let request = builder
        .body(Full::new(Bytes::from(body.to_string())))
        .expect("Failed to build request");

    let response = sender
        .send_request(request)
        .await
        .expect("Failed to send request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("Failed to read response body")
        .to_bytes();

    HttpResponse {
        status,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    }
}

/// The outcome of a `tools/call`.
#[derive(Debug)]
pub struct ToolCall {
    pub is_error: bool,
    pub structured: Value,
    pub content: Vec<Value>,
}

impl ToolCall {
    /// A field of the structured result.
    pub fn field(&self, name: &str) -> &Value {
        self.structured
            .get(name)
            .unwrap_or_else(|| panic!("No '{}' in result: {}", name, self.structured))
    }

    /// The content blocks of a given type, e.g. "image".
    pub fn blocks(&self, block_type: &str) -> Vec<&Value> {
        self.content
            .iter()
            .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some(block_type))
            .collect()
    }
}

/// An external agent talking to the MCP server.
pub struct McpAgent {
    port: u16,

    /// The workspace's endpoint, which names it in the path.
    path: String,

    token: String,
    name: String,
    version: String,
    session_id: Option<String>,
    next_id: i64,
}

impl McpAgent {
    /// Create an agent talking to one workspace's endpoint with its token.
    pub fn new(port: u16, workspace_id: &str, token: &str) -> Self {
        Self {
            port,
            path: format!("/mcp/w/{}", workspace_id),
            token: token.to_string(),
            name: "test-agent".to_string(),
            version: "1.2.3".to_string(),
            session_id: None,
            next_id: 0,
        }
    }

    /// A second agent on the same endpoint, as a second terminal in the same
    /// window would be.
    pub fn another(&self) -> Self {
        Self {
            port: self.port,
            path: self.path.clone(),
            token: self.token.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            session_id: None,
            next_id: 0,
        }
    }

    /// Talk to the endpoint as a client running inside one of the workspace's
    /// kernels does: the same workspace and token, with its own session named.
    pub fn in_session(mut self, session_id: &str) -> Self {
        self.path = format!("{}/s/{}", self.path, session_id);
        self
    }

    /// Set the name this agent reports in `clientInfo`.
    pub fn named(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// The MCP listener port this agent talks to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The bearer token this agent presents.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The path of the endpoint this agent posts to.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Complete the MCP handshake, returning the `initialize` result.
    pub async fn initialize(&mut self) -> Value {
        let response = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": self.name, "version": self.version },
                }),
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "initialize: {}",
            response.body
        );
        self.session_id = response
            .headers
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let result = expect_result(response.json());
        self.notify("notifications/initialized", json!({})).await;
        result
    }

    /// List the server's tools, keyed by name.
    pub async fn list_tools(&mut self) -> Vec<Value> {
        let response = self.request("tools/list", json!({})).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "tools/list: {}",
            response.body
        );
        expect_result(response.json())
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default()
    }

    /// Call a tool and unpack its result.
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> ToolCall {
        let response = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "tools/call {}: {}",
            name,
            response.body
        );
        tool_call(expect_result(response.json()))
    }

    /// Send a JSON-RPC request with this agent's credentials and session.
    pub async fn request(&mut self, method: &str, params: Value) -> HttpResponse {
        self.next_id += 1;
        let body = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params,
        });
        post(self.port, &self.path, &self.headers(), &body.to_string()).await
    }

    /// Send a JSON-RPC notification, which expects no response body.
    async fn notify(&self, method: &str, params: Value) {
        let body = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        post(self.port, &self.path, &self.headers(), &body.to_string()).await;
    }

    /// The headers every request carries.
    fn headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = vec![
            ("content-type", "application/json".to_string()),
            ("accept", "application/json, text/event-stream".to_string()),
            ("authorization", format!("Bearer {}", self.token)),
            ("host", format!("127.0.0.1:{}", self.port)),
        ];
        if let Some(session_id) = &self.session_id {
            headers.push(("mcp-session-id", session_id.clone()));
        }
        headers
    }
}

/// Unpack a `tools/call` result.
fn tool_call(result: Value) -> ToolCall {
    ToolCall {
        is_error: result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        structured: result
            .get("structuredContent")
            .cloned()
            .unwrap_or(Value::Null),
        content: result
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

/// An agent that starts `kcserver mcp-stdio` as its MCP server and talks to it
/// over the child's stdin and stdout.
///
/// Every line the child writes to stdout must be a JSON-RPC message, so reading
/// one that is not fails the test.
pub struct StdioAgent {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,

    /// Messages read while waiting for a different one.
    unclaimed: Vec<Value>,

    name: String,
    next_id: i64,
}

impl StdioAgent {
    /// Start the bridge with the given arguments and environment, in the given
    /// working directory. The Positron variables are cleared first, so the
    /// bridge sees only what the test gives it.
    pub fn spawn(args: &[&str], env: &[(&str, &str)], working_dir: &std::path::Path) -> Self {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_kcserver"));
        command
            .arg("mcp-stdio")
            .args(args)
            .current_dir(working_dir)
            .env_remove("POSITRON_MCP_URL")
            .env_remove("POSITRON_MCP_TOKEN")
            .env("RUST_LOG", "debug")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("Failed to start kcserver mcp-stdio");
        let stdin = child.stdin.take().expect("No stdin");
        let stdout = child.stdout.take().expect("No stdout");
        Self {
            child,
            stdin,
            stdout: tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout)),
            unclaimed: Vec::new(),
            name: "stdio-test-agent".to_string(),
            next_id: 0,
        }
    }

    /// Set the name this agent reports in `clientInfo`.
    pub fn named(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// The bridge's process ID.
    pub fn pid(&self) -> u32 {
        self.child.id().expect("The bridge has exited")
    }

    /// Kill the bridge outright, as an agent that crashes takes its servers
    /// with it, and wait for it to die.
    pub async fn kill(mut self) {
        self.child.kill().await.expect("Failed to kill the bridge");
    }

    /// Write one message to the bridge.
    pub async fn send(&mut self, message: Value) {
        use tokio::io::AsyncWriteExt;
        let line = format!("{}\n", message);
        self.stdin
            .write_all(line.as_bytes())
            .await
            .expect("Failed to write to the bridge");
    }

    /// Send a request without waiting for its response, returning its ID.
    pub async fn send_request(&mut self, method: &str, params: Value) -> i64 {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        id
    }

    /// The next message the bridge writes, whatever it is.
    pub async fn next_message(&mut self) -> Value {
        if !self.unclaimed.is_empty() {
            return self.unclaimed.remove(0);
        }
        self.read_message().await
    }

    /// Wait for the response to a request, keeping anything else for later.
    pub async fn response(&mut self, id: i64) -> Value {
        if let Some(index) = self.unclaimed.iter().position(|m| m["id"] == json!(id)) {
            return self.unclaimed.remove(index);
        }
        loop {
            let message = self.read_message().await;
            if message["id"] == json!(id) {
                return message;
            }
            self.unclaimed.push(message);
        }
    }

    /// Send a request and return its result.
    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send_request(method, params).await;
        expect_result(self.response(id).await)
    }

    /// Complete the MCP handshake, returning the `initialize` result.
    pub async fn initialize(&mut self) -> Value {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": self.name, "version": "1.0.0" },
                }),
            )
            .await;
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        result
    }

    /// The names of the tools the bridge lists.
    pub async fn tool_names(&mut self) -> Vec<String> {
        self.request("tools/list", json!({}))
            .await
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect()
    }

    /// Call a tool and unpack its result.
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> ToolCall {
        tool_call(
            self.request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await,
        )
    }

    /// Close stdin, as an agent shutting down does, and wait for the bridge
    /// to exit.
    pub async fn shut_down(mut self) -> std::process::ExitStatus {
        drop(self.stdin);
        tokio::time::timeout(Duration::from_secs(10), self.child.wait())
            .await
            .expect("The bridge did not exit when its stdin closed")
            .expect("Failed to wait for the bridge")
    }

    async fn read_message(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(30), self.stdout.next_line())
            .await
            .expect("Timed out waiting for the bridge")
            .expect("Failed to read from the bridge")
            .expect("The bridge closed its stdout");
        serde_json::from_str(&line).unwrap_or_else(|e| {
            panic!("The bridge wrote a line that is not JSON ({}): {}", e, line)
        })
    }
}

/// Unwrap a JSON-RPC response, panicking with the error when there is one.
fn expect_result(response: Value) -> Value {
    if let Some(error) = response.get("error") {
        panic!("JSON-RPC error: {}", error);
    }
    response
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("No result in response: {}", response))
}

/// A stand-in for the Positron window on the other end of the frontend channel.
pub struct SimulatedFrontend {
    outbound: mpsc::UnboundedSender<FrontendMessage>,
    requests: mpsc::UnboundedReceiver<CommandRequest>,
    clients: mpsc::UnboundedReceiver<Vec<McpClient>>,
    closer: mpsc::UnboundedSender<()>,
}

impl SimulatedFrontend {
    /// Connect the frontend channel and start pumping it.
    pub async fn connect(base_url: &str, workspace_id: &str) -> Self {
        let url = format!(
            "{}/mcp/workspaces/{}/channel",
            base_url.replace("http://", "ws://"),
            workspace_id
        );
        let (stream, response) = tokio_tungstenite::connect_async(&url)
            .await
            .unwrap_or_else(|e| panic!("Failed to open the frontend channel at {}: {}", url, e));
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

        let (mut sink, mut source) = stream.split();
        let (outbound, mut outbound_rx) = mpsc::unbounded_channel::<FrontendMessage>();
        let (request_tx, requests) = mpsc::unbounded_channel::<CommandRequest>();
        let (clients_tx, clients) = mpsc::unbounded_channel::<Vec<McpClient>>();
        let (closer, mut close_rx) = mpsc::unbounded_channel::<()>();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = close_rx.recv() => {
                        let _ = sink.send(Message::Close(None)).await;
                        break;
                    }
                    message = outbound_rx.recv() => {
                        let Some(message) = message else { break };
                        let text = serde_json::to_string(&message).expect("Failed to serialize");
                        if sink.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    incoming = source.next() => {
                        match incoming {
                            Some(Ok(Message::Text(text))) => {
                                let message: ServerFrontendMessage = serde_json::from_str(&text)
                                    .unwrap_or_else(|e| panic!("Unreadable server frame ({}): {}", e, text));
                                match message {
                                    ServerFrontendMessage::CommandRequest(request) => {
                                        let _ = request_tx.send(request);
                                    }
                                    ServerFrontendMessage::ClientsChanged(changed) => {
                                        let _ = clients_tx.send(changed.clients);
                                    }
                                }
                            }
                            Some(Ok(_)) => {}
                            Some(Err(_)) | None => break,
                        }
                    }
                }
            }
        });

        Self {
            outbound,
            requests,
            clients,
            closer,
        }
    }

    /// Send a frame to the supervisor.
    pub fn send(&self, message: FrontendMessage) {
        self.outbound
            .send(message)
            .expect("Frontend channel closed");
    }

    /// Wait for the next brokered command request.
    pub async fn next_command(&mut self, timeout: Duration) -> CommandRequest {
        tokio::time::timeout(timeout, self.requests.recv())
            .await
            .expect("Timed out waiting for a command request")
            .expect("Frontend channel closed")
    }

    /// Wait until the supervisor reports a client list that satisfies `done`,
    /// returning it.
    pub async fn wait_for_clients(
        &mut self,
        timeout: Duration,
        done: impl Fn(&[McpClient]) -> bool,
    ) -> Vec<McpClient> {
        tokio::time::timeout(timeout, async {
            loop {
                let clients = self.clients.recv().await.expect("Frontend channel closed");
                if done(&clients) {
                    return clients;
                }
            }
        })
        .await
        .expect("Timed out waiting for the client list")
    }

    /// Answer a brokered command request.
    pub fn reply(&self, reply: CommandReply) {
        self.send(FrontendMessage::CommandReply(reply));
    }

    /// Close the channel and wait for the supervisor to notice.
    pub async fn disconnect(self) {
        let _ = self.closer.send(());
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
