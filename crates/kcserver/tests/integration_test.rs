//
// integration_test.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

//! Integration tests for Kallichore server
#![allow(missing_docs)]

mod common;

use common::TestServer;
use kallichore_api::ServerStatusResponse;

#[tokio::test]
async fn test_server_starts_and_responds() {
    let server = TestServer::start().await;
    let client = server.create_client().await;
    
    let response = client.server_status().await.expect("Failed to get server status");
    
    match response {
        ServerStatusResponse::ServerStatusAndInformation(status) => {
            assert_eq!(status.version, "0.1.47");
            assert!(status.sessions >= 0);
        }
        ServerStatusResponse::Error(err) => {
            panic!("Server returned error: {:?}", err);
        }
    }
}
