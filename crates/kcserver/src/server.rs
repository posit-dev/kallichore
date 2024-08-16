//
// server.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Main library entry point for kallichore_api implementation.

#![allow(unused_imports)]

use async_trait::async_trait;
use futures::{future, Stream, StreamExt, TryFutureExt, TryStreamExt};
use hyper::server::conn::Http;
use hyper::service::Service;
use log::info;
use std::env;
use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use swagger::auth::MakeAllowAllAuthenticator;
use swagger::EmptyContext;
use swagger::{Has, XSpanIdString};
use tokio::net::TcpListener;

use kallichore_api::{models, NewSessionResponse};

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
    sessions: Arc<RwLock<Vec<KernelSession>>>,
}

impl<C> Server<C> {
    pub fn new() -> Self {
        Server {
            marker: PhantomData,
            sessions: Arc::new(RwLock::new(vec![])),
        }
    }
}

use kallichore_api::server::MakeService;
use kallichore_api::{Api, ListSessionsResponse};
use std::error::Error;
use swagger::ApiError;

use crate::connection_file::{self, ConnectionFile};
use crate::session::KernelSession;

#[async_trait]
impl<C> Api<C> for Server<C>
where
    C: Has<XSpanIdString> + Send + Sync,
{
    /// List active sessions
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError> {
        info!("list_sessions() - X-Span-ID: {:?}", context.get().0.clone());

        let sessions = self.sessions.read().unwrap();
        // Convert the vector of sessions to a vector of SessionsListSessionsInner
        let sessions: Vec<models::SessionListSessionsInner> = sessions
            .iter()
            .map(|s| models::SessionListSessionsInner {
                session_id: s.session_id.clone(),
                argv: s.argv.clone(),
                process_id: match s.process_id {
                    Some(pid) => Some(pid as i32),
                    None => None,
                },
                status: s.status.clone(),
            })
            .collect();
        let session_list = models::SessionList {
            total: sessions.len() as i32,
            sessions: sessions.clone(),
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
            let sessions = self.sessions.read().unwrap();
            for s in sessions.iter() {
                if s.session_id == session.session_id {
                    let err = models::Error {
                        code: String::from("KS-001"),
                        message: format!("Session {} already exists", session.session_id),
                        details: None,
                    };
                    return Ok(NewSessionResponse::InvalidRequest(err));
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
        let connection_file = ConnectionFile::generate(String::from("localhost")).unwrap();

        let temp_dir = env::temp_dir();
        let mut file_name = std::ffi::OsString::from("connection_");
        file_name.push(new_session_id.clone());
        file_name.push(".json");

        // Combine the temporary directory with the file name to get the full path
        let connection_path: PathBuf = temp_dir.join(file_name);
        connection_file.to_file(connection_path.clone()).unwrap();

        log::trace!(
            "Created connection file for session {} at {:?}",
            new_session_id.clone(),
            connection_path
        );

        // Loop through the arguments; if any is the special string "{connection_file}", replace it with the session id
        let args: Vec<String> = args
            .iter()
            .map(|arg| {
                if arg == "{connection_file}" {
                    connection_path.to_string_lossy().to_string()
                } else {
                    arg.clone()
                }
            })
            .collect();

        let session = models::Session {
            session_id: session_id.session_id.clone(),
            argv: args,
            working_directory: session.working_directory.clone(),
        };

        let kernel_session = KernelSession::new(session);
        let mut sessions = self.sessions.write().unwrap();
        sessions.push(kernel_session);
        Ok(NewSessionResponse::TheSessionID(session_id))
    }
}
