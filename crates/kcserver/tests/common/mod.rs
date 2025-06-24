//
// mod.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

use kallichore_api::{ApiNoContext, Client, ContextWrapperExt};
use kcshared::port_picker::pick_unused_tcp_port;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use swagger::{AuthData, ContextBuilder, EmptyContext, Push, XSpanIdString};
use tokio::time::timeout;

type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

#[allow(dead_code)]
pub struct TestServer {
    child: Child,
    base_url: String,
    port: u16,
}

impl TestServer {
    pub async fn start() -> Self {
        let port = pick_unused_tcp_port().expect("Failed to pick unused port");

        // Try to use pre-built binary first, fall back to cargo run
        let binary_path = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target/debug/kcserver")
            .with_extension(if cfg!(windows) { "exe" } else { "" });

        let mut cmd = if binary_path.exists() {
            println!("Using pre-built binary at: {:?}", binary_path);
            let mut c = Command::new(&binary_path);
            c.args(&[
                "--port",
                &port.to_string(),
                "--token",
                "none", // Disable auth for testing
            ]);
            c
        } else {
            println!("Pre-built binary not found, using cargo run");
            let mut c = Command::new("cargo");
            c.args(&[
                "run",
                "--bin",
                "kcserver",
                "--",
                "--port",
                &port.to_string(),
                "--token",
                "none", // Disable auth for testing
            ]);
            c
        };

        // Capture output for debugging if needed
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Reduce log level for faster startup but still capture errors
        cmd.env("RUST_LOG", "warn");

        let child = cmd.spawn().expect("Failed to start kcserver");

        let base_url = format!("http://localhost:{}", port);

        let test_server = TestServer {
            child,
            base_url,
            port,
        };

        test_server.wait_for_ready().await;
        test_server
    }
    async fn wait_for_ready(&self) {
        let client = self.create_client().await;

        // Increased timeout for Windows and debug builds
        for attempt in 0..60 {
            match timeout(Duration::from_millis(500), client.server_status()).await {
                Ok(Ok(_)) => {
                    println!("Server ready after {} attempts", attempt + 1);
                    return;
                }
                Ok(Err(e)) => {
                    if attempt > 45 {
                        println!("Server status error on attempt {}: {:?}", attempt, e);
                    }
                }
                Err(_) => {
                    if attempt > 45 {
                        println!("Server status timeout on attempt {}", attempt);
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        panic!("Server failed to start within timeout");
    }

    pub async fn create_client(&self) -> Box<dyn ApiNoContext<ClientContext> + Send + Sync> {
        #[allow(trivial_casts)]
        let context: ClientContext = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            None as Option<AuthData>,
            XSpanIdString::default()
        );

        let client = Client::try_new_http(&self.base_url).expect("Failed to create HTTP client");

        Box::new(client.with_context(context))
    }

    #[allow(dead_code)]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
