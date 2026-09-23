//
// execution_history.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! A bounded record of the code a session has run and what it produced.
//!
//! Every part of an entry is clipped as it arrives, and only the most recent
//! entries are kept, so the history never holds more than
//! `MAX_ENTRIES * (MAX_INPUT_BYTES + MAX_OUTPUT_BYTES + MAX_ERROR_BYTES)`
//! bytes of text, however much output a kernel produces.

use std::collections::VecDeque;
use std::fmt;

use chrono::Utc;
use kallichore_api::models;
use kcshared::jupyter_message::JupyterMessage;
use serde_json::Value;

/// The most executions a session remembers.
pub const MAX_ENTRIES: usize = 100;

/// The most executions reported with a session's details.
pub const RECENT_ENTRIES: usize = 3;

/// The most code kept for one execution.
const MAX_INPUT_BYTES: usize = 4 * 1024;

/// The most output kept for one execution.
const MAX_OUTPUT_BYTES: usize = 4 * 1024;

/// The most error text (name, message, and traceback) kept for one execution.
const MAX_ERROR_BYTES: usize = 4 * 1024;

/// The most bytes of an error's name kept.
const MAX_ERROR_NAME_BYTES: usize = 256;

/// The executions a session has run, oldest first.
#[derive(Debug, Default)]
pub struct ExecutionHistory {
    entries: VecDeque<Entry>,
}

#[derive(Debug)]
struct Entry {
    /// The `execute_request` this entry records; the parent of its output.
    msg_id: String,
    input: Clip,
    output: Clip,
    error: Option<models::ExecutionError>,
    error_truncated: bool,
    timestamp: i64,
    source: Option<String>,
    agent: Option<String>,
}

impl ExecutionHistory {
    /// Start an entry for an `execute_request` being sent to the kernel.
    /// Silent requests are not recorded, since the kernel reports nothing for
    /// them.
    pub fn begin(&mut self, request: &JupyterMessage) {
        let content = &request.content;
        if content["silent"].as_bool() == Some(true) {
            return;
        }
        let attribution = &request.metadata["attribution"];
        if self.entries.len() == MAX_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            msg_id: request.header.msg_id.clone(),
            input: Clip::from(MAX_INPUT_BYTES, text(content, "code")),
            output: Clip::new(MAX_OUTPUT_BYTES),
            error: None,
            error_truncated: false,
            timestamp: Utc::now().timestamp_millis(),
            source: attribution["source"].as_str().map(String::from),
            agent: attribution["agent_name"].as_str().map(String::from),
        });
    }

    /// Add a message from the kernel to the execution it belongs to, if any.
    pub fn record(&mut self, message: &JupyterMessage) {
        let Some(parent) = &message.parent_header else {
            return;
        };
        let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| entry.msg_id == parent.msg_id)
        else {
            return;
        };
        let content = &message.content;
        match message.header.msg_type.as_str() {
            "stream" => entry.output.push(text(content, "text")),
            "execute_result" | "display_data" => {
                let data = &content["data"];
                match data["text/plain"].as_str() {
                    Some(plain) => entry.output.push(plain),
                    None => {
                        let types: Vec<&str> = data
                            .as_object()
                            .map(|bundle| bundle.keys().map(String::as_str).collect())
                            .unwrap_or_default();
                        entry.output.push(&format!("[{}]", types.join(", ")));
                    }
                }
                entry.output.push("\n");
            }
            "error" => entry.set_error(content),
            "execute_reply" if content["status"] == "error" && entry.error.is_none() => {
                entry.set_error(content)
            }
            _ => {}
        }
    }

    /// Every remembered execution, oldest first.
    pub fn entries(&self) -> Vec<models::ExecutionHistoryEntry> {
        self.entries.iter().map(Entry::to_model).collect()
    }

    /// How many executions are remembered.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// The last `count` executions, oldest first.
    pub fn recent(&self, count: usize) -> Vec<models::ExecutionHistoryEntry> {
        let skip = self.entries.len().saturating_sub(count);
        self.entries
            .iter()
            .skip(skip)
            .map(Entry::to_model)
            .collect()
    }
}

impl Entry {
    /// Record an error from an `error` message or an `execute_reply`, keeping
    /// the end of the traceback, where the failure is.
    fn set_error(&mut self, content: &Value) {
        let name = Clip::from(MAX_ERROR_NAME_BYTES, text(content, "ename"));
        let message = Clip::from(MAX_ERROR_BYTES / 2, text(content, "evalue"));
        let lines: Vec<&str> = content["traceback"]
            .as_array()
            .map(|lines| lines.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        let mut budget = MAX_ERROR_BYTES - name.len() - message.len();
        let mut traceback: Vec<String> = lines
            .iter()
            .rev()
            .take_while(|line| match budget.checked_sub(line.len()) {
                Some(left) => {
                    budget = left;
                    true
                }
                None => false,
            })
            .map(|line| line.to_string())
            .collect();
        traceback.reverse();

        self.error_truncated =
            name.truncated() || message.truncated() || traceback.len() < lines.len();
        self.error = Some(models::ExecutionError {
            name: name.to_string(),
            message: message.to_string(),
            traceback,
        });
    }

    fn to_model(&self) -> models::ExecutionHistoryEntry {
        models::ExecutionHistoryEntry {
            input: self.input.to_string(),
            output: self.output.to_string(),
            error: self.error.clone(),
            timestamp: self.timestamp,
            source: self.source.clone(),
            agent: self.agent.clone(),
            truncated: self.input.truncated() || self.output.truncated() || self.error_truncated,
        }
    }
}

/// Text that keeps at most `max` bytes: the first half it is given and the
/// most recent half, counting what it drops between them.
#[derive(Debug)]
struct Clip {
    max: usize,
    head: String,
    tail: String,
    omitted: usize,
}

impl Clip {
    fn new(max: usize) -> Self {
        Self {
            max,
            head: String::new(),
            tail: String::new(),
            omitted: 0,
        }
    }

    fn from(max: usize, text: &str) -> Self {
        let mut clip = Self::new(max);
        clip.push(text);
        clip
    }

    fn push(&mut self, mut text: &str) {
        let half = self.max / 2;
        if self.tail.is_empty() {
            let end = floor_boundary(text, half.saturating_sub(self.head.len()));
            self.head.push_str(&text[..end]);
            text = &text[end..];
        }
        if text.len() >= half {
            let start = ceil_boundary(text, text.len() - half);
            self.omitted += self.tail.len() + start;
            self.tail = text[start..].to_string();
        } else {
            self.tail.push_str(text);
            if self.tail.len() > half {
                let start = ceil_boundary(&self.tail, self.tail.len() - half);
                self.omitted += start;
                self.tail.drain(..start);
            }
        }
    }

    fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    fn truncated(&self) -> bool {
        self.omitted > 0
    }
}

impl fmt::Display for Clip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.head)?;
        if self.truncated() {
            write!(f, "\n[... {} bytes omitted ...]\n", self.omitted)?;
        }
        f.write_str(&self.tail)
    }
}

/// A string field of a message's content, or an empty string.
fn text<'a>(content: &'a Value, key: &str) -> &'a str {
    content[key].as_str().unwrap_or("")
}

/// The largest character boundary in `text` at or before `index`.
fn floor_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// The smallest character boundary in `text` at or after `index`.
fn ceil_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use kcshared::jupyter_message::{JupyterChannel, JupyterMessageHeader};
    use serde_json::json;

    fn message(
        msg_type: &str,
        parent: Option<&str>,
        content: Value,
        metadata: Value,
    ) -> JupyterMessage {
        JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: format!("{}-id", msg_type),
                msg_type: msg_type.to_string(),
            },
            parent_header: parent.map(|id| JupyterMessageHeader {
                msg_id: id.to_string(),
                msg_type: "execute_request".to_string(),
            }),
            channel: JupyterChannel::IOPub,
            content,
            metadata,
            buffers: vec![],
        }
    }

    fn request(msg_id: &str, code: &str, silent: bool) -> JupyterMessage {
        let mut request = message(
            "execute_request",
            None,
            json!({ "code": code, "silent": silent }),
            json!({ "attribution": { "source": "agent", "agent_name": "claude-code" } }),
        );
        request.header.msg_id = msg_id.to_string();
        request
    }

    #[test]
    fn clip_keeps_head_and_tail_within_budget() {
        let mut clip = Clip::new(8);
        for chunk in ["abc", "def", "ghijkl", "mn"] {
            clip.push(chunk);
        }
        assert_eq!(clip.len(), 8);
        assert_eq!(clip.to_string(), "abcd\n[... 6 bytes omitted ...]\nklmn");

        let clip = Clip::from(8, "abcdefgh");
        assert!(!clip.truncated());
        assert_eq!(clip.to_string(), "abcdefgh");
    }

    #[test]
    fn clip_cuts_on_character_boundaries() {
        let clip = Clip::from(6, "ééééé");
        assert!(clip.len() <= 6);
        assert_eq!(clip.to_string(), "é\n[... 6 bytes omitted ...]\né");
    }

    #[test]
    fn records_output_and_errors_for_their_request() {
        let mut history = ExecutionHistory::default();
        history.begin(&request("a", "print(1); 1 / 0", false));
        history.begin(&request("quiet", "x", true));
        history.record(&message(
            "stream",
            Some("a"),
            json!({ "name": "stdout", "text": "1\n" }),
            json!({}),
        ));
        history.record(&message(
            "stream",
            Some("other"),
            json!({ "text": "stray" }),
            json!({}),
        ));
        history.record(&message(
            "display_data",
            Some("a"),
            json!({ "data": { "image/png": "iVBO" } }),
            json!({}),
        ));
        history.record(&message(
            "error",
            Some("a"),
            json!({ "ename": "ZeroDivisionError", "evalue": "division by zero", "traceback": ["frame", "last"] }),
            json!({}),
        ));

        let entries = history.entries();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.input, "print(1); 1 / 0");
        assert_eq!(entry.output, "1\n[image/png]\n");
        assert_eq!(entry.source.as_deref(), Some("agent"));
        assert_eq!(entry.agent.as_deref(), Some("claude-code"));
        let error = entry.error.as_ref().unwrap();
        assert_eq!(error.name, "ZeroDivisionError");
        assert_eq!(error.traceback, vec!["frame", "last"]);
        assert!(!entry.truncated);
    }

    #[test]
    fn keeps_the_end_of_a_long_traceback() {
        let mut history = ExecutionHistory::default();
        history.begin(&request("a", "boom()", false));
        let frames: Vec<String> = (0..MAX_ERROR_BYTES).map(|i| format!("{:04}", i)).collect();
        history.record(&message(
            "error",
            Some("a"),
            json!({ "ename": "E", "evalue": "", "traceback": frames }),
            json!({}),
        ));

        let entry = &history.entries()[0];
        let traceback = &entry.error.as_ref().unwrap().traceback;
        assert_eq!(traceback.last(), frames.last());
        assert!(traceback.len() * 4 <= MAX_ERROR_BYTES);
        assert!(entry.truncated);
    }

    #[test]
    fn forgets_the_oldest_executions() {
        let mut history = ExecutionHistory::default();
        for i in 0..MAX_ENTRIES + 5 {
            history.begin(&request(&i.to_string(), &i.to_string(), false));
        }
        let entries = history.entries();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].input, "5");

        let recent = history.recent(RECENT_ENTRIES);
        assert_eq!(recent.len(), RECENT_ENTRIES);
        assert_eq!(recent.last().unwrap().input, (MAX_ENTRIES + 4).to_string());
    }
}
