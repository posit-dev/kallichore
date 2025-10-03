//
// basic_server_tests.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Basic server functionality tests

#[path = "common/mod.rs"]
mod common;

use common::TestServer;
use kallichore_api::ServerStatusResponse;

#[tokio::test]
async fn test_server_starts_and_responds() {
    let server = TestServer::start().await;
    let client = server.create_client().await;

    let response = client
        .server_status()
        .await
        .expect("Failed to get server status");

    match response {
        ServerStatusResponse::ServerStatusAndInformation(status) => {
            assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
            assert!(status.sessions >= 0);
        }
        ServerStatusResponse::Error(err) => {
            panic!("Server returned error: {:?}", err);
        }
    }

    // Explicitly drop the server to ensure cleanup
    drop(server);
}
