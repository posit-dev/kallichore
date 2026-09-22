//
// stdio_bridge.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! A stdio MCP server that relays to a workspace's HTTP endpoint.
//!
//! Almost every MCP client can start a server as a child process and talk to
//! it over stdin and stdout, and a client configured that way needs nothing
//! but a command line: no URL, no token, nothing that goes stale when the
//! supervisor restarts. `kcserver mcp-stdio` is that command. It holds no
//! sessions of its own; it finds the endpoint of the workspace the client is
//! working in and relays each JSON-RPC message to it over HTTP, so the server
//! the client ends up talking to is the same one every other agent reaches.
//!
//! The endpoint is found in this order:
//!
//! 1. The workspace named with `--workspace`, looked up in the connections
//!    directory.
//! 2. `POSITRON_MCP_URL` and `POSITRON_MCP_TOKEN`, which Positron publishes to
//!    its terminals and `kcserver` to the kernels it starts. When that
//!    endpoint stops answering, its workspace is looked up in the connections
//!    directory instead, which is where a supervisor restart leaves its new
//!    address.
//! 3. The workspace in the connections directory whose folder contains the
//!    bridge's working directory.
//!
//! Nothing is resolved until it is needed, and a failure is resolved again, so
//! a client started before Positron, or across a supervisor restart, never has
//! to reconnect. While no endpoint answers, the bridge answers by itself: the
//! handshake succeeds, the tool list is the real one, and every tool call
//! explains that Positron is not reachable.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rmcp::model::{
    CallToolResult, DiscoverResult, InitializeRequestParams, InitializeResult, ListToolsResult,
};
use rmcp::ServerHandler;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;

use super::handler::{server_info, PositronMcpHandler, INSTRUCTIONS};
use super::listener::{CALLER_PATH_SEGMENT, MCP_PATH_PREFIX};
use super::{MCP_TOKEN_VAR, MCP_URL_VAR};

/// The index Positron keeps in the connections directory.
const INDEX_FILE: &str = "connections.json";

/// The header carrying a pre-2026-07-28 protocol session.
const SESSION_HEADER: &str = "mcp-session-id";

/// The header naming the protocol version a request speaks.
const PROTOCOL_HEADER: &str = "mcp-protocol-version";

/// Where a 2026-07-28 request names its protocol version.
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// The first protocol revision whose results carry `resultType`.
const RESULT_TYPE_VERSION: &str = "2026-07-28";

/// How long to wait for the listener to accept a connection. It is on
/// loopback, so a refusal is immediate and this only bounds a wedged one.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to spend ending the protocol session on the way out.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// How the bridge was asked to find its workspace.
#[derive(Debug, Default, Clone)]
pub struct BridgeOptions {
    /// Positron's connections directory, holding `connections.json` and the
    /// descriptors it points at.
    pub connections: Option<PathBuf>,

    /// A workspace ID to use regardless of the environment and working
    /// directory, for clients whose working directory means nothing.
    pub workspace: Option<String>,
}

/// Serve MCP over stdin and stdout until stdin closes.
pub async fn run(options: BridgeOptions) -> std::io::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // One writer, so that messages from concurrent requests never interleave
    // within a line.
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = rx.recv().await {
            let written = async {
                stdout.write_all(line.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await
            };
            if let Err(e) = written.await {
                log::error!("Failed to write to stdout: {}", e);
                break;
            }
        }
    });

    let bridge = Arc::new(Bridge {
        link: Mutex::new(Link::new(Resolver::from_environment(options))),
        out: tx,
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut requests = JoinSet::new();
    while let Some(line) = lines.next_line().await? {
        while requests.try_join_next().is_some() {}

        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(e) => {
                bridge.send(&error_response(
                    &Value::Null,
                    -32700,
                    &format!("Parse error: {}", e),
                ));
                continue;
            }
        };

        // The handshake and notifications are handled in order, since the
        // server must see `initialize` and `notifications/initialized` before
        // anything that follows them. Requests run concurrently, so a long
        // execution does not hold up a quick listing, or the cancellation
        // meant to stop it.
        match message.get("method").and_then(Value::as_str) {
            Some("initialize") if message.get("id").is_some() => bridge.initialize(message).await,
            Some(_) if message.get("id").is_none() => bridge.notify(message).await,
            _ => {
                let bridge = bridge.clone();
                requests.spawn(async move { bridge.request(message).await });
            }
        }
    }

    // Answer everything already asked before leaving.
    while requests.join_next().await.is_some() {}
    bridge.close().await;
    drop(bridge);
    let _ = writer.await;
    Ok(())
}

/// Why no endpoint is answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Offline {
    /// No workspace could be found for this client.
    NoWorkspace,

    /// A workspace was found, but its endpoint did not answer.
    Unreachable,
}

impl Offline {
    /// The code tool calls report.
    fn code(self) -> &'static str {
        match self {
            Offline::NoWorkspace => "NO_WORKSPACE",
            Offline::Unreachable => "POSITRON_NOT_RUNNING",
        }
    }

    /// What the agent should tell the user.
    fn message(self) -> &'static str {
        match self {
            Offline::NoWorkspace => {
                "No Positron workspace is open for this directory. Ask the user to open \
                 this folder in Positron with the 'ai.mcp.enabled' setting on, then try \
                 again; there is no need to reconnect."
            }
            Offline::Unreachable => {
                "Positron is not reachable: the workspace is closed, or its MCP server is \
                 turned off. Ask the user to open the workspace in Positron, then try again; \
                 there is no need to reconnect."
            }
        }
    }
}

/// A workspace's HTTP endpoint and the token it accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    host: String,
    port: u16,

    /// The endpoint's path, including any `/s/<session>` suffix.
    path: String,

    token: String,
}

impl Endpoint {
    /// Parse an endpoint URL, as Positron and `kcserver` publish it.
    fn parse(url: &str, token: &str) -> Option<Self> {
        let uri: hyper::Uri = url.parse().ok()?;
        if uri.scheme_str() != Some("http") {
            return None;
        }
        Some(Self {
            host: uri.host()?.to_string(),
            port: uri.port_u16()?,
            path: uri.path().to_string(),
            token: token.to_string(),
        })
    }

    /// The workspace the endpoint belongs to.
    fn workspace_id(&self) -> Option<&str> {
        let rest = self.path.strip_prefix(MCP_PATH_PREFIX)?;
        let id = rest
            .split(CALLER_PATH_SEGMENT)
            .next()?
            .trim_end_matches('/');
        (!id.is_empty()).then_some(id)
    }

    /// The `/s/<session>` suffix naming the caller's own session, if any.
    fn caller_suffix(&self) -> Option<&str> {
        self.path
            .find(CALLER_PATH_SEGMENT)
            .map(|start| &self.path[start..])
    }

    /// The same endpoint with a caller's session named, if there is one.
    fn with_caller(mut self, suffix: Option<&str>) -> Self {
        if let Some(suffix) = suffix {
            self.path = format!("{}{}", self.path.trim_end_matches('/'), suffix);
        }
        self
    }
}

/// Positron's index of the workspaces registered on this machine.
#[derive(Deserialize)]
struct Index {
    workspaces: HashMap<String, IndexEntry>,
}

/// One workspace in the index.
#[derive(Deserialize)]
struct IndexEntry {
    /// The descriptor holding the workspace's token.
    descriptor: PathBuf,

    /// The workspace's folders.
    #[serde(default)]
    folders: Vec<PathBuf>,
}

/// The parts of a workspace's descriptor the bridge reads.
#[derive(Deserialize)]
struct Descriptor {
    url: String,
    token: String,
}

/// Finds the endpoint of the workspace this client belongs to.
struct Resolver {
    options: BridgeOptions,

    /// The endpoint published in the environment.
    environment: Option<Endpoint>,

    /// Whether the environment's endpoint has stopped answering, after which
    /// its workspace is looked up in the connections directory first.
    environment_stale: bool,

    /// The working directory, which chooses a workspace by its folders.
    working_dir: Option<PathBuf>,
}

impl Resolver {
    /// A resolver reading this process's environment and working directory.
    fn from_environment(options: BridgeOptions) -> Self {
        let environment = match (std::env::var(MCP_URL_VAR), std::env::var(MCP_TOKEN_VAR)) {
            (Ok(url), Ok(token)) => {
                let endpoint = Endpoint::parse(&url, &token);
                if endpoint.is_none() {
                    log::warn!(
                        "Ignoring {}, which is not an MCP endpoint: {}",
                        MCP_URL_VAR,
                        url
                    );
                }
                endpoint
            }
            _ => None,
        };
        Self {
            options,
            environment,
            environment_stale: false,
            working_dir: std::env::current_dir().ok(),
        }
    }

    /// The endpoint to use, given the one that just stopped answering.
    fn resolve(&mut self, stale: Option<&Endpoint>) -> Option<Endpoint> {
        if let Some(id) = self.options.workspace.clone() {
            return self.lookup(&id);
        }

        if let Some(environment) = self.environment.clone() {
            if stale == Some(&environment) {
                self.environment_stale = true;
            }
            if self.environment_stale {
                let moved = environment.workspace_id().and_then(|id| self.lookup(id));
                if let Some(moved) = moved {
                    return Some(moved.with_caller(environment.caller_suffix()));
                }
            }
            return Some(environment);
        }

        let id = self.workspace_for_working_dir()?;
        self.lookup(&id)
    }

    /// Read the connections index.
    fn index(&self) -> Option<Index> {
        let path = self.options.connections.as_ref()?.join(INDEX_FILE);
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(e) => {
                log::debug!("Could not read {}: {}", path.display(), e);
                return None;
            }
        };
        match serde_json::from_str(&contents) {
            Ok(index) => Some(index),
            Err(e) => {
                log::warn!("Could not parse {}: {}", path.display(), e);
                None
            }
        }
    }

    /// A workspace's current endpoint, from its descriptor.
    fn lookup(&self, workspace_id: &str) -> Option<Endpoint> {
        let index = self.index()?;
        let entry = index.workspaces.get(workspace_id)?;
        let contents = std::fs::read_to_string(&entry.descriptor)
            .map_err(|e| log::debug!("Could not read {}: {}", entry.descriptor.display(), e))
            .ok()?;
        let descriptor: Descriptor = serde_json::from_str(&contents)
            .map_err(|e| log::warn!("Could not parse {}: {}", entry.descriptor.display(), e))
            .ok()?;
        Endpoint::parse(&descriptor.url, &descriptor.token)
    }

    /// The workspace whose folder most closely contains the working directory.
    fn workspace_for_working_dir(&self) -> Option<String> {
        let working_dir = canonical(self.working_dir.as_ref()?);
        let index = self.index()?;
        index
            .workspaces
            .into_iter()
            .filter_map(|(id, entry)| {
                entry
                    .folders
                    .iter()
                    .map(|folder| canonical(folder))
                    .filter(|folder| working_dir.starts_with(folder))
                    .map(|folder| folder.components().count())
                    .max()
                    .map(|depth| (depth, id))
            })
            .max()
            .map(|(_, id)| id)
    }
}

/// A path with symbolic links resolved when it exists, so that `/tmp` and
/// `/private/tmp` compare equal; as given when it does not.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The connection to the endpoint, shared by every request.
struct Link {
    resolver: Resolver,

    /// The endpoint in use, when one is answering.
    upstream: Option<Endpoint>,

    /// The protocol session the endpoint issued at the handshake.
    session_id: Option<String>,

    /// Counts connections, so a request that failed on one that has since
    /// been replaced retries on the replacement rather than replacing it again.
    generation: u64,

    /// The client's `initialize`, replayed to every endpoint the bridge
    /// connects to so that the server hears the client's own name.
    handshake: Option<Value>,

    /// Whether the client has sent `notifications/initialized`.
    initialized: bool,

    /// The protocol version the handshake settled on.
    protocol_version: Option<String>,

    /// Why the last attempt to connect failed.
    offline: Offline,
}

impl Link {
    fn new(resolver: Resolver) -> Self {
        Self {
            resolver,
            upstream: None,
            session_id: None,
            generation: 0,
            handshake: None,
            initialized: false,
            protocol_version: None,
            offline: Offline::NoWorkspace,
        }
    }

    /// Find an endpoint and, once the client has shaken hands, shake hands with
    /// it on the client's behalf.
    ///
    /// The endpoint that was in use is taken as stale, so resolution can move
    /// past it. `out` receives the handshake's reply when the client is waiting
    /// for one; a replayed handshake's reply is the server's business.
    ///
    /// Returns whether the client has been answered.
    async fn connect(&mut self, out: Option<&Output>) -> bool {
        let mut stale = self.upstream.take();
        self.session_id = None;

        for _ in 0..2 {
            let Some(endpoint) = self.resolver.resolve(stale.as_ref()) else {
                self.offline = Offline::NoWorkspace;
                return false;
            };

            let Some(handshake) = self.handshake.clone() else {
                // A 2026-07-28 client has no handshake; its requests stand on
                // their own.
                self.connected(endpoint);
                return false;
            };

            let mut version = None;
            let outcome = post(&endpoint, None, None, &handshake, &mut |message| {
                if let Some(v) = message.pointer("/result/protocolVersion") {
                    version = v.as_str().map(str::to_string);
                }
                if let Some(out) = out {
                    let _ = out.send(message.to_string());
                }
            })
            .await;

            match outcome.status {
                Status::Done => {
                    self.protocol_version = version.or(self.protocol_version.take());
                    self.session_id = outcome.session_id;
                    if self.initialized {
                        let initialized =
                            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
                        self.post_notification(&endpoint, &initialized).await;
                    }
                    self.connected(endpoint);
                    return outcome.answered;
                }
                Status::Refused(reason) => {
                    log::info!("Positron's MCP endpoint did not answer: {}", reason);
                    stale = Some(endpoint);
                }
                Status::Failed(reason) => {
                    log::warn!("Positron's MCP endpoint refused the handshake: {}", reason);
                    self.offline = Offline::Unreachable;
                    return outcome.answered;
                }
            }
        }

        self.offline = Offline::Unreachable;
        false
    }

    /// Adopt an endpoint that answered.
    fn connected(&mut self, endpoint: Endpoint) {
        log::info!(
            "Relaying to Positron workspace {} at {}:{}",
            endpoint.workspace_id().unwrap_or("?"),
            endpoint.host,
            endpoint.port
        );
        self.upstream = Some(endpoint);
        self.generation += 1;
    }

    /// Send a notification, which expects no reply, on the current session.
    async fn post_notification(&self, endpoint: &Endpoint, message: &Value) {
        let outcome = post(
            endpoint,
            self.session_id.as_deref(),
            self.protocol_version.as_deref(),
            message,
            &mut |_| {},
        )
        .await;
        if let Status::Refused(reason) | Status::Failed(reason) = outcome.status {
            log::debug!("Dropped a notification: {}", reason);
        }
    }
}

/// Where lines for stdout go.
type Output = mpsc::UnboundedSender<String>;

/// The bridge: the link to the endpoint and the way back to the client.
struct Bridge {
    link: Mutex<Link>,
    out: Output,
}

impl Bridge {
    /// Write a message to the client.
    fn send(&self, message: &Value) {
        let _ = self.out.send(message.to_string());
    }

    /// Relay the client's `initialize`, or answer it here when no endpoint is
    /// answering.
    async fn initialize(&self, message: Value) {
        let mut link = self.link.lock().await;
        link.handshake = Some(message.clone());
        link.initialized = false;
        link.protocol_version = None;
        if link.connect(Some(&self.out)).await {
            return;
        }
        let offline = link.offline;
        if let Some(reply) = offline_reply(&message, offline, &mut link.protocol_version) {
            self.send(&reply);
        }
    }

    /// Relay a notification. Dropped when no endpoint is answering: nothing is
    /// waiting on it, and `notifications/initialized` is replayed on connect.
    async fn notify(&self, message: Value) {
        let mut link = self.link.lock().await;
        if message.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
            link.initialized = true;
        }
        if let Some(endpoint) = link.upstream.clone() {
            link.post_notification(&endpoint, &message).await;
        }
    }

    /// Relay a request, connecting first if need be, and make sure the client
    /// gets exactly one answer.
    async fn request(&self, message: Value) {
        let id = message.get("id").cloned().unwrap_or(Value::Null);

        for _ in 0..2 {
            let (endpoint, session_id, protocol_version, generation) = {
                let mut link = self.link.lock().await;
                if link.upstream.is_none() {
                    link.connect(None).await;
                }
                let Some(endpoint) = link.upstream.clone() else {
                    break;
                };
                (
                    endpoint,
                    link.session_id.clone(),
                    request_protocol_version(&message).or(link.protocol_version.clone()),
                    link.generation,
                )
            };

            let outcome = post(
                &endpoint,
                session_id.as_deref(),
                protocol_version.as_deref(),
                &message,
                &mut |reply| self.send(&reply),
            )
            .await;

            match outcome.status {
                Status::Done if outcome.answered || message.get("id").is_none() => return,
                Status::Done => {
                    self.send(&error_response(
                        &id,
                        -32603,
                        "Positron's MCP endpoint closed the response without answering.",
                    ));
                    return;
                }
                // The request never reached a tool, so it is safe to send
                // again once the link has been re-established.
                Status::Refused(reason) => {
                    log::info!("Positron's MCP endpoint did not answer: {}", reason);
                    let mut link = self.link.lock().await;
                    if link.generation == generation {
                        link.connect(None).await;
                    }
                }
                // The request may have run, so it must not run twice.
                Status::Failed(reason) => {
                    if !outcome.answered {
                        self.send(&error_response(&id, -32603, &reason));
                    }
                    return;
                }
            }
        }

        let mut link = self.link.lock().await;
        let offline = link.offline;
        if let Some(reply) = offline_reply(&message, offline, &mut link.protocol_version) {
            self.send(&reply);
        }
    }

    /// End the protocol session, if there is one, as the client goes away.
    async fn close(&self) {
        let link = self.link.lock().await;
        let (Some(endpoint), Some(session_id)) = (&link.upstream, &link.session_id) else {
            return;
        };
        let request = base_request(Method::DELETE, endpoint)
            .header(SESSION_HEADER, session_id)
            .body(Full::new(Bytes::new()));
        if let Ok(request) = request {
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, send(endpoint, request)).await;
        }
    }
}

/// How a relayed message fared.
enum Status {
    /// The endpoint accepted it.
    Done,

    /// It never reached a handler: nothing listening, a bad token, an unknown
    /// session. Safe to send again elsewhere.
    Refused(String),

    /// It reached the server, which may have acted on it.
    Failed(String),
}

/// The result of relaying one message.
struct Outcome {
    status: Status,

    /// Whether a reply to the message itself was delivered.
    answered: bool,

    /// The protocol session the server issued, if it issued one.
    session_id: Option<String>,
}

/// POST one message to an endpoint, delivering every message that comes back
/// as it arrives.
async fn post(
    endpoint: &Endpoint,
    session_id: Option<&str>,
    protocol_version: Option<&str>,
    message: &Value,
    deliver: &mut (dyn FnMut(Value) + Send),
) -> Outcome {
    let id = message.get("id").cloned();
    let mut answered = false;
    let mut deliver = |reply: Value| {
        if id.is_some() && reply.get("id") == id.as_ref() {
            answered = true;
        }
        deliver(reply);
    };

    let mut builder = base_request(Method::POST, endpoint)
        .header(CONTENT_TYPE, "application/json")
        .header(hyper::header::ACCEPT, "application/json, text/event-stream");
    if let Some(session_id) = session_id {
        builder = builder.header(SESSION_HEADER, session_id);
    }
    if let Some(version) = protocol_version {
        builder = builder.header(PROTOCOL_HEADER, version);
    }
    let request = match builder.body(Full::new(Bytes::from(message.to_string()))) {
        Ok(request) => request,
        Err(e) => {
            return Outcome {
                status: Status::Failed(format!("Could not build the request: {}", e)),
                answered,
                session_id: None,
            }
        }
    };

    let response = match send(endpoint, request).await {
        Ok(response) => response,
        Err(status) => {
            return Outcome {
                status,
                answered,
                session_id: None,
            }
        }
    };

    let status = response.status();
    let issued_session = response
        .headers()
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let is_stream = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));

    // Refusals from the listener and from the protocol layer's session check
    // happen before any tool is reached.
    if matches!(
        status,
        StatusCode::UNAUTHORIZED
            | StatusCode::FORBIDDEN
            | StatusCode::NOT_FOUND
            | StatusCode::SERVICE_UNAVAILABLE
    ) {
        let body = read_body(response.into_body()).await.unwrap_or_default();
        return Outcome {
            status: Status::Refused(format!("{}: {}", status, body.trim())),
            answered,
            session_id: None,
        };
    }

    let body = response.into_body();
    let result = if is_stream {
        read_events(body, &mut deliver).await
    } else {
        read_body(body).await.map(|text| {
            if let Ok(reply) = serde_json::from_str::<Value>(&text) {
                deliver(reply);
            } else if !text.trim().is_empty() && !status.is_success() {
                log::warn!(
                    "Positron's MCP endpoint answered {}: {}",
                    status,
                    text.trim()
                );
            }
        })
    };

    let status = match result {
        Err(e) => Status::Failed(format!(
            "The connection to Positron was lost while the request was in progress, \
             so it may or may not have run: {}",
            e
        )),
        Ok(()) if status.is_success() => Status::Done,
        Ok(()) => Status::Failed(format!("Positron's MCP endpoint answered {}", status)),
    };
    Outcome {
        status,
        answered,
        session_id: issued_session,
    }
}

/// A request to an endpoint with the headers every request carries.
fn base_request(method: Method, endpoint: &Endpoint) -> http::request::Builder {
    Request::builder()
        .method(method)
        .uri(endpoint.path.as_str())
        .header(HOST, format!("{}:{}", endpoint.host, endpoint.port))
        .header(AUTHORIZATION, format!("Bearer {}", endpoint.token))
}

/// Send a request over a fresh loopback connection.
///
/// A connection that cannot be made is a refusal; a request that cannot be
/// delivered over one that was made may have been read, so it is a failure.
async fn send(
    endpoint: &Endpoint,
    request: Request<Full<Bytes>>,
) -> Result<Response<Incoming>, Status> {
    let address = (endpoint.host.as_str(), endpoint.port);
    let stream = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => return Err(Status::Refused(e.to_string())),
        Err(_) => return Err(Status::Refused("timed out connecting".to_string())),
    };
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| Status::Refused(e.to_string()))?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            log::debug!("MCP relay connection ended: {}", e);
        }
    });
    sender
        .send_request(request)
        .await
        .map_err(|e| Status::Failed(format!("Could not deliver the request to Positron: {}", e)))
}

/// Read a whole response body as text.
async fn read_body(body: Incoming) -> Result<String, hyper::Error> {
    let bytes = body.collect().await?.to_bytes();
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Read a server-sent event stream, delivering each event's message as soon as
/// the event is complete, so progress notifications arrive while a tool runs.
async fn read_events(
    mut body: Incoming,
    deliver: &mut (dyn FnMut(Value) + Send),
) -> Result<(), hyper::Error> {
    let mut buffer = String::new();
    while let Some(frame) = body.frame().await {
        let Ok(data) = frame?.into_data() else {
            continue;
        };
        buffer.push_str(&String::from_utf8_lossy(&data).replace('\r', ""));
        while let Some(end) = buffer.find("\n\n") {
            let event: String = buffer.drain(..end + 2).collect();
            deliver_event(&event, deliver);
        }
    }
    deliver_event(&buffer, deliver);
    Ok(())
}

/// Deliver the message carried by one event, if it carries one. Priming events
/// carry no data, and comments and IDs are no concern of the client's.
fn deliver_event(event: &str, deliver: &mut (dyn FnMut(Value) + Send)) {
    let data: Vec<&str> = event
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| data.strip_prefix(' ').unwrap_or(data))
        .collect();
    if data.is_empty() {
        return;
    }
    match serde_json::from_str(&data.join("\n")) {
        Ok(message) => deliver(message),
        Err(e) => log::debug!("Ignoring an event that is not a JSON-RPC message: {}", e),
    }
}

/// The protocol version a 2026-07-28 request names for itself.
fn request_protocol_version(message: &Value) -> Option<String> {
    message
        .get("params")?
        .get("_meta")?
        .get(META_PROTOCOL_VERSION)?
        .as_str()
        .map(str::to_string)
}

/// A JSON-RPC error response.
fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Answers the handshake the way the real server would, with instructions
/// saying why it is not the real server.
struct OfflineServer(String);

impl ServerHandler for OfflineServer {
    fn get_info(&self) -> InitializeResult {
        server_info(self.0.clone())
    }
}

/// Answer a request with no endpoint to relay it to.
///
/// The handshake and the tool list are answered as the server would answer
/// them, so the client sees the same tools whether or not Positron is up and
/// has nothing to reconnect once it is; tool calls say what the user has to do.
fn offline_reply(
    message: &Value,
    offline: Offline,
    protocol_version: &mut Option<String>,
) -> Option<Value> {
    let id = message.get("id")?;
    let server = OfflineServer(format!(
        "{}\n\nPositron is not reachable right now. {}",
        INSTRUCTIONS,
        offline.message()
    ));

    let result = match message.get("method").and_then(Value::as_str) {
        Some("initialize") => {
            let params: InitializeRequestParams =
                match serde_json::from_value(message.get("params").cloned().unwrap_or_default()) {
                    Ok(params) => params,
                    Err(e) => return Some(error_response(id, -32602, &e.to_string())),
                };
            match server.negotiate_initialize(&params) {
                Ok(result) => {
                    *protocol_version = Some(result.protocol_version.to_string());
                    serde_json::to_value(result)
                }
                Err(e) => return Some(error_response(id, e.code.0 as i64, &e.message)),
            }
        }
        Some("server/discover") => serde_json::to_value(DiscoverResult::from_server_info(
            server.supported_protocol_versions().into_owned(),
            server.get_info(),
        )),
        Some("tools/list") => serde_json::to_value(ListToolsResult::with_all_items(
            PositronMcpHandler::tool_list(),
        )),
        Some("tools/call") => serde_json::to_value(CallToolResult::structured_error(json!({
            "status": "error",
            "code": offline.code(),
            "message": offline.message(),
        }))),
        Some("ping") => Ok(json!({})),
        _ => return Some(error_response(id, -32603, offline.message())),
    };

    let mut result = match result {
        Ok(result) => result,
        Err(e) => return Some(error_response(id, -32603, &e.to_string())),
    };
    // Results name their type only from 2026-07-28 on.
    let version = request_protocol_version(message).or(protocol_version.clone());
    if version.is_none_or(|v| v.as_str() < RESULT_TYPE_VERSION) {
        if let Some(object) = result.as_object_mut() {
            object.remove("resultType");
        }
    }
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_name_their_workspace_and_caller() {
        let endpoint = Endpoint::parse(
            "http://127.0.0.1:4567/mcp/w/my-project-h7k2qa/s/python-1",
            "t",
        )
        .unwrap();
        assert_eq!(endpoint.port, 4567);
        assert_eq!(endpoint.workspace_id(), Some("my-project-h7k2qa"));
        assert_eq!(endpoint.caller_suffix(), Some("/s/python-1"));

        let moved = Endpoint::parse("http://127.0.0.1:9999/mcp/w/my-project-h7k2qa", "u")
            .unwrap()
            .with_caller(endpoint.caller_suffix());
        assert_eq!(moved.path, "/mcp/w/my-project-h7k2qa/s/python-1");

        assert!(Endpoint::parse("https://127.0.0.1:1/mcp/w/x", "t").is_none());
    }

    #[test]
    fn events_are_delivered_whole() {
        let mut delivered = Vec::new();
        let mut deliver = |message: Value| delivered.push(message);
        deliver_event("id: 0\nretry: 3000\ndata:\n\n", &mut deliver);
        deliver_event("event: message\ndata: {\"id\":1}\n\n", &mut deliver);
        assert_eq!(delivered, vec![json!({ "id": 1 })]);
    }

    #[test]
    fn offline_calls_explain_themselves() {
        let mut version = None;
        let call = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                           "params": { "name": "list_sessions" } });
        let reply = offline_reply(&call, Offline::NoWorkspace, &mut version).unwrap();
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(reply["result"]["structuredContent"]["code"], "NO_WORKSPACE");
        assert!(reply["result"].get("resultType").is_none());

        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/cancelled" });
        assert!(offline_reply(&notification, Offline::Unreachable, &mut version).is_none());
    }
}
