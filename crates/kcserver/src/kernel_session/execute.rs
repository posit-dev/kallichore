//
// execute.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Executes code in a kernel session and collects its output.
//!
//! Shared by the `/sessions/{id}/execute` REST endpoint and the MCP
//! `execute_code`/`evaluate_code` tools, so both go through the same execution
//! queue and are mirrored to the session's WebSocket client.

use std::time::Duration;

use chrono::Utc;
use kallichore_api::models;
use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    kernel_message::{ExecutionAttribution, ExecutionRequested, KernelMessage},
    websocket_message::WebsocketMessage,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{make_message_id, KernelSession};

/// How long to wait for a starting kernel to become ready before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// How often to re-check a starting kernel's status.
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A request to execute code in a session.
pub struct ExecuteOptions {
    /// The code to execute.
    pub code: String,

    /// The Jupyter `silent` flag, which stops the kernel broadcasting anything
    /// on iopub at all -- including the result.
    pub silent: bool,

    /// Whether the execution increments the kernel's execution counter and
    /// enters its history.
    pub store_history: bool,

    /// Whether to abort queued executions when this one fails.
    pub stop_on_error: bool,

    /// How long to wait before abandoning the execution.
    pub timeout: Option<Duration>,

    /// Who requested the execution, when it isn't the connected client.
    pub attribution: Option<ExecutionAttribution>,

    /// Cancelled when the caller no longer wants the result. The execution is
    /// then abandoned, as it is on a timeout.
    pub cancel: Option<CancellationToken>,

    /// Receives the text of each stream message as it arrives.
    pub stream_tx: Option<mpsc::UnboundedSender<String>>,
}

/// Errors that can occur while executing code.
#[derive(Debug)]
pub enum ExecuteError {
    /// The session is not in a state where it can run code.
    NotReady(String),

    /// The request could not be handed to the kernel.
    SendFailed(String),

    /// Execution did not complete within the requested timeout, and has been
    /// abandoned.
    Timeout,

    /// The caller cancelled the execution, and it has been abandoned.
    Cancelled,

    /// The kernel's message channel closed while the execution was in flight.
    ChannelClosed,
}

impl KernelSession {
    /// Execute code and collect everything the kernel emits in response.
    ///
    /// Returns once the `execute_reply` and the trailing `status: idle` have
    /// both arrived.
    pub async fn execute_collect(
        &self,
        options: ExecuteOptions,
    ) -> Result<models::ExecuteReply, ExecuteError> {
        self.wait_until_runnable().await?;

        let msg_id = make_message_id();
        let attribution_metadata = match &options.attribution {
            Some(attribution) => serde_json::json!({ "attribution": attribution }),
            None => serde_json::json!({}),
        };

        let jupyter_msg = JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: msg_id.clone(),
                msg_type: "execute_request".to_string(),
            },
            parent_header: None,
            channel: JupyterChannel::Shell,
            content: serde_json::json!({
                "code": options.code,
                "silent": options.silent,
                "store_history": options.store_history,
                "user_expressions": {},
                "allow_stdin": false,
                "stop_on_error": options.stop_on_error,
            }),
            metadata: attribution_metadata,
            buffers: vec![],
        };

        // Register an RPC listener for this msg_id
        let (rpc_tx, mut rpc_rx) = mpsc::unbounded_channel::<JupyterMessage>();
        {
            let mut state = self.state.write().await;
            state.rpc_listeners.insert(msg_id.clone(), rpc_tx);
        }

        // Tell the client who asked for this, before any iopub traffic for it
        // can arrive. The channel buffers while no client is connected, so a
        // window that reconnects later still learns the provenance of the
        // output it is about to receive.
        if let Some(attribution) = options.attribution {
            self.announce_execution(&msg_id, &options.code, attribution)
                .await;
        }

        // Send the message through the same path as WebSocket executions,
        // which routes through the execution queue in the ZMQ proxy
        if let Err(e) = self.ws_zmq_tx.send(jupyter_msg).await {
            let mut state = self.state.write().await;
            state.rpc_listeners.remove(&msg_id);
            return Err(ExecuteError::SendFailed(e.to_string()));
        }

        let result = collect_execution_output(
            &mut rpc_rx,
            &msg_id,
            options.timeout,
            options.cancel.as_ref(),
            options.stream_tx.as_ref(),
        )
        .await;

        {
            let mut state = self.state.write().await;
            state.rpc_listeners.remove(&msg_id);
        }

        if matches!(
            result,
            Err(ExecuteError::Timeout) | Err(ExecuteError::Cancelled)
        ) {
            self.abandon(&msg_id).await;
        }

        result
    }

    /// Stop an execution nobody is waiting for: withdraw it if it is still
    /// queued, or interrupt the kernel if it is running. Code it was queued
    /// behind is left alone.
    async fn abandon(&self, msg_id: &str) {
        let running = {
            let mut state = self.state.write().await;
            let queue = &mut state.execution_queue;
            if queue.remove_pending(msg_id) {
                return;
            }
            queue
                .active
                .as_ref()
                .is_some_and(|active| active.header.msg_id == msg_id)
        };
        if running {
            if let Err(e) = self.interrupt().await {
                log::warn!(
                    "Failed to interrupt kernel after abandoning execution: {}",
                    e
                );
            }
        }
    }

    /// Wait for a kernel that is still coming up to become runnable.
    ///
    /// Callers commonly issue an execute request immediately after
    /// `start_session`, before the kernel has published its first idle status
    /// on iopub, so waiting is friendlier than rejecting outright.
    async fn wait_until_runnable(&self) -> Result<(), ExecuteError> {
        let deadline = std::time::Instant::now() + READY_TIMEOUT;
        loop {
            let status = { self.state.read().await.status };
            match status {
                models::Status::Idle | models::Status::Busy => return Ok(()),
                models::Status::Uninitialized
                | models::Status::Starting
                | models::Status::Ready => {
                    if std::time::Instant::now() >= deadline {
                        return Err(ExecuteError::NotReady(format!(
                            "Session did not become ready within {:?} (last status: '{}')",
                            READY_TIMEOUT, status
                        )));
                    }
                    tokio::time::sleep(READY_POLL_INTERVAL).await;
                }
                models::Status::Offline | models::Status::Exited => {
                    return Err(ExecuteError::NotReady(format!(
                        "Session is in '{}' state; must be idle or busy to execute code",
                        status
                    )));
                }
            }
        }
    }

    /// Push an `ExecutionRequested` event onto the client channel.
    async fn announce_execution(
        &self,
        msg_id: &str,
        code: &str,
        attribution: ExecutionAttribution,
    ) {
        let event =
            WebsocketMessage::Kernel(KernelMessage::ExecutionRequested(ExecutionRequested {
                msg_id: msg_id.to_string(),
                code: code.to_string(),
                requested_at: Utc::now(),
                attribution,
            }));
        if let Err(e) = self.ws_json_tx.send(event).await {
            log::warn!(
                "[session {}] Failed to announce execution {}: {}",
                self.connection.session_id,
                msg_id,
                e
            );
        }
    }
}

/// Collect execution output from the RPC listener channel until the
/// execute_reply arrives on the shell channel.
async fn collect_execution_output(
    rpc_rx: &mut mpsc::UnboundedReceiver<JupyterMessage>,
    msg_id: &str,
    timeout_duration: Option<Duration>,
    cancel: Option<&CancellationToken>,
    stream_tx: Option<&mpsc::UnboundedSender<String>>,
) -> Result<models::ExecuteReply, ExecuteError> {
    let mut output: Vec<models::ExecuteOutput> = Vec::new();
    let mut data: Option<std::collections::HashMap<String, String>> = None;
    let mut status = models::ExecuteReplyStatus::Ok;
    let mut execution_count: i32 = 0;
    let mut error_name: Option<String> = None;
    let mut error_message: Option<String> = None;
    let mut error_traceback: Option<Vec<String>> = None;

    // We need both execute_reply (shell) and status:idle (IOPub) before
    // returning, because ZMQ delivers them over different sockets and the
    // execute_result on IOPub may arrive after execute_reply on shell.
    let mut got_execute_reply = false;
    let mut got_idle = false;

    // Use an absolute deadline so the timeout covers total execution time,
    // not each individual message receive.
    let deadline = timeout_duration.map(|d| tokio::time::Instant::now() + d);

    let cancelled = async {
        match cancel {
            Some(cancel) => cancel.cancelled().await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(cancelled);

    loop {
        let received = async {
            match deadline {
                Some(deadline) => tokio::time::timeout_at(deadline, rpc_rx.recv())
                    .await
                    .map_err(|_| ExecuteError::Timeout),
                None => Ok(rpc_rx.recv().await),
            }
        };
        let msg = tokio::select! {
            _ = &mut cancelled => return Err(ExecuteError::Cancelled),
            received = received => match received? {
                Some(msg) => msg,
                None => return Err(ExecuteError::ChannelClosed),
            },
        };

        let msg_type = msg.header.msg_type.as_str();
        log::trace!(
            "execute_code RPC received message type '{}' for msg_id '{}'",
            msg_type,
            msg_id,
        );

        match msg_type {
            "stream" => {
                let mut entry = models::ExecuteOutput::new(models::ExecuteOutputType::Stream);
                entry.stream_name = msg
                    .content
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                entry.text = msg
                    .content
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                if let (Some(tx), Some(text)) = (stream_tx, &entry.text) {
                    let _ = tx.send(text.clone());
                }
                output.push(entry);
            }
            "display_data" => {
                let mut entry = models::ExecuteOutput::new(models::ExecuteOutputType::DisplayData);
                entry.data = mime_map(msg.content.get("data"));
                entry.metadata = msg.content.get("metadata").cloned();
                output.push(entry);
            }
            "error" => {
                let mut entry = models::ExecuteOutput::new(models::ExecuteOutputType::Error);
                entry.error_name = msg
                    .content
                    .get("ename")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                entry.error_message = msg
                    .content
                    .get("evalue")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                entry.error_traceback = traceback(msg.content.get("traceback"));
                output.push(entry);
            }
            "execute_result" => {
                // Hoist into the top-level `data` field of ExecuteReply
                data = mime_map(msg.content.get("data"));
                execution_count = msg
                    .content
                    .get("execution_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
            }
            "execute_reply" => {
                // This is the shell reply that signals execution is complete.
                let reply_status = msg
                    .content
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ok");
                status = if reply_status == "error" {
                    models::ExecuteReplyStatus::Error
                } else {
                    models::ExecuteReplyStatus::Ok
                };
                execution_count = msg
                    .content
                    .get("execution_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(execution_count as i64) as i32;

                // Extract error info from the reply itself if present
                if reply_status == "error" {
                    error_name = msg
                        .content
                        .get("ename")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    error_message = msg
                        .content
                        .get("evalue")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    error_traceback = traceback(msg.content.get("traceback"));
                }

                got_execute_reply = true;
                if got_idle {
                    break;
                }
            }
            "status" => {
                let is_idle =
                    msg.content.get("execution_state").and_then(|v| v.as_str()) == Some("idle");
                if is_idle {
                    got_idle = true;
                    if got_execute_reply {
                        break;
                    }
                }
            }
            "execute_input" => {
                // Echo of the submitted code; nothing to collect.
            }
            other => {
                log::debug!(
                    "execute_code RPC ignoring unexpected message type '{}' for msg_id '{}'",
                    other,
                    msg_id,
                );
            }
        }
    }

    let mut reply = models::ExecuteReply::new(status, execution_count, output);
    reply.data = data;
    reply.error_name = error_name;
    reply.error_message = error_message;
    reply.error_traceback = error_traceback;

    Ok(reply)
}

/// Flatten a Jupyter MIME bundle into a map of strings, rendering non-string
/// values (such as JSON payloads) as JSON text.
fn mime_map(
    value: Option<&serde_json::Value>,
) -> Option<std::collections::HashMap<String, String>> {
    value.and_then(|v| {
        v.as_object().map(|obj| {
            obj.iter()
                .map(|(k, v)| {
                    let s = v
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| v.to_string());
                    (k.clone(), s)
                })
                .collect()
        })
    })
}

/// Extract a traceback, which Jupyter sends as an array of strings.
fn traceback(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    value.and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
    })
}
