//
// execute_code_test.rs
//
// Copyright (C) 2025-2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Integration tests for the execute_code RPC endpoint
//!
//! These tests verify that code can be executed via the HTTP RPC without
//! needing a WebSocket connection.

#![allow(unused_imports)]

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    create_session_with_client, create_test_session, get_python_executable, is_ipykernel_available,
};
use common::TestServer;
use kallichore_api::models::{ExecuteReplyStatus, ExecuteRequest, Status};
use kallichore_api::{ExecuteCodeResponse, GetSessionResponse, StartSessionResponse};
use std::time::Duration;
use uuid::Uuid;

/// Helper: create a session, start it, and return the session_id
async fn setup_kernel_session(
    server: &TestServer,
    python_cmd: &str,
) -> (
    Box<dyn kallichore_api::ApiNoContext<common::test_utils::ClientContext> + Send + Sync>,
    String,
) {
    let client = server.create_client().await;
    let session_id = format!("test-exec-{}", Uuid::new_v4());
    let new_session = create_test_session(session_id.clone(), python_cmd);

    create_session_with_client(&client, new_session).await;

    let start_response = client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");

    match start_response {
        StartSessionResponse::Started(_) => {
            println!("Kernel started for session {}", session_id);
        }
        other => panic!("Unexpected start response: {:?}", other),
    }

    // Wait for the kernel to become idle (ready to execute)
    for _ in 0..60 {
        let session_response = client
            .get_session(session_id.clone())
            .await
            .expect("Failed to get session");
        if let GetSessionResponse::SessionDetails(details) = session_response {
            if details.status == Status::Idle {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    (client, session_id)
}

#[tokio::test]
async fn test_execute_code_basic() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = match get_python_executable().await {
            Some(cmd) => cmd,
            None => {
                println!("Skipping: No Python executable found");
                return;
            }
        };
        if !is_ipykernel_available().await {
            println!("Skipping: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let (client, session_id) = setup_kernel_session(&server, &python_cmd).await;

        // Execute simple print statement
        let request = ExecuteRequest::new("print('hello world')".to_string());
        let response = client
            .execute_code(session_id.clone(), request)
            .await
            .expect("Failed to call execute_code");

        match response {
            ExecuteCodeResponse::ExecutionCompleted(reply) => {
                println!("Execute reply: {:?}", reply);
                assert_eq!(reply.status, ExecuteReplyStatus::Ok);
                assert!(reply.execution_count >= 1);

                // Check that stdout was captured
                let stdout: String = reply
                    .output
                    .iter()
                    .filter(|o| o.stream_name.as_deref() == Some("stdout"))
                    .filter_map(|o| o.text.as_deref())
                    .collect();
                assert!(
                    stdout.contains("hello world"),
                    "Expected 'hello world' in stdout, got: {:?}",
                    stdout
                );
            }
            other => panic!("Expected ExecutionCompleted, got: {:?}", other),
        }
    })
    .await;

    assert!(test_result.is_ok(), "Test timed out");
}

#[tokio::test]
async fn test_execute_code_with_result() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = match get_python_executable().await {
            Some(cmd) => cmd,
            None => {
                println!("Skipping: No Python executable found");
                return;
            }
        };
        if !is_ipykernel_available().await {
            println!("Skipping: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let (client, session_id) = setup_kernel_session(&server, &python_cmd).await;

        // Execute an expression that produces a result
        let request = ExecuteRequest::new("2 + 3".to_string());
        let response = client
            .execute_code(session_id.clone(), request)
            .await
            .expect("Failed to call execute_code");

        match response {
            ExecuteCodeResponse::ExecutionCompleted(reply) => {
                println!("Execute reply: {:?}", reply);
                assert_eq!(reply.status, ExecuteReplyStatus::Ok);

                // The result should be hoisted into the top-level data field
                assert!(reply.data.is_some(), "Expected data field to be set");
                let data = reply.data.unwrap();
                let text_plain = data.get("text/plain").expect("Expected text/plain in data");
                assert_eq!(text_plain, "5");
            }
            other => panic!("Expected ExecutionCompleted, got: {:?}", other),
        }
    })
    .await;

    assert!(test_result.is_ok(), "Test timed out");
}

#[tokio::test]
async fn test_execute_code_error() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = match get_python_executable().await {
            Some(cmd) => cmd,
            None => {
                println!("Skipping: No Python executable found");
                return;
            }
        };
        if !is_ipykernel_available().await {
            println!("Skipping: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let (client, session_id) = setup_kernel_session(&server, &python_cmd).await;

        // Execute code that raises an error
        let request = ExecuteRequest::new("raise ValueError('test error')".to_string());
        let response = client
            .execute_code(session_id.clone(), request)
            .await
            .expect("Failed to call execute_code");

        match response {
            ExecuteCodeResponse::ExecutionCompleted(reply) => {
                println!("Execute reply: {:?}", reply);
                assert_eq!(reply.status, ExecuteReplyStatus::Error);
                assert_eq!(reply.error_name.as_deref(), Some("ValueError"));
                assert_eq!(reply.error_message.as_deref(), Some("test error"));
                assert!(reply.error_traceback.is_some());

                // The error should also appear in the output array
                let error_outputs: Vec<_> = reply
                    .output
                    .iter()
                    .filter(|o| {
                        o.r#type == kallichore_api::models::ExecuteOutputType::Error
                    })
                    .collect();
                assert!(
                    !error_outputs.is_empty(),
                    "Expected error output in the output array"
                );
            }
            other => panic!("Expected ExecutionCompleted, got: {:?}", other),
        }
    })
    .await;

    assert!(test_result.is_ok(), "Test timed out");
}

#[tokio::test]
async fn test_execute_code_sequential() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = match get_python_executable().await {
            Some(cmd) => cmd,
            None => {
                println!("Skipping: No Python executable found");
                return;
            }
        };
        if !is_ipykernel_available().await {
            println!("Skipping: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let (client, session_id) = setup_kernel_session(&server, &python_cmd).await;

        // First execution: set a variable
        let request = ExecuteRequest::new("x = 42".to_string());
        let response = client
            .execute_code(session_id.clone(), request)
            .await
            .expect("Failed to call execute_code");
        match &response {
            ExecuteCodeResponse::ExecutionCompleted(reply) => {
                assert_eq!(reply.status, ExecuteReplyStatus::Ok);
            }
            other => panic!("Expected ExecutionCompleted, got: {:?}", other),
        }

        // Second execution: use the variable
        let request = ExecuteRequest::new("print(x * 2)".to_string());
        let response = client
            .execute_code(session_id.clone(), request)
            .await
            .expect("Failed to call execute_code");
        match response {
            ExecuteCodeResponse::ExecutionCompleted(reply) => {
                assert_eq!(reply.status, ExecuteReplyStatus::Ok);

                let stdout: String = reply
                    .output
                    .iter()
                    .filter(|o| o.stream_name.as_deref() == Some("stdout"))
                    .filter_map(|o| o.text.as_deref())
                    .collect();
                assert!(
                    stdout.contains("84"),
                    "Expected '84' in stdout, got: {:?}",
                    stdout
                );
            }
            other => panic!("Expected ExecutionCompleted, got: {:?}", other),
        }
    })
    .await;

    assert!(test_result.is_ok(), "Test timed out");
}

#[tokio::test]
async fn test_execute_code_timeout() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = match get_python_executable().await {
            Some(cmd) => cmd,
            None => {
                println!("Skipping: No Python executable found");
                return;
            }
        };
        if !is_ipykernel_available().await {
            println!("Skipping: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let (client, session_id) = setup_kernel_session(&server, &python_cmd).await;

        // Execute code that takes longer than the timeout
        let mut request =
            ExecuteRequest::new("import time; time.sleep(10)".to_string());
        request.timeout_seconds = Some(1);
        let response = client
            .execute_code(session_id.clone(), request)
            .await
            .expect("Failed to call execute_code");

        match response {
            ExecuteCodeResponse::ExecutionTimedOut(err) => {
                println!("Got expected timeout: {:?}", err);
                assert_eq!(err.code, "timeout");
            }
            other => panic!("Expected ExecutionTimedOut, got: {:?}", other),
        }
    })
    .await;

    assert!(test_result.is_ok(), "Test timed out");
}

#[tokio::test]
async fn test_execute_code_session_not_found() {
    let test_result = tokio::time::timeout(Duration::from_secs(15), async {
        let server = TestServer::start().await;
        let client = server.create_client().await;

        let request = ExecuteRequest::new("print('hello')".to_string());
        let response = client
            .execute_code("nonexistent-session".to_string(), request)
            .await
            .expect("Failed to call execute_code");

        match response {
            ExecuteCodeResponse::SessionNotFound => {
                println!("Got expected SessionNotFound");
            }
            other => panic!("Expected SessionNotFound, got: {:?}", other),
        }
    })
    .await;

    assert!(test_result.is_ok(), "Test timed out");
}

#[tokio::test]
async fn test_execute_code_silent() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = match get_python_executable().await {
            Some(cmd) => cmd,
            None => {
                println!("Skipping: No Python executable found");
                return;
            }
        };
        if !is_ipykernel_available().await {
            println!("Skipping: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let (client, session_id) = setup_kernel_session(&server, &python_cmd).await;

        // Execute silently - should not produce an execute_result
        let mut request = ExecuteRequest::new("2 + 3".to_string());
        request.silent = Some(true);
        let response = client
            .execute_code(session_id.clone(), request)
            .await
            .expect("Failed to call execute_code");

        match response {
            ExecuteCodeResponse::ExecutionCompleted(reply) => {
                println!("Silent execute reply: {:?}", reply);
                assert_eq!(reply.status, ExecuteReplyStatus::Ok);
                // In silent mode, execution_count should not increment
                // and there should be no execute_result data
                assert!(
                    reply.data.is_none(),
                    "Silent execution should not produce result data"
                );
            }
            other => panic!("Expected ExecutionCompleted, got: {:?}", other),
        }
    })
    .await;

    assert!(test_result.is_ok(), "Test timed out");
}
