//
// jupyter_messages.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use kcshared::jupyter_message::JupyterMessage;
use serde::Deserialize;

/// An enum of message types we know how to handle from Jupyter. This is in no
/// way exhaustive; it just includes the types we care about.
#[allow(dead_code)]
pub enum JupyterMsg {
    ExecuteRequest(JupyterExecuteRequest),
    InterruptRequest,
    ShutdownRequest,
    Status(JupyterStatus),
    Other,
}

/// Convert a JupyterMessage (generic type) into a JupyterMsg (specific type)
impl From<JupyterMessage> for JupyterMsg {
    fn from(msg: JupyterMessage) -> Self {
        match msg.header.msg_type.as_str() {
            "execute_request" => match serde_json::from_value::<JupyterExecuteRequest>(msg.content)
            {
                Ok(content) => JupyterMsg::ExecuteRequest(content),
                Err(_) => JupyterMsg::Other,
            },
            "status" => match serde_json::from_value::<JupyterStatus>(msg.content) {
                Ok(content) => JupyterMsg::Status(content),
                Err(_) => JupyterMsg::Other,
            },
            "interrupt_request" => JupyterMsg::InterruptRequest,
            "shutdown_request" => JupyterMsg::ShutdownRequest,
            _ => JupyterMsg::Other,
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct JupyterExecuteRequest {
    pub code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionState {
    Busy,
    Idle,
}

#[derive(Deserialize)]
pub struct JupyterStatus {
    pub execution_state: ExecutionState,
}
