//
// execution_queue.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::collections::VecDeque;

use kallichore_api::models;
use kcshared::jupyter_message::JupyterMessage;

#[derive(Debug)]
pub struct ExecutionQueue {
    pub active: Option<JupyterMessage>,
    pub pending: VecDeque<JupyterMessage>,
}

impl ExecutionQueue {
    /// Create a new execution queue
    pub fn new() -> Self {
        ExecutionQueue {
            active: None,
            pending: VecDeque::new(),
        }
    }

    /// Clear the execution queue
    pub fn clear(&mut self) {
        self.active = None;
        if self.pending.len() > 0 {
            log::debug!(
                "Discarding {} pending execution requests",
                self.pending.len()
            );
        }
        self.pending.clear();
    }

    /// Process a given request, either executing it immediately or queueing it
    /// for later execution.
    ///
    /// Returns true if the request can be executed immediately, or false if it
    /// was queued for later execution.
    pub fn process_request(&mut self, request: JupyterMessage) -> bool {
        match &self.active {
            None => {
                log::trace!(
                    "Executing request {} immediately (no requests are waiting)",
                    request.header.msg_id
                );
                self.active = Some(request.clone());
                true
            }
            Some(active) => {
                log::debug!(
                    "Queueing request {} (active request is {}; there are {} pending requests)",
                    request.header.msg_id,
                    active.header.msg_id,
                    self.pending.len()
                );
                self.pending.push_back(request);
                false
            }
        }
    }

    /// Gets the next request to execute, if any
    pub fn next_request(&mut self) -> Option<JupyterMessage> {
        let req = self.pending.pop_front();
        match req {
            Some(req) => {
                log::debug!(
                    "Executing pending request {} ({} pending requests remain)",
                    req.header.msg_id,
                    self.pending.len()
                );
                self.active = Some(req.clone());
                Some(req)
            }
            None => {
                self.active = None;
                None
            }
        }
    }

    /// Serialize the execution queue as a model
    pub fn to_json(&self) -> models::ExecutionQueue {
        // Serialize the indiviual pending requests
        let pending: Vec<serde_json::Value> = self
            .pending
            .iter()
            .map(|msg| match serde_json::to_value(msg) {
                Ok(value) => value,
                Err(e) => {
                    log::error!("Failed to serialize pending request: {}", e);
                    serde_json::Value::Null
                }
            })
            .collect();

        // Reverse the order of the pending requests; VecDeque's iterator
        // returns front to back, but we want to return the queue in the order
        // it will be executed (back to front)
        let pending: Vec<serde_json::Value> = pending.into_iter().rev().collect();

        // Serialize the active request and the vector of pending requests
        models::ExecutionQueue {
            active: match &self.active {
                Some(active) => match serde_json::to_value(active) {
                    Ok(value) => Some(value),
                    Err(e) => {
                        log::error!("Failed to serialize active request: {}", e);
                        None
                    }
                },
                None => None,
            },
            length: pending.len() as i32,
            pending,
        }
    }
}
