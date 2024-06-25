//! kcserver
//!
//! Kallichore server

use std::{borrow::BorrowMut, convert::Infallible};
use std::net::SocketAddr;
use hyper::{service::make_service_fn, Body, Request, Response, Server};
use openapi_client::server::{MakeService, Service};

mod api;

async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new("Hello, World".into()))
}

#[tokio::main]
async fn main() {
    // We'll bind to 127.0.0.1:3000
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    use crate::api::KallichoreApi;
    let api = KallichoreApi;
    //let api_impl = Service::new(KallichoreApi);
    let make_service = MakeService::new(api);

    let server = Server::bind(&addr).serve(make_service);

    // Run this server for... forever!
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
