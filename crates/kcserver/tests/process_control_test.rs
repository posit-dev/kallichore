//
// process_control_test.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Tests for signalling kernel processes: killing a session and interrupting
//! one with SIGINT.

#![allow(unused_imports)]

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    create_session_with_client, create_test_session, get_python_executable,
    is_ipykernel_available,
};
use common::TestServer;
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use kcshared::websocket_message::WebsocketMessage;
use std::time::Duration;
use uuid::Uuid;

/// Whether a process is still running, used to check that a kill took effect.
#[cfg(unix)]
fn process_is_running(pid: i32) -> bool {
    // Signal 0 performs kill()'s existence check without sending anything.
    #[allow(unsafe_code)]
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    alive
}

/// Killing a running session must terminate the kernel process, and killing a
/// session whose process is already gone must report failure rather than
/// silently succeeding.
#[cfg(unix)]
#[tokio::test]
async fn test_kill_session_terminates_process() {
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        let Some(python_cmd) = get_python_executable().await else {
            println!("Skipping test: no Python executable found");
            return;
        };
        if !is_ipykernel_available().await {
            println!("Skipping test: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let client = server.create_client().await;

        let session_id = format!("kill-test-{}", Uuid::new_v4());
        create_session_with_client(&client, create_test_session(session_id.clone(), &python_cmd))
            .await;

        match client.start_session(session_id.clone()).await {
            Ok(kallichore_api::StartSessionResponse::Started(_)) => {}
            other => {
                println!("Skipping test: kernel failed to start: {:?}", other);
                return;
            }
        }

        // Find the PID the supervisor is tracking for this session.
        let kallichore_api::ListSessionsResponse::ListOfActiveSessions(list) =
            client.list_sessions().await.expect("failed to list sessions");
        let pid = list
            .sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .and_then(|s| s.process_id)
            .expect("session should report a process ID");
        assert!(process_is_running(pid), "kernel {} should be running", pid);

        let response = client
            .kill_session(session_id.clone())
            .await
            .expect("kill request failed");
        assert!(
            matches!(response, kallichore_api::KillSessionResponse::Killed(_)),
            "expected the session to be killed, got {:?}",
            response
        );

        // The signal is asynchronous, so give the process a moment to die.
        for _ in 0..50 {
            if !process_is_running(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            !process_is_running(pid),
            "kernel {} should have been killed",
            pid
        );

        // A second kill has no process left to signal and must say so.
        let response = client
            .kill_session(session_id.clone())
            .await
            .expect("second kill request failed");
        assert!(
            matches!(response, kallichore_api::KillSessionResponse::KillFailed(_)),
            "killing an already-dead session should fail, got {:?}",
            response
        );

        drop(server);
    })
    .await;

    result.expect("test timed out");
}

/// A session configured for signal-based interrupts must actually receive
/// SIGINT, which Python surfaces as a KeyboardInterrupt.
#[cfg(unix)]
#[tokio::test]
async fn test_signal_interrupt_reaches_kernel() {
    use common::transport::CommunicationChannel;
    use kallichore_api::models::InterruptMode;

    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let Some(python_cmd) = get_python_executable().await else {
            println!("Skipping test: no Python executable found");
            return;
        };
        if !is_ipykernel_available().await {
            println!("Skipping test: ipykernel not available");
            return;
        }

        let server = TestServer::start().await;
        let client = server.create_client().await;

        let session_id = format!("interrupt-test-{}", Uuid::new_v4());
        let mut new_session = create_test_session(session_id.clone(), &python_cmd);
        // The default test session interrupts over the message channel; this
        // test is specifically about the SIGINT path.
        new_session.interrupt_mode = InterruptMode::Signal;
        create_session_with_client(&client, new_session).await;

        match client.start_session(session_id.clone()).await {
            Ok(kallichore_api::StartSessionResponse::Started(_)) => {}
            other => {
                println!("Skipping test: kernel failed to start: {:?}", other);
                return;
            }
        }

        let ws_url = format!(
            "ws://localhost:{}/sessions/{}/channels",
            server.port(),
            session_id
        );
        let mut comm = CommunicationChannel::create_websocket(&ws_url)
            .await
            .expect("failed to create websocket");

        // Put the kernel into a long sleep so there is something to interrupt.
        let sleep_request = WebsocketMessage::Jupyter(JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: Uuid::new_v4().to_string(),
                msg_type: "execute_request".to_string(),
            },
            parent_header: None,
            channel: JupyterChannel::Shell,
            content: serde_json::json!({
                "code": "import time\ntime.sleep(120)",
                "silent": false,
                "store_history": true,
                "user_expressions": {},
                "allow_stdin": false,
                "stop_on_error": true
            }),
            metadata: serde_json::json!({}),
            buffers: vec![],
        });
        comm.send_message(&sleep_request)
            .await
            .expect("failed to send execute_request");
        tokio::time::sleep(Duration::from_secs(3)).await;

        let response = client
            .interrupt_session(session_id.clone())
            .await
            .expect("interrupt request failed");
        assert!(
            matches!(
                response,
                kallichore_api::InterruptSessionResponse::Interrupted(_)
            ),
            "expected the session to be interrupted, got {:?}",
            response
        );

        // Python reports a SIGINT during sleep() as a KeyboardInterrupt.
        let saw_interrupt = tokio::time::timeout(Duration::from_secs(20), async {
            while let Ok(Some(text)) = comm.receive_message().await {
                if text.contains("KeyboardInterrupt") {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(
            saw_interrupt,
            "kernel should have reported a KeyboardInterrupt after SIGINT"
        );

        let _ = comm.close().await;
        drop(server);
    })
    .await;

    result.expect("test timed out");
}
