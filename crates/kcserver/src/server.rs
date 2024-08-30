//
// server.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Main library entry point for kallichore_api implementation.

#![allow(unused_imports)]

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{future, SinkExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use hyper::header::{HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE};
use hyper::server::conn::Http;
use hyper::service::Service;
use hyper::upgrade::Upgraded;
use hyper::{Body, Response, StatusCode};
use hyper_util::rt::TokioIo;
use log::info;
use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::{any, env};
use swagger::auth::MakeAllowAllAuthenticator;
use swagger::EmptyContext;
use swagger::{Has, XSpanIdString};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use kallichore_api::{models, ChannelsWebsocketResponse, NewSessionResponse};

pub async fn create(addr: &str) {
    let addr = addr.parse().expect("Failed to parse bind address");

    let server = Server::new();

    let service = MakeService::new(server);

    let service = MakeAllowAllAuthenticator::new(service, "cosmo");

    #[allow(unused_mut)]
    let mut service =
        kallichore_api::server::context::MakeAddContext::<_, EmptyContext>::new(service);

    // Using HTTP
    hyper::server::Server::bind(&addr)
        .serve(service)
        .await
        .unwrap()
}

#[derive(Clone)]
pub struct Server<C> {
    marker: PhantomData<C>,
    kernel_sessions: Arc<RwLock<Vec<KernelSession>>>,
    client_sessions: Arc<RwLock<Vec<ClientSession>>>,
}

impl<C> Server<C> {
    pub fn new() -> Self {
        Server {
            marker: PhantomData,
            kernel_sessions: Arc::new(RwLock::new(vec![])),
            client_sessions: Arc::new(RwLock::new(vec![])),
        }
    }
}

use kallichore_api::server::MakeService;
use kallichore_api::{Api, ListSessionsResponse};
use std::error::Error;
use swagger::ApiError;

use crate::client_session::ClientSession;
use crate::connection_file::{self, ConnectionFile};
use crate::error::KSError;
use crate::kernel_session::{self, KernelSession};
use crate::zmq_ws_proxy;

#[async_trait]
impl<C> Api<C> for Server<C>
where
    C: Has<XSpanIdString> + Send + Sync,
{
    /// List active sessions
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError> {
        info!("list_sessions() - X-Span-ID: {:?}", context.get().0.clone());

        // Make a copy of the active session list to avoid holding the lock
        let sessions = {
            let sessions = self.kernel_sessions.read().unwrap();
            sessions.clone()
        };

        // Create a list of session metadata
        let mut result: Vec<models::SessionListSessionsInner> = Vec::new();
        for s in sessions.iter() {
            let state = {
                let state = s.state.read().await;
                state.clone()
            };
            result.push(models::SessionListSessionsInner {
                session_id: s.connection.session_id.clone(),
                username: s.connection.username.clone(),
                argv: s.argv.clone(),
                process_id: match s.process_id {
                    Some(pid) => Some(pid as i32),
                    None => None,
                },
                connected: state.connected,
                working_directory: state.working_directory.clone(),
                started: s.started.clone(),
                status: state.status,
            });
        }
        let session_list = models::SessionList {
            total: result.len() as i32,
            sessions: result.clone(),
        };
        Ok(ListSessionsResponse::ListOfActiveSessions(session_list))
    }
    /// Create a new session

    async fn new_session(
        &self,
        session: models::Session,
        context: &C,
    ) -> Result<NewSessionResponse, ApiError> {
        info!(
            "new_session({:?}) - X-Span-ID: {:?}",
            session,
            context.get().0.clone()
        );

        {
            // Check to see if the session already exists, dropping the read
            // lock afterwards.
            let sessions = self.kernel_sessions.read().unwrap();
            for s in sessions.iter() {
                if s.connection.session_id == session.session_id {
                    let error = KSError::SessionExists(session.session_id.clone());
                    error.log();
                    return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
                }
            }
        }

        let new_session_id = session.session_id.clone();
        let session_id = models::NewSession200Response {
            session_id: new_session_id.clone(),
        };
        let args = session.argv.clone();

        // Create a connection file for the session in a temporary directory
        // TODO: Handle error
        let connection_file = match ConnectionFile::generate(String::from("127.0.0.1")) {
            Ok(connection_file) => connection_file,
            Err(e) => {
                let error = KSError::SessionStartFailed(e);
                error.log();
                return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
            }
        };

        let temp_dir = env::temp_dir();
        let mut connection_file_name = std::ffi::OsString::from("connection_");
        connection_file_name.push(new_session_id.clone());
        connection_file_name.push(".json");

        // Combine the temporary directory with the file name to get the full path
        let connection_path: PathBuf = temp_dir.join(connection_file_name);
        if let Err(err) = connection_file.to_file(connection_path.clone()) {
            let error = KSError::SessionStartFailed(anyhow!(
                "Failed to write connection file {}: {}",
                connection_path.to_string_lossy(),
                err
            ));
            error.log();
            return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
        }

        log::trace!(
            "Created connection file for session {} at {:?}",
            new_session_id.clone(),
            connection_path
        );

        let mut log_file_name = std::ffi::OsString::from("kernel_log_");
        log_file_name.push(new_session_id.clone());
        log_file_name.push(".txt");
        let log_path: PathBuf = temp_dir.join(log_file_name);

        log::trace!(
            "Created log file for session {} at {:?}",
            new_session_id.clone(),
            log_path
        );

        // Loop through the arguments; if any is the special string "{connection_file}", replace it with the session id
        let args: Vec<String> = args
            .iter()
            .map(|arg| {
                if arg == "{connection_file}" {
                    connection_path.to_string_lossy().to_string()
                } else if arg == "{log_file}" {
                    log_path.to_string_lossy().to_string()
                } else {
                    arg.clone()
                }
            })
            .collect();

        let session = models::Session {
            session_id: session_id.session_id.clone(),
            argv: args,
            working_directory: session.working_directory.clone(),
            username: session.username.clone(),
            env: session.env.clone(),
        };

        let sessions = self.kernel_sessions.clone();
        let kernel_session = match KernelSession::new(session, connection_file.clone()) {
            Ok(kernel_session) => kernel_session,
            Err(e) => {
                let error = KSError::SessionStartFailed(e);
                return Ok(NewSessionResponse::InvalidRequest(error.to_json(None)));
            }
        };

        let mut sessions = sessions.write().unwrap();
        let connection = kernel_session.connection.clone();
        let ws_json_tx = kernel_session.ws_json_tx.clone();
        let ws_zmq_rx = kernel_session.ws_zmq_rx.clone();
        sessions.push(kernel_session);

        tokio::spawn(async move {
            match zmq_ws_proxy::zmq_ws_proxy(connection, connection_file, ws_json_tx, ws_zmq_rx)
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    let error = KSError::SessionStartFailed(e);
                    error.log();
                }
            }
        });
        Ok(NewSessionResponse::TheSessionID(session_id))
    }

    async fn channels_websocket(
        &self,
        session_id: String,
        _context: &C,
    ) -> Result<ChannelsWebsocketResponse, ApiError> {
        info!("upgrade to websocket: {}", session_id);
        Ok(ChannelsWebsocketResponse::UpgradeConnectionToAWebsocket)
    }

    async fn channels_websocket_request(
        &self,
        mut request: hyper::Request<Body>,
        session_id: String,
        _context: &C,
    ) -> Result<Response<Body>, ApiError> {
        log::debug!(
            "Upgrading channel connection to websocket for session '{}'",
            session_id
        );
        let derived = {
            let headers = request.headers();
            let key = headers.get(SEC_WEBSOCKET_KEY);
            key.map(|k| derive_accept_key(k.as_bytes()))
        };
        let version = request.version();
        let kernel_sessions = self.kernel_sessions.clone();
        {
            // Validate the session ID before upgrading the connection
            let kernel_sessions = kernel_sessions.read().unwrap();
            if kernel_sessions
                .iter()
                .find(|s| s.connection.session_id == session_id)
                .is_none()
            {
                let err = KSError::SessionNotFound(session_id.clone());
                err.log();
                let err = err.to_json(Some(
                    "Establishing a websocket connection requires a valid session ID".to_string(),
                ));
                // Serialize the error to JSON
                let err = serde_json::to_string(&err).unwrap();
                let mut response = Response::new(err.into());
                *response.status_mut() = StatusCode::BAD_REQUEST;
                return Ok(response);
            }
        };

        let client_sessions: Arc<RwLock<Vec<ClientSession>>> = self.client_sessions.clone();
        {
            // Check if the session already exists
            let client_sessions = client_sessions.read().unwrap();
            if client_sessions
                .iter()
                .find(|s| s.connection.session_id == session_id)
                .is_some()
            {
                let err = KSError::SessionConnected(session_id.clone());
                err.log();

                // Serialize the error to JSON
                let err = err.to_json(None);
                let err = serde_json::to_string(&err).unwrap();
                let mut response = Response::new(err.into());
                *response.status_mut() = StatusCode::BAD_REQUEST;
                return Ok(response);
            }
        }

        tokio::task::spawn(async move {
            match hyper::upgrade::on(&mut request).await {
                Ok(upgraded) => {
                    log::debug!("Creating session for websocket connection");

                    // Find the kernel session with the given ID
                    let client_session = {
                        let sessions = kernel_sessions.read().unwrap();
                        let session = sessions
                            .iter()
                            .find(|s| s.connection.session_id == session_id)
                            .expect("Session not found");

                        let ws_zmq_tx = session.ws_zmq_tx.clone();
                        let ws_json_rx = session.ws_json_rx.clone();
                        let connection = session.connection.clone();
                        ClientSession::new(connection, ws_json_rx, ws_zmq_tx)
                    };

                    log::debug!(
                        "Connection upgraded to websocket for session '{}'",
                        session_id
                    );

                    let stream =
                        WebSocketStream::from_raw_socket(upgraded, Role::Server, None).await;
                    client_session.handle_channel_ws(stream).await;
                }
                Err(e) => {
                    log::error!("Failed to upgrade channel connection to websocket: {}", e);
                }
            }
        });
        let upgrade = HeaderValue::from_static("Upgrade");
        let websocket = HeaderValue::from_static("websocket");
        let mut response = Response::new(Body::default());
        *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
        *response.version_mut() = version;
        response.headers_mut().append(CONNECTION, upgrade);
        response.headers_mut().append(UPGRADE, websocket);
        response
            .headers_mut()
            .append(SEC_WEBSOCKET_ACCEPT, derived.unwrap().parse().unwrap());
        Ok(response)
    }
}
