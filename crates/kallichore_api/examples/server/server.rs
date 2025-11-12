//! Main library entry point for kallichore_api implementation.

#![allow(unused_imports)]

use async_trait::async_trait;
use futures::{future, Stream, StreamExt, TryFutureExt, TryStreamExt};
use hyper::server::conn::http1;
use hyper::service::{service_fn, Service};
use hyper_util::rt::TokioIo;
use log::info;
use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use swagger::auth::MakeAllowAllAuthenticator;
use swagger::EmptyContext;
use swagger::{Has, XSpanIdString};
use tokio::net::TcpListener;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios")))]
use openssl::ssl::{Ssl, SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};

use kallichore_api::models;

/// Builds an SSL implementation for Simple HTTPS from some hard-coded file names
pub async fn create(addr: &str, https: bool) {
    let addr: SocketAddr = addr.parse().expect("Failed to parse bind address");
    let listener = TcpListener::bind(&addr).await.unwrap();

    let server = Server::new();

    let service = MakeService::new(server);
    let service = MakeAllowAllAuthenticator::new(service, "cosmo");

    #[allow(unused_mut)]
    let mut service =
        kallichore_api::server::context::MakeAddContext::<_, EmptyContext>::new(service);

    if https {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "ios"))]
        {
            unimplemented!("SSL is not implemented for the examples on MacOS, Windows or iOS");
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios")))]
        {
            let mut ssl = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls())
                .expect("Failed to create SSL Acceptor");

            // Server authentication
            ssl.set_private_key_file("examples/server-key.pem", SslFiletype::PEM)
                .expect("Failed to set private key");
            ssl.set_certificate_chain_file("examples/server-chain.pem")
                .expect("Failed to set certificate chain");
            ssl.check_private_key()
                .expect("Failed to check private key");

            let tls_acceptor = ssl.build();

            info!("Starting a server (with https)");
            loop {
                if let Ok((tcp, addr)) = listener.accept().await {
                    let ssl = Ssl::new(tls_acceptor.context()).unwrap();
                    let service = service.call(addr);

                    tokio::spawn(async move {
                        let tls = tokio_openssl::SslStream::new(ssl, tcp).map_err(|_| ())?;
                        let service = service.await.map_err(|_| ())?;

                        http1::Builder::new()
                            .serve_connection(TokioIo::new(tls), service)
                            .await
                            .map_err(|_| ())
                    });
                }
            }
        }
    } else {
        info!("Starting a server (over http, so no TLS)");
        println!("Listening on http://{}", addr);

        loop {
            // When an incoming TCP connection is received grab a TCP stream for
            // client<->server communication.
            //
            // Note, this is a .await point, this loop will loop forever but is not a busy loop. The
            // .await point allows the Tokio runtime to pull the task off of the thread until the task
            // has work to do. In this case, a connection arrives on the port we are listening on and
            // the task is woken up, at which point the task is then put back on a thread, and is
            // driven forward by the runtime, eventually yielding a TCP stream.
            let (tcp_stream, addr) = listener
                .accept()
                .await
                .expect("Failed to accept connection");

            let service = service.call(addr).await.unwrap();
            let io = TokioIo::new(tcp_stream);
            // Spin up a new task in Tokio so we can continue to listen for new TCP connection on the
            // current task without waiting for the processing of the HTTP1 connection we just received
            // to finish
            tokio::task::spawn(async move {
                // Handle the connection from the client using HTTP1 and pass any
                // HTTP requests received on that connection to the `hello` function
                let result = http1::Builder::new().serve_connection(io, service).await;
                if let Err(err) = result {
                    println!("Error serving connection: {err:?}");
                }
            });
        }
    }
}

#[derive(Copy)]
pub struct Server<C> {
    marker: PhantomData<C>,
}

impl<C> Server<C> {
    pub fn new() -> Self {
        Server {
            marker: PhantomData,
        }
    }
}

impl<C> Clone for Server<C> {
    fn clone(&self) -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

use crate::server_auth;
use jsonwebtoken::{
    decode, encode, errors::Error as JwtError, Algorithm, DecodingKey, EncodingKey, Header,
    TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use swagger::auth::Authorization;

use kallichore_api::server::MakeService;
use kallichore_api::{
    AdoptSessionResponse, Api, ChannelsUpgradeResponse, ClientHeartbeatResponse,
    ConnectionInfoResponse, DeleteSessionResponse, GetServerConfigurationResponse,
    GetSessionResponse, InterruptSessionResponse, KillSessionResponse, ListSessionsResponse,
    NewSessionResponse, RestartSessionResponse, ServerStatusResponse,
    SetServerConfigurationResponse, ShutdownServerResponse, StartSessionResponse,
};
use std::error::Error;
use swagger::ApiError;

#[async_trait]
impl<C> Api<C> for Server<C>
where
    C: Has<XSpanIdString> + Send + Sync,
{
    /// Notify the server that a client is connected
    async fn client_heartbeat(
        &self,
        client_heartbeat: models::ClientHeartbeat,
        context: &C,
    ) -> Result<ClientHeartbeatResponse, ApiError> {
        info!(
            "client_heartbeat({:?}) - X-Span-ID: {:?}",
            client_heartbeat,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Get the server configuration
    async fn get_server_configuration(
        &self,
        context: &C,
    ) -> Result<GetServerConfigurationResponse, ApiError> {
        info!(
            "get_server_configuration() - X-Span-ID: {:?}",
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// List active sessions
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError> {
        info!("list_sessions() - X-Span-ID: {:?}", context.get().0.clone());
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Create a new session
    async fn new_session(
        &self,
        new_session: models::NewSession,
        context: &C,
    ) -> Result<NewSessionResponse, ApiError> {
        info!(
            "new_session({:?}) - X-Span-ID: {:?}",
            new_session,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Get server status and information
    async fn server_status(&self, context: &C) -> Result<ServerStatusResponse, ApiError> {
        info!("server_status() - X-Span-ID: {:?}", context.get().0.clone());
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Change the server configuration
    async fn set_server_configuration(
        &self,
        server_configuration: models::ServerConfiguration,
        context: &C,
    ) -> Result<SetServerConfigurationResponse, ApiError> {
        info!(
            "set_server_configuration({:?}) - X-Span-ID: {:?}",
            server_configuration,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Shut down all sessions and the server itself
    async fn shutdown_server(&self, context: &C) -> Result<ShutdownServerResponse, ApiError> {
        info!(
            "shutdown_server() - X-Span-ID: {:?}",
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Adopt an existing session
    async fn adopt_session(
        &self,
        session_id: String,
        connection_info: models::ConnectionInfo,
        context: &C,
    ) -> Result<AdoptSessionResponse, ApiError> {
        info!(
            "adopt_session(\"{}\", {:?}) - X-Span-ID: {:?}",
            session_id,
            connection_info,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Upgrade to a WebSocket or domain socket for channel communication
    async fn channels_upgrade(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<ChannelsUpgradeResponse, ApiError> {
        info!(
            "channels_upgrade(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Get Jupyter connection information for the session
    async fn connection_info(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<ConnectionInfoResponse, ApiError> {
        info!(
            "connection_info(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Delete session
    async fn delete_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<DeleteSessionResponse, ApiError> {
        info!(
            "delete_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Get session details
    async fn get_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<GetSessionResponse, ApiError> {
        info!(
            "get_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Interrupt session
    async fn interrupt_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<InterruptSessionResponse, ApiError> {
        info!(
            "interrupt_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Force quit session
    async fn kill_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<KillSessionResponse, ApiError> {
        info!(
            "kill_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Restart a session
    async fn restart_session(
        &self,
        session_id: String,
        restart_session: Option<models::RestartSession>,
        context: &C,
    ) -> Result<RestartSessionResponse, ApiError> {
        info!(
            "restart_session(\"{}\", {:?}) - X-Span-ID: {:?}",
            session_id,
            restart_session,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }

    /// Start a session
    async fn start_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<StartSessionResponse, ApiError> {
        info!(
            "start_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Api-Error: Operation is NOT implemented".into()))
    }
}
