#![allow(unused_qualifications)]

use validator::Validate;

#[cfg(any(feature = "client", feature = "server"))]
use crate::header;
use crate::models;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ActiveSession {
    /// A unique identifier for the session
    #[serde(rename = "session_id")]
    pub session_id: String,

    /// The program and command-line parameters for the session
    #[serde(rename = "argv")]
    pub argv: Vec<String>,

    /// The underlying process ID of the session, if the session is running.
    #[serde(rename = "process_id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<i32>,

    /// The username of the user who owns the session
    #[serde(rename = "username")]
    pub username: String,

    /// A human-readable name for the session
    #[serde(rename = "display_name")]
    pub display_name: String,

    /// The interpreter language
    #[serde(rename = "language")]
    pub language: String,

    #[serde(rename = "interrupt_mode")]
    pub interrupt_mode: models::InterruptMode,

    /// The environment variables set when the session was started
    #[serde(rename = "initial_env")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_env: Option<std::collections::HashMap<String, String>>,

    /// Whether the session is connected to a client
    #[serde(rename = "connected")]
    pub connected: bool,

    /// An ISO 8601 timestamp of when the session was started
    #[serde(rename = "started")]
    pub started: chrono::DateTime<chrono::Utc>,

    /// The session's current working directory
    #[serde(rename = "working_directory")]
    pub working_directory: String,

    /// The text to use to prompt for input
    #[serde(rename = "input_prompt")]
    pub input_prompt: String,

    /// The text to use to prompt for input continuations
    #[serde(rename = "continuation_prompt")]
    pub continuation_prompt: String,

    #[serde(rename = "execution_queue")]
    pub execution_queue: models::ExecutionQueue,

    #[serde(rename = "status")]
    pub status: models::Status,

    /// The number of seconds the session has been idle, or 0 if the session is busy
    #[serde(rename = "idle_seconds")]
    pub idle_seconds: i32,

    /// The number of seconds the session has been busy, or 0 if the session is idle
    #[serde(rename = "busy_seconds")]
    pub busy_seconds: i32,
}

impl ActiveSession {
    #[allow(clippy::new_without_default)]
    pub fn new(
        session_id: String,
        argv: Vec<String>,
        username: String,
        display_name: String,
        language: String,
        interrupt_mode: models::InterruptMode,
        connected: bool,
        started: chrono::DateTime<chrono::Utc>,
        working_directory: String,
        input_prompt: String,
        continuation_prompt: String,
        execution_queue: models::ExecutionQueue,
        status: models::Status,
        idle_seconds: i32,
        busy_seconds: i32,
    ) -> ActiveSession {
        ActiveSession {
            session_id,
            argv,
            process_id: None,
            username,
            display_name,
            language,
            interrupt_mode,
            initial_env: None,
            connected,
            started,
            working_directory,
            input_prompt,
            continuation_prompt,
            execution_queue,
            status,
            idle_seconds,
            busy_seconds,
        }
    }
}

/// Converts the ActiveSession value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for ActiveSession {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            Some("session_id".to_string()),
            Some(self.session_id.to_string()),
            Some("argv".to_string()),
            Some(
                self.argv
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            self.process_id
                .as_ref()
                .map(|process_id| ["process_id".to_string(), process_id.to_string()].join(",")),
            Some("username".to_string()),
            Some(self.username.to_string()),
            Some("display_name".to_string()),
            Some(self.display_name.to_string()),
            Some("language".to_string()),
            Some(self.language.to_string()),
            // Skipping interrupt_mode in query parameter serialization

            // Skipping initial_env in query parameter serialization
            Some("connected".to_string()),
            Some(self.connected.to_string()),
            // Skipping started in query parameter serialization
            Some("working_directory".to_string()),
            Some(self.working_directory.to_string()),
            Some("input_prompt".to_string()),
            Some(self.input_prompt.to_string()),
            Some("continuation_prompt".to_string()),
            Some(self.continuation_prompt.to_string()),
            // Skipping execution_queue in query parameter serialization

            // Skipping status in query parameter serialization
            Some("idle_seconds".to_string()),
            Some(self.idle_seconds.to_string()),
            Some("busy_seconds".to_string()),
            Some(self.busy_seconds.to_string()),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ActiveSession value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ActiveSession {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub session_id: Vec<String>,
            pub argv: Vec<Vec<String>>,
            pub process_id: Vec<i32>,
            pub username: Vec<String>,
            pub display_name: Vec<String>,
            pub language: Vec<String>,
            pub interrupt_mode: Vec<models::InterruptMode>,
            pub initial_env: Vec<std::collections::HashMap<String, String>>,
            pub connected: Vec<bool>,
            pub started: Vec<chrono::DateTime<chrono::Utc>>,
            pub working_directory: Vec<String>,
            pub input_prompt: Vec<String>,
            pub continuation_prompt: Vec<String>,
            pub execution_queue: Vec<models::ExecutionQueue>,
            pub status: Vec<models::Status>,
            pub idle_seconds: Vec<i32>,
            pub busy_seconds: Vec<i32>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing ActiveSession".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "session_id" => intermediate_rep.session_id.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "argv" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ActiveSession"
                                .to_string(),
                        )
                    }
                    #[allow(clippy::redundant_clone)]
                    "process_id" => intermediate_rep.process_id.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "username" => intermediate_rep.username.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "display_name" => intermediate_rep.display_name.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "language" => intermediate_rep.language.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "interrupt_mode" => intermediate_rep.interrupt_mode.push(
                        <models::InterruptMode as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    "initial_env" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ActiveSession"
                                .to_string(),
                        )
                    }
                    #[allow(clippy::redundant_clone)]
                    "connected" => intermediate_rep.connected.push(
                        <bool as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "started" => intermediate_rep.started.push(
                        <chrono::DateTime<chrono::Utc> as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "working_directory" => intermediate_rep.working_directory.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "input_prompt" => intermediate_rep.input_prompt.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "continuation_prompt" => intermediate_rep.continuation_prompt.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "execution_queue" => intermediate_rep.execution_queue.push(
                        <models::ExecutionQueue as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "status" => intermediate_rep.status.push(
                        <models::Status as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "idle_seconds" => intermediate_rep.idle_seconds.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "busy_seconds" => intermediate_rep.busy_seconds.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ActiveSession".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ActiveSession {
            session_id: intermediate_rep
                .session_id
                .into_iter()
                .next()
                .ok_or_else(|| "session_id missing in ActiveSession".to_string())?,
            argv: intermediate_rep
                .argv
                .into_iter()
                .next()
                .ok_or_else(|| "argv missing in ActiveSession".to_string())?,
            process_id: intermediate_rep.process_id.into_iter().next(),
            username: intermediate_rep
                .username
                .into_iter()
                .next()
                .ok_or_else(|| "username missing in ActiveSession".to_string())?,
            display_name: intermediate_rep
                .display_name
                .into_iter()
                .next()
                .ok_or_else(|| "display_name missing in ActiveSession".to_string())?,
            language: intermediate_rep
                .language
                .into_iter()
                .next()
                .ok_or_else(|| "language missing in ActiveSession".to_string())?,
            interrupt_mode: intermediate_rep
                .interrupt_mode
                .into_iter()
                .next()
                .ok_or_else(|| "interrupt_mode missing in ActiveSession".to_string())?,
            initial_env: intermediate_rep.initial_env.into_iter().next(),
            connected: intermediate_rep
                .connected
                .into_iter()
                .next()
                .ok_or_else(|| "connected missing in ActiveSession".to_string())?,
            started: intermediate_rep
                .started
                .into_iter()
                .next()
                .ok_or_else(|| "started missing in ActiveSession".to_string())?,
            working_directory: intermediate_rep
                .working_directory
                .into_iter()
                .next()
                .ok_or_else(|| "working_directory missing in ActiveSession".to_string())?,
            input_prompt: intermediate_rep
                .input_prompt
                .into_iter()
                .next()
                .ok_or_else(|| "input_prompt missing in ActiveSession".to_string())?,
            continuation_prompt: intermediate_rep
                .continuation_prompt
                .into_iter()
                .next()
                .ok_or_else(|| "continuation_prompt missing in ActiveSession".to_string())?,
            execution_queue: intermediate_rep
                .execution_queue
                .into_iter()
                .next()
                .ok_or_else(|| "execution_queue missing in ActiveSession".to_string())?,
            status: intermediate_rep
                .status
                .into_iter()
                .next()
                .ok_or_else(|| "status missing in ActiveSession".to_string())?,
            idle_seconds: intermediate_rep
                .idle_seconds
                .into_iter()
                .next()
                .ok_or_else(|| "idle_seconds missing in ActiveSession".to_string())?,
            busy_seconds: intermediate_rep
                .busy_seconds
                .into_iter()
                .next()
                .ok_or_else(|| "busy_seconds missing in ActiveSession".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ActiveSession> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ActiveSession>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ActiveSession>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ActiveSession - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ActiveSession> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ActiveSession as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into ActiveSession - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

/// Connection information for an existing session
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ConnectionInfo {
    /// The port for control messages
    #[serde(rename = "control_port")]
    pub control_port: i32,

    /// The port for shell messages
    #[serde(rename = "shell_port")]
    pub shell_port: i32,

    /// The port for stdin messages
    #[serde(rename = "stdin_port")]
    pub stdin_port: i32,

    /// The port for heartbeat messages
    #[serde(rename = "hb_port")]
    pub hb_port: i32,

    /// The port for IOPub messages
    #[serde(rename = "iopub_port")]
    pub iopub_port: i32,

    /// The signature scheme for messages
    #[serde(rename = "signature_scheme")]
    pub signature_scheme: String,

    /// The key for messages
    #[serde(rename = "key")]
    pub key: String,

    /// The transport protocol
    #[serde(rename = "transport")]
    pub transport: String,

    /// The IP address for the connection
    #[serde(rename = "ip")]
    pub ip: String,
}

impl ConnectionInfo {
    #[allow(clippy::new_without_default)]
    pub fn new(
        control_port: i32,
        shell_port: i32,
        stdin_port: i32,
        hb_port: i32,
        iopub_port: i32,
        signature_scheme: String,
        key: String,
        transport: String,
        ip: String,
    ) -> ConnectionInfo {
        ConnectionInfo {
            control_port,
            shell_port,
            stdin_port,
            hb_port,
            iopub_port,
            signature_scheme,
            key,
            transport,
            ip,
        }
    }
}

/// Converts the ConnectionInfo value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for ConnectionInfo {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            Some("control_port".to_string()),
            Some(self.control_port.to_string()),
            Some("shell_port".to_string()),
            Some(self.shell_port.to_string()),
            Some("stdin_port".to_string()),
            Some(self.stdin_port.to_string()),
            Some("hb_port".to_string()),
            Some(self.hb_port.to_string()),
            Some("iopub_port".to_string()),
            Some(self.iopub_port.to_string()),
            Some("signature_scheme".to_string()),
            Some(self.signature_scheme.to_string()),
            Some("key".to_string()),
            Some(self.key.to_string()),
            Some("transport".to_string()),
            Some(self.transport.to_string()),
            Some("ip".to_string()),
            Some(self.ip.to_string()),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ConnectionInfo value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ConnectionInfo {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub control_port: Vec<i32>,
            pub shell_port: Vec<i32>,
            pub stdin_port: Vec<i32>,
            pub hb_port: Vec<i32>,
            pub iopub_port: Vec<i32>,
            pub signature_scheme: Vec<String>,
            pub key: Vec<String>,
            pub transport: Vec<String>,
            pub ip: Vec<String>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing ConnectionInfo".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "control_port" => intermediate_rep.control_port.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "shell_port" => intermediate_rep.shell_port.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "stdin_port" => intermediate_rep.stdin_port.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "hb_port" => intermediate_rep.hb_port.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "iopub_port" => intermediate_rep.iopub_port.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "signature_scheme" => intermediate_rep.signature_scheme.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "key" => intermediate_rep.key.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "transport" => intermediate_rep.transport.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "ip" => intermediate_rep.ip.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ConnectionInfo".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ConnectionInfo {
            control_port: intermediate_rep
                .control_port
                .into_iter()
                .next()
                .ok_or_else(|| "control_port missing in ConnectionInfo".to_string())?,
            shell_port: intermediate_rep
                .shell_port
                .into_iter()
                .next()
                .ok_or_else(|| "shell_port missing in ConnectionInfo".to_string())?,
            stdin_port: intermediate_rep
                .stdin_port
                .into_iter()
                .next()
                .ok_or_else(|| "stdin_port missing in ConnectionInfo".to_string())?,
            hb_port: intermediate_rep
                .hb_port
                .into_iter()
                .next()
                .ok_or_else(|| "hb_port missing in ConnectionInfo".to_string())?,
            iopub_port: intermediate_rep
                .iopub_port
                .into_iter()
                .next()
                .ok_or_else(|| "iopub_port missing in ConnectionInfo".to_string())?,
            signature_scheme: intermediate_rep
                .signature_scheme
                .into_iter()
                .next()
                .ok_or_else(|| "signature_scheme missing in ConnectionInfo".to_string())?,
            key: intermediate_rep
                .key
                .into_iter()
                .next()
                .ok_or_else(|| "key missing in ConnectionInfo".to_string())?,
            transport: intermediate_rep
                .transport
                .into_iter()
                .next()
                .ok_or_else(|| "transport missing in ConnectionInfo".to_string())?,
            ip: intermediate_rep
                .ip
                .into_iter()
                .next()
                .ok_or_else(|| "ip missing in ConnectionInfo".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ConnectionInfo> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ConnectionInfo>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ConnectionInfo>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ConnectionInfo - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ConnectionInfo> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ConnectionInfo as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into ConnectionInfo - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct Error {
    #[serde(rename = "code")]
    pub code: String,

    #[serde(rename = "message")]
    pub message: String,

    #[serde(rename = "details")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl Error {
    #[allow(clippy::new_without_default)]
    pub fn new(code: String, message: String) -> Error {
        Error {
            code,
            message,
            details: None,
        }
    }
}

/// Converts the Error value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for Error {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            Some("code".to_string()),
            Some(self.code.to_string()),
            Some("message".to_string()),
            Some(self.message.to_string()),
            self.details
                .as_ref()
                .map(|details| ["details".to_string(), details.to_string()].join(",")),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a Error value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for Error {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub code: Vec<String>,
            pub message: Vec<String>,
            pub details: Vec<String>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing Error".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "code" => intermediate_rep.code.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "message" => intermediate_rep.message.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "details" => intermediate_rep.details.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing Error".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(Error {
            code: intermediate_rep
                .code
                .into_iter()
                .next()
                .ok_or_else(|| "code missing in Error".to_string())?,
            message: intermediate_rep
                .message
                .into_iter()
                .next()
                .ok_or_else(|| "message missing in Error".to_string())?,
            details: intermediate_rep.details.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<Error> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<Error>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<Error>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for Error - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<Error> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => match <Error as std::str::FromStr>::from_str(value) {
                std::result::Result::Ok(value) => {
                    std::result::Result::Ok(header::IntoHeaderValue(value))
                }
                std::result::Result::Err(err) => std::result::Result::Err(format!(
                    "Unable to convert header value '{}' into Error - {}",
                    value, err
                )),
            },
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

/// The execution queue for a session
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ExecutionQueue {
    /// The execution request currently being evaluated, if any
    #[serde(rename = "active")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<serde_json::Value>,

    /// The number of items in the pending queue
    #[serde(rename = "length")]
    pub length: i32,

    /// The queue of pending execution requests
    #[serde(rename = "pending")]
    pub pending: Vec<serde_json::Value>,
}

impl ExecutionQueue {
    #[allow(clippy::new_without_default)]
    pub fn new(length: i32, pending: Vec<serde_json::Value>) -> ExecutionQueue {
        ExecutionQueue {
            active: None,
            length,
            pending,
        }
    }
}

/// Converts the ExecutionQueue value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for ExecutionQueue {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            // Skipping active in query parameter serialization
            Some("length".to_string()),
            Some(self.length.to_string()),
            // Skipping pending in query parameter serialization
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ExecutionQueue value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ExecutionQueue {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub active: Vec<serde_json::Value>,
            pub length: Vec<i32>,
            pub pending: Vec<Vec<serde_json::Value>>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing ExecutionQueue".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "active" => intermediate_rep.active.push(
                        <serde_json::Value as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "length" => intermediate_rep.length.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "pending" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ExecutionQueue"
                                .to_string(),
                        )
                    }
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ExecutionQueue".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ExecutionQueue {
            active: intermediate_rep.active.into_iter().next(),
            length: intermediate_rep
                .length
                .into_iter()
                .next()
                .ok_or_else(|| "length missing in ExecutionQueue".to_string())?,
            pending: intermediate_rep
                .pending
                .into_iter()
                .next()
                .ok_or_else(|| "pending missing in ExecutionQueue".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ExecutionQueue> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ExecutionQueue>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ExecutionQueue>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ExecutionQueue - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ExecutionQueue> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ExecutionQueue as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into ExecutionQueue - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

/// The mechansim for interrupting the session
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum InterruptMode {
    #[serde(rename = "signal")]
    Signal,
    #[serde(rename = "message")]
    Message,
}

impl std::fmt::Display for InterruptMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            InterruptMode::Signal => write!(f, "signal"),
            InterruptMode::Message => write!(f, "message"),
        }
    }
}

impl std::str::FromStr for InterruptMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "signal" => std::result::Result::Ok(InterruptMode::Signal),
            "message" => std::result::Result::Ok(InterruptMode::Message),
            _ => std::result::Result::Err(format!("Value not valid: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct NewSession {
    /// A unique identifier for the session
    #[serde(rename = "session_id")]
    pub session_id: String,

    /// A human-readable name for the session
    #[serde(rename = "display_name")]
    pub display_name: String,

    /// The interpreter language
    #[serde(rename = "language")]
    pub language: String,

    /// The username of the user who owns the session
    #[serde(rename = "username")]
    pub username: String,

    /// The text to use to prompt for input
    #[serde(rename = "input_prompt")]
    pub input_prompt: String,

    /// The text to use to prompt for input continuations
    #[serde(rename = "continuation_prompt")]
    pub continuation_prompt: String,

    /// The program and command-line parameters for the session
    #[serde(rename = "argv")]
    pub argv: Vec<String>,

    /// The working directory in which to start the session.
    #[serde(rename = "working_directory")]
    pub working_directory: String,

    /// A list of environment variable actions to perform
    #[serde(rename = "env")]
    pub env: Vec<models::VarAction>,

    /// The number of seconds to wait for a connection to the session's ZeroMQ sockets before timing out
    #[serde(rename = "connection_timeout")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_timeout: Option<i32>,

    #[serde(rename = "interrupt_mode")]
    pub interrupt_mode: models::InterruptMode,

    /// The Jupyter protocol version supported by the underlying kernel
    #[serde(rename = "protocol_version")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,

    /// Whether to run the session inside a login shell; only relevant on POSIX systems
    #[serde(rename = "run_in_shell")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_in_shell: Option<bool>,
}

impl NewSession {
    #[allow(clippy::new_without_default)]
    pub fn new(
        session_id: String,
        display_name: String,
        language: String,
        username: String,
        input_prompt: String,
        continuation_prompt: String,
        argv: Vec<String>,
        working_directory: String,
        env: Vec<models::VarAction>,
        interrupt_mode: models::InterruptMode,
    ) -> NewSession {
        NewSession {
            session_id,
            display_name,
            language,
            username,
            input_prompt,
            continuation_prompt,
            argv,
            working_directory,
            env,
            connection_timeout: Some(30),
            interrupt_mode,
            protocol_version: Some("5.3".to_string()),
            run_in_shell: Some(false),
        }
    }
}

/// Converts the NewSession value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for NewSession {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            Some("session_id".to_string()),
            Some(self.session_id.to_string()),
            Some("display_name".to_string()),
            Some(self.display_name.to_string()),
            Some("language".to_string()),
            Some(self.language.to_string()),
            Some("username".to_string()),
            Some(self.username.to_string()),
            Some("input_prompt".to_string()),
            Some(self.input_prompt.to_string()),
            Some("continuation_prompt".to_string()),
            Some(self.continuation_prompt.to_string()),
            Some("argv".to_string()),
            Some(
                self.argv
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            Some("working_directory".to_string()),
            Some(self.working_directory.to_string()),
            // Skipping env in query parameter serialization
            self.connection_timeout.as_ref().map(|connection_timeout| {
                [
                    "connection_timeout".to_string(),
                    connection_timeout.to_string(),
                ]
                .join(",")
            }),
            // Skipping interrupt_mode in query parameter serialization
            self.protocol_version.as_ref().map(|protocol_version| {
                ["protocol_version".to_string(), protocol_version.to_string()].join(",")
            }),
            self.run_in_shell.as_ref().map(|run_in_shell| {
                ["run_in_shell".to_string(), run_in_shell.to_string()].join(",")
            }),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a NewSession value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for NewSession {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub session_id: Vec<String>,
            pub display_name: Vec<String>,
            pub language: Vec<String>,
            pub username: Vec<String>,
            pub input_prompt: Vec<String>,
            pub continuation_prompt: Vec<String>,
            pub argv: Vec<Vec<String>>,
            pub working_directory: Vec<String>,
            pub env: Vec<Vec<models::VarAction>>,
            pub connection_timeout: Vec<i32>,
            pub interrupt_mode: Vec<models::InterruptMode>,
            pub protocol_version: Vec<String>,
            pub run_in_shell: Vec<bool>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing NewSession".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "session_id" => intermediate_rep.session_id.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "display_name" => intermediate_rep.display_name.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "language" => intermediate_rep.language.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "username" => intermediate_rep.username.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "input_prompt" => intermediate_rep.input_prompt.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "continuation_prompt" => intermediate_rep.continuation_prompt.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "argv" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in NewSession"
                                .to_string(),
                        )
                    }
                    #[allow(clippy::redundant_clone)]
                    "working_directory" => intermediate_rep.working_directory.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "env" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in NewSession"
                                .to_string(),
                        )
                    }
                    #[allow(clippy::redundant_clone)]
                    "connection_timeout" => intermediate_rep.connection_timeout.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "interrupt_mode" => intermediate_rep.interrupt_mode.push(
                        <models::InterruptMode as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "protocol_version" => intermediate_rep.protocol_version.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "run_in_shell" => intermediate_rep.run_in_shell.push(
                        <bool as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing NewSession".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(NewSession {
            session_id: intermediate_rep
                .session_id
                .into_iter()
                .next()
                .ok_or_else(|| "session_id missing in NewSession".to_string())?,
            display_name: intermediate_rep
                .display_name
                .into_iter()
                .next()
                .ok_or_else(|| "display_name missing in NewSession".to_string())?,
            language: intermediate_rep
                .language
                .into_iter()
                .next()
                .ok_or_else(|| "language missing in NewSession".to_string())?,
            username: intermediate_rep
                .username
                .into_iter()
                .next()
                .ok_or_else(|| "username missing in NewSession".to_string())?,
            input_prompt: intermediate_rep
                .input_prompt
                .into_iter()
                .next()
                .ok_or_else(|| "input_prompt missing in NewSession".to_string())?,
            continuation_prompt: intermediate_rep
                .continuation_prompt
                .into_iter()
                .next()
                .ok_or_else(|| "continuation_prompt missing in NewSession".to_string())?,
            argv: intermediate_rep
                .argv
                .into_iter()
                .next()
                .ok_or_else(|| "argv missing in NewSession".to_string())?,
            working_directory: intermediate_rep
                .working_directory
                .into_iter()
                .next()
                .ok_or_else(|| "working_directory missing in NewSession".to_string())?,
            env: intermediate_rep
                .env
                .into_iter()
                .next()
                .ok_or_else(|| "env missing in NewSession".to_string())?,
            connection_timeout: intermediate_rep.connection_timeout.into_iter().next(),
            interrupt_mode: intermediate_rep
                .interrupt_mode
                .into_iter()
                .next()
                .ok_or_else(|| "interrupt_mode missing in NewSession".to_string())?,
            protocol_version: intermediate_rep.protocol_version.into_iter().next(),
            run_in_shell: intermediate_rep.run_in_shell.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<NewSession> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<NewSession>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<NewSession>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for NewSession - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<NewSession> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <NewSession as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into NewSession - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct NewSession200Response {
    /// A unique identifier for the session
    #[serde(rename = "session_id")]
    pub session_id: String,
}

impl NewSession200Response {
    #[allow(clippy::new_without_default)]
    pub fn new(session_id: String) -> NewSession200Response {
        NewSession200Response { session_id }
    }
}

/// Converts the NewSession200Response value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for NewSession200Response {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            Some("session_id".to_string()),
            Some(self.session_id.to_string()),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a NewSession200Response value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for NewSession200Response {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub session_id: Vec<String>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing NewSession200Response".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "session_id" => intermediate_rep.session_id.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing NewSession200Response".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(NewSession200Response {
            session_id: intermediate_rep
                .session_id
                .into_iter()
                .next()
                .ok_or_else(|| "session_id missing in NewSession200Response".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<NewSession200Response> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<NewSession200Response>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<NewSession200Response>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for NewSession200Response - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<NewSession200Response>
{
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <NewSession200Response as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into NewSession200Response - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct RestartSession {
    /// The desired working directory for the session after restart, if different from the session's working directory at startup
    #[serde(rename = "working_directory")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,

    /// A list of environment variable actions to perform
    #[serde(rename = "env")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<models::VarAction>>,
}

impl RestartSession {
    #[allow(clippy::new_without_default)]
    pub fn new() -> RestartSession {
        RestartSession {
            working_directory: None,
            env: None,
        }
    }
}

/// Converts the RestartSession value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for RestartSession {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            self.working_directory.as_ref().map(|working_directory| {
                [
                    "working_directory".to_string(),
                    working_directory.to_string(),
                ]
                .join(",")
            }),
            // Skipping env in query parameter serialization
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a RestartSession value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for RestartSession {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub working_directory: Vec<String>,
            pub env: Vec<Vec<models::VarAction>>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing RestartSession".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "working_directory" => intermediate_rep.working_directory.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "env" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in RestartSession"
                                .to_string(),
                        )
                    }
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing RestartSession".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(RestartSession {
            working_directory: intermediate_rep.working_directory.into_iter().next(),
            env: intermediate_rep.env.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<RestartSession> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<RestartSession>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<RestartSession>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for RestartSession - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<RestartSession> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <RestartSession as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into RestartSession - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ServerConfiguration {
    /// The number of hours the server will wait before shutting down idle sessions (-1 if idle shutdown is disabled)
    #[serde(rename = "idle_shutdown_hours")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_shutdown_hours: Option<i32>,

    /// The current log level
    // Note: inline enums are not fully supported by openapi-generator
    #[serde(rename = "log_level")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
}

impl ServerConfiguration {
    #[allow(clippy::new_without_default)]
    pub fn new() -> ServerConfiguration {
        ServerConfiguration {
            idle_shutdown_hours: None,
            log_level: None,
        }
    }
}

/// Converts the ServerConfiguration value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for ServerConfiguration {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            self.idle_shutdown_hours
                .as_ref()
                .map(|idle_shutdown_hours| {
                    [
                        "idle_shutdown_hours".to_string(),
                        idle_shutdown_hours.to_string(),
                    ]
                    .join(",")
                }),
            self.log_level
                .as_ref()
                .map(|log_level| ["log_level".to_string(), log_level.to_string()].join(",")),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ServerConfiguration value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ServerConfiguration {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub idle_shutdown_hours: Vec<i32>,
            pub log_level: Vec<String>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing ServerConfiguration".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "idle_shutdown_hours" => intermediate_rep.idle_shutdown_hours.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "log_level" => intermediate_rep.log_level.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ServerConfiguration".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ServerConfiguration {
            idle_shutdown_hours: intermediate_rep.idle_shutdown_hours.into_iter().next(),
            log_level: intermediate_rep.log_level.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ServerConfiguration> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ServerConfiguration>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ServerConfiguration>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ServerConfiguration - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<ServerConfiguration>
{
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ServerConfiguration as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into ServerConfiguration - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ServerStatus {
    #[serde(rename = "sessions")]
    pub sessions: i32,

    #[serde(rename = "active")]
    pub active: i32,

    #[serde(rename = "busy")]
    pub busy: bool,

    /// The number of seconds all sessions have been idle, or 0 if any session is busy
    #[serde(rename = "idle_seconds")]
    pub idle_seconds: i32,

    /// The number of seconds any session has been busy, or 0 if all sessions are idle
    #[serde(rename = "busy_seconds")]
    pub busy_seconds: i32,

    #[serde(rename = "version")]
    pub version: String,
}

impl ServerStatus {
    #[allow(clippy::new_without_default)]
    pub fn new(
        sessions: i32,
        active: i32,
        busy: bool,
        idle_seconds: i32,
        busy_seconds: i32,
        version: String,
    ) -> ServerStatus {
        ServerStatus {
            sessions,
            active,
            busy,
            idle_seconds,
            busy_seconds,
            version,
        }
    }
}

/// Converts the ServerStatus value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for ServerStatus {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            Some("sessions".to_string()),
            Some(self.sessions.to_string()),
            Some("active".to_string()),
            Some(self.active.to_string()),
            Some("busy".to_string()),
            Some(self.busy.to_string()),
            Some("idle_seconds".to_string()),
            Some(self.idle_seconds.to_string()),
            Some("busy_seconds".to_string()),
            Some(self.busy_seconds.to_string()),
            Some("version".to_string()),
            Some(self.version.to_string()),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ServerStatus value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ServerStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub sessions: Vec<i32>,
            pub active: Vec<i32>,
            pub busy: Vec<bool>,
            pub idle_seconds: Vec<i32>,
            pub busy_seconds: Vec<i32>,
            pub version: Vec<String>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing ServerStatus".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "sessions" => intermediate_rep.sessions.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "active" => intermediate_rep.active.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "busy" => intermediate_rep.busy.push(
                        <bool as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "idle_seconds" => intermediate_rep.idle_seconds.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "busy_seconds" => intermediate_rep.busy_seconds.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "version" => intermediate_rep.version.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ServerStatus".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ServerStatus {
            sessions: intermediate_rep
                .sessions
                .into_iter()
                .next()
                .ok_or_else(|| "sessions missing in ServerStatus".to_string())?,
            active: intermediate_rep
                .active
                .into_iter()
                .next()
                .ok_or_else(|| "active missing in ServerStatus".to_string())?,
            busy: intermediate_rep
                .busy
                .into_iter()
                .next()
                .ok_or_else(|| "busy missing in ServerStatus".to_string())?,
            idle_seconds: intermediate_rep
                .idle_seconds
                .into_iter()
                .next()
                .ok_or_else(|| "idle_seconds missing in ServerStatus".to_string())?,
            busy_seconds: intermediate_rep
                .busy_seconds
                .into_iter()
                .next()
                .ok_or_else(|| "busy_seconds missing in ServerStatus".to_string())?,
            version: intermediate_rep
                .version
                .into_iter()
                .next()
                .ok_or_else(|| "version missing in ServerStatus".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ServerStatus> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ServerStatus>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ServerStatus>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ServerStatus - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ServerStatus> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ServerStatus as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into ServerStatus - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct SessionList {
    #[serde(rename = "total")]
    pub total: i32,

    #[serde(rename = "sessions")]
    pub sessions: Vec<models::ActiveSession>,
}

impl SessionList {
    #[allow(clippy::new_without_default)]
    pub fn new(total: i32, sessions: Vec<models::ActiveSession>) -> SessionList {
        SessionList { total, sessions }
    }
}

/// Converts the SessionList value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for SessionList {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            Some("total".to_string()),
            Some(self.total.to_string()),
            // Skipping sessions in query parameter serialization
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a SessionList value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for SessionList {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub total: Vec<i32>,
            pub sessions: Vec<Vec<models::ActiveSession>>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing SessionList".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "total" => intermediate_rep.total.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "sessions" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in SessionList"
                                .to_string(),
                        )
                    }
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing SessionList".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(SessionList {
            total: intermediate_rep
                .total
                .into_iter()
                .next()
                .ok_or_else(|| "total missing in SessionList".to_string())?,
            sessions: intermediate_rep
                .sessions
                .into_iter()
                .next()
                .ok_or_else(|| "sessions missing in SessionList".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<SessionList> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<SessionList>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<SessionList>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for SessionList - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<SessionList> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <SessionList as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into SessionList - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct StartupError {
    /// The exit code of the process, if it exited
    #[serde(rename = "exit_code")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,

    /// The output of the process (combined stdout and stderr) emitted during startup, if any
    #[serde(rename = "output")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,

    #[serde(rename = "error")]
    pub error: models::Error,
}

impl StartupError {
    #[allow(clippy::new_without_default)]
    pub fn new(error: models::Error) -> StartupError {
        StartupError {
            exit_code: None,
            output: None,
            error,
        }
    }
}

/// Converts the StartupError value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for StartupError {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            self.exit_code
                .as_ref()
                .map(|exit_code| ["exit_code".to_string(), exit_code.to_string()].join(",")),
            self.output
                .as_ref()
                .map(|output| ["output".to_string(), output.to_string()].join(",")),
            // Skipping error in query parameter serialization
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a StartupError value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for StartupError {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub exit_code: Vec<i32>,
            pub output: Vec<String>,
            pub error: Vec<models::Error>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing StartupError".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "exit_code" => intermediate_rep.exit_code.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "output" => intermediate_rep.output.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "error" => intermediate_rep.error.push(
                        <models::Error as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing StartupError".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(StartupError {
            exit_code: intermediate_rep.exit_code.into_iter().next(),
            output: intermediate_rep.output.into_iter().next(),
            error: intermediate_rep
                .error
                .into_iter()
                .next()
                .ok_or_else(|| "error missing in StartupError".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<StartupError> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<StartupError>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<StartupError>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for StartupError - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<StartupError> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <StartupError as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into StartupError - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

/// The status of the session
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum Status {
    #[serde(rename = "uninitialized")]
    Uninitialized,
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "busy")]
    Busy,
    #[serde(rename = "offline")]
    Offline,
    #[serde(rename = "exited")]
    Exited,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Status::Uninitialized => write!(f, "uninitialized"),
            Status::Starting => write!(f, "starting"),
            Status::Ready => write!(f, "ready"),
            Status::Idle => write!(f, "idle"),
            Status::Busy => write!(f, "busy"),
            Status::Offline => write!(f, "offline"),
            Status::Exited => write!(f, "exited"),
        }
    }
}

impl std::str::FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "uninitialized" => std::result::Result::Ok(Status::Uninitialized),
            "starting" => std::result::Result::Ok(Status::Starting),
            "ready" => std::result::Result::Ok(Status::Ready),
            "idle" => std::result::Result::Ok(Status::Idle),
            "busy" => std::result::Result::Ok(Status::Busy),
            "offline" => std::result::Result::Ok(Status::Offline),
            "exited" => std::result::Result::Ok(Status::Exited),
            _ => std::result::Result::Err(format!("Value not valid: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct VarAction {
    #[serde(rename = "action")]
    pub action: models::VarActionType,

    /// The name of the variable to act on
    #[serde(rename = "name")]
    pub name: String,

    /// The value to replace, append, or prepend
    #[serde(rename = "value")]
    pub value: String,
}

impl VarAction {
    #[allow(clippy::new_without_default)]
    pub fn new(action: models::VarActionType, name: String, value: String) -> VarAction {
        VarAction {
            action,
            name,
            value,
        }
    }
}

/// Converts the VarAction value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for VarAction {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![
            // Skipping action in query parameter serialization
            Some("name".to_string()),
            Some(self.name.to_string()),
            Some("value".to_string()),
            Some(self.value.to_string()),
        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a VarAction value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for VarAction {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub action: Vec<models::VarActionType>,
            pub name: Vec<String>,
            pub value: Vec<String>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => {
                    return std::result::Result::Err(
                        "Missing value while parsing VarAction".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "action" => intermediate_rep.action.push(
                        <models::VarActionType as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "name" => intermediate_rep.name.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "value" => intermediate_rep.value.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing VarAction".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(VarAction {
            action: intermediate_rep
                .action
                .into_iter()
                .next()
                .ok_or_else(|| "action missing in VarAction".to_string())?,
            name: intermediate_rep
                .name
                .into_iter()
                .next()
                .ok_or_else(|| "name missing in VarAction".to_string())?,
            value: intermediate_rep
                .value
                .into_iter()
                .next()
                .ok_or_else(|| "value missing in VarAction".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<VarAction> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<VarAction>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<VarAction>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for VarAction - value: {} is invalid {}",
                hdr_value, e
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<VarAction> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <VarAction as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{}' into VarAction - {}",
                        value, err
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {:?} to string: {}",
                hdr_value, e
            )),
        }
    }
}

/// The type of action to perform on the environment variable
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum VarActionType {
    #[serde(rename = "replace")]
    Replace,
    #[serde(rename = "append")]
    Append,
    #[serde(rename = "prepend")]
    Prepend,
}

impl std::fmt::Display for VarActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            VarActionType::Replace => write!(f, "replace"),
            VarActionType::Append => write!(f, "append"),
            VarActionType::Prepend => write!(f, "prepend"),
        }
    }
}

impl std::str::FromStr for VarActionType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "replace" => std::result::Result::Ok(VarActionType::Replace),
            "append" => std::result::Result::Ok(VarActionType::Append),
            "prepend" => std::result::Result::Ok(VarActionType::Prepend),
            _ => std::result::Result::Err(format!("Value not valid: {}", s)),
        }
    }
}
