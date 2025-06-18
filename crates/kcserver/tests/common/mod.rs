//
// mod.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

use std::process::{Child, Command, Stdio};
use std::time::Duration;
use kallichore_api::{ApiNoContext, Client, ContextWrapperExt};
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
        let port = portpicker::pick_unused_port().expect("Failed to pick unused port");
        
        let mut cmd = Command::new("cargo");
        cmd.args(&[
            "run", 
            "--bin", 
            "kcserver", 
            "--",
            "--port", 
            &port.to_string(),
            "--token",
            "none" // Disable auth for testing
        ]);
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        
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
        
        for _attempt in 0..50 {
            match timeout(Duration::from_millis(500), client.server_status()).await {
                Ok(Ok(_)) => return,
                _ => tokio::time::sleep(Duration::from_millis(200)).await,
            }
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
        
        let client = Client::try_new_http(&self.base_url)
            .expect("Failed to create HTTP client");
        
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
