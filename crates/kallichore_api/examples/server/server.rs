//! Main library entry point for kallichore_api implementation.

#![allow(unused_imports)]

use async_trait::async_trait;
use futures::{future, Stream, StreamExt, TryFutureExt, TryStreamExt};
use hyper::server::conn::Http;
use hyper::service::Service;
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
    let addr = addr.parse().expect("Failed to parse bind address");

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
            let tcp_listener = TcpListener::bind(&addr).await.unwrap();

            loop {
                if let Ok((tcp, _)) = tcp_listener.accept().await {
                    let ssl = Ssl::new(tls_acceptor.context()).unwrap();
                    let addr = tcp.peer_addr().expect("Unable to get remote address");
                    let service = service.call(addr);

                    tokio::spawn(async move {
                        let tls = tokio_openssl::SslStream::new(ssl, tcp).map_err(|_| ())?;
                        let service = service.await.map_err(|_| ())?;

                        Http::new()
                            .serve_connection(tls, service)
                            .await
                            .map_err(|_| ())
                    });
                }
            }
        }
    } else {
        // Using HTTP
        hyper::server::Server::bind(&addr)
            .serve(service)
            .await
            .unwrap()
    }
}

#[derive(Copy, Clone)]
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

use kallichore_api::server::MakeService;
use kallichore_api::{
    AdoptSessionResponse, Api, ChannelsWebsocketResponse, DeleteSessionResponse,
    GetSessionResponse, InterruptSessionResponse, KillSessionResponse, ListSessionsResponse,
    NewSessionResponse, RestartSessionResponse, ServerStatusResponse, ShutdownServerResponse,
    StartSessionResponse,
};
use std::error::Error;
use swagger::ApiError;

#[async_trait]
impl<C> Api<C> for Server<C>
where
    C: Has<XSpanIdString> + Send + Sync,
{
    /// Adopt an existing session
    async fn adopt_session(
        &self,
        adopted_session: models::AdoptedSession,
        context: &C,
    ) -> Result<AdoptSessionResponse, ApiError> {
        info!(
            "adopt_session({:?}) - X-Span-ID: {:?}",
            adopted_session,
            context.get().0.clone()
        );
        Err(ApiError("Generic failure".into()))
    }

    /// Upgrade to a WebSocket for channel communication
    async fn channels_websocket(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<ChannelsWebsocketResponse, ApiError> {
        info!(
            "channels_websocket(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Generic failure".into()))
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
        Err(ApiError("Generic failure".into()))
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
        Err(ApiError("Generic failure".into()))
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
        Err(ApiError("Generic failure".into()))
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
        Err(ApiError("Generic failure".into()))
    }

    /// List active sessions
    async fn list_sessions(&self, context: &C) -> Result<ListSessionsResponse, ApiError> {
        info!("list_sessions() - X-Span-ID: {:?}", context.get().0.clone());
        Err(ApiError("Generic failure".into()))
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
        Err(ApiError("Generic failure".into()))
    }

    // --- Start Kallichore ---
    async fn channels_websocket_request(
        &self,
        request: hyper::Request<hyper::Body>,
        session_id: String,
        context: &C,
    ) -> Result<hyper::Response<hyper::Body>, ApiError> {
        info!(
            "channels_websocket_request({:?}, \"{}\") - X-Span-ID: {:?}",
            request,
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Generic failure".into()))
    }
    // --- End Kallichore ---
    /// Restart a session
    async fn restart_session(
        &self,
        session_id: String,
        context: &C,
    ) -> Result<RestartSessionResponse, ApiError> {
        info!(
            "restart_session(\"{}\") - X-Span-ID: {:?}",
            session_id,
            context.get().0.clone()
        );
        Err(ApiError("Generic failure".into()))
    }

    /// Get server status and information
    async fn server_status(&self, context: &C) -> Result<ServerStatusResponse, ApiError> {
        info!("server_status() - X-Span-ID: {:?}", context.get().0.clone());
        Err(ApiError("Generic failure".into()))
    }

    /// Shut down all sessions and the server itself
    async fn shutdown_server(&self, context: &C) -> Result<ShutdownServerResponse, ApiError> {
        info!(
            "shutdown_server() - X-Span-ID: {:?}",
            context.get().0.clone()
        );
        Err(ApiError("Generic failure".into()))
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
        Err(ApiError("Generic failure".into()))
    }
}
