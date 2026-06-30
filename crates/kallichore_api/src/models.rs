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

    #[serde(rename = "session_mode")]
    pub session_mode: models::SessionMode,

    /// The session's current working directory
    #[serde(rename = "working_directory")]
    pub working_directory: String,

    /// For notebook sessions, the URI of the notebook file
    #[serde(rename = "notebook_uri")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_uri: Option<String>,

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

    /// The kernel information, as returned by the kernel_info_request message
    #[serde(rename = "kernel_info")]
    pub kernel_info: serde_json::Value,

    #[serde(rename = "resource_usage")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_usage: Option<models::ResourceUsage>,

    /// The number of seconds the session has been idle, or 0 if the session is busy
    #[serde(rename = "idle_seconds")]
    pub idle_seconds: i32,

    /// The number of seconds the session has been busy, or 0 if the session is idle
    #[serde(rename = "busy_seconds")]
    pub busy_seconds: i32,

    /// The path to the Unix domain socket used to send/receive data from the session, if applicable
    #[serde(rename = "socket_path")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<String>,
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
        session_mode: models::SessionMode,
        working_directory: String,
        input_prompt: String,
        continuation_prompt: String,
        execution_queue: models::ExecutionQueue,
        status: models::Status,
        kernel_info: serde_json::Value,
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
            session_mode,
            working_directory,
            notebook_uri: None,
            input_prompt,
            continuation_prompt,
            execution_queue,
            status,
            kernel_info,
            resource_usage: None,
            idle_seconds,
            busy_seconds,
            socket_path: None,
        }
    }
}

/// Converts the ActiveSession value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ActiveSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            // Skipping non-primitive type interrupt_mode in query parameter serialization
            // Skipping map initial_env in query parameter serialization
            Some("connected".to_string()),
            Some(self.connected.to_string()),
            // Skipping non-primitive type started in query parameter serialization
            // Skipping non-primitive type session_mode in query parameter serialization
            Some("working_directory".to_string()),
            Some(self.working_directory.to_string()),
            self.notebook_uri.as_ref().map(|notebook_uri| {
                ["notebook_uri".to_string(), notebook_uri.to_string()].join(",")
            }),
            Some("input_prompt".to_string()),
            Some(self.input_prompt.to_string()),
            Some("continuation_prompt".to_string()),
            Some(self.continuation_prompt.to_string()),
            // Skipping non-primitive type execution_queue in query parameter serialization
            // Skipping non-primitive type status in query parameter serialization
            // Skipping non-primitive type kernel_info in query parameter serialization
            // Skipping non-primitive type resource_usage in query parameter serialization
            Some("idle_seconds".to_string()),
            Some(self.idle_seconds.to_string()),
            Some("busy_seconds".to_string()),
            Some(self.busy_seconds.to_string()),
            self.socket_path
                .as_ref()
                .map(|socket_path| ["socket_path".to_string(), socket_path.to_string()].join(",")),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ActiveSession value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
            pub session_mode: Vec<models::SessionMode>,
            pub working_directory: Vec<String>,
            pub notebook_uri: Vec<String>,
            pub input_prompt: Vec<String>,
            pub continuation_prompt: Vec<String>,
            pub execution_queue: Vec<models::ExecutionQueue>,
            pub status: Vec<models::Status>,
            pub kernel_info: Vec<serde_json::Value>,
            pub resource_usage: Vec<models::ResourceUsage>,
            pub idle_seconds: Vec<i32>,
            pub busy_seconds: Vec<i32>,
            pub socket_path: Vec<String>,
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
                    "session_mode" => intermediate_rep.session_mode.push(
                        <models::SessionMode as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "working_directory" => intermediate_rep.working_directory.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "notebook_uri" => intermediate_rep.notebook_uri.push(
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
                    "kernel_info" => intermediate_rep.kernel_info.push(
                        <serde_json::Value as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "resource_usage" => intermediate_rep.resource_usage.push(
                        <models::ResourceUsage as std::str::FromStr>::from_str(val)
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
                    #[allow(clippy::redundant_clone)]
                    "socket_path" => intermediate_rep.socket_path.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
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
            session_mode: intermediate_rep
                .session_mode
                .into_iter()
                .next()
                .ok_or_else(|| "session_mode missing in ActiveSession".to_string())?,
            working_directory: intermediate_rep
                .working_directory
                .into_iter()
                .next()
                .ok_or_else(|| "working_directory missing in ActiveSession".to_string())?,
            notebook_uri: intermediate_rep.notebook_uri.into_iter().next(),
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
            kernel_info: intermediate_rep
                .kernel_info
                .into_iter()
                .next()
                .ok_or_else(|| "kernel_info missing in ActiveSession".to_string())?,
            resource_usage: intermediate_rep.resource_usage.into_iter().next(),
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
            socket_path: intermediate_rep.socket_path.into_iter().next(),
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
                "Invalid header value for ActiveSession - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into ActiveSession - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ActiveSession>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ActiveSession>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ActiveSession>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ActiveSession> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ActiveSession as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ActiveSession - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ClientHeartbeat {
    /// The process ID of the client sending the heartbeat
    #[serde(rename = "process_id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<i32>,
}

impl ClientHeartbeat {
    #[allow(clippy::new_without_default)]
    pub fn new() -> ClientHeartbeat {
        ClientHeartbeat { process_id: None }
    }
}

/// Converts the ClientHeartbeat value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ClientHeartbeat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![self
            .process_id
            .as_ref()
            .map(|process_id| ["process_id".to_string(), process_id.to_string()].join(","))];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ClientHeartbeat value
/// as specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ClientHeartbeat {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub process_id: Vec<i32>,
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
                        "Missing value while parsing ClientHeartbeat".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "process_id" => intermediate_rep.process_id.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ClientHeartbeat".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ClientHeartbeat {
            process_id: intermediate_rep.process_id.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ClientHeartbeat> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ClientHeartbeat>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ClientHeartbeat>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ClientHeartbeat - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<ClientHeartbeat>
{
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ClientHeartbeat as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into ClientHeartbeat - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ClientHeartbeat>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ClientHeartbeat>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ClientHeartbeat>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ClientHeartbeat> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ClientHeartbeat as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ClientHeartbeat - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ConnectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ConnectionInfo value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
                "Invalid header value for ConnectionInfo - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into ConnectionInfo - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ConnectionInfo>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ConnectionInfo>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ConnectionInfo>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ConnectionInfo> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ConnectionInfo as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ConnectionInfo - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            Some("code".to_string()),
            Some(self.code.to_string()),
            Some("message".to_string()),
            Some(self.message.to_string()),
            self.details
                .as_ref()
                .map(|details| ["details".to_string(), details.to_string()].join(",")),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a Error value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
                "Invalid header value for Error - value: {hdr_value} is invalid {e}"
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
                    "Unable to convert header value '{value}' into Error - {err}"
                )),
            },
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<Error>>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<Error>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<Vec<Error>> {
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<Error> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <Error as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into Error - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// A single output message produced during code execution
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ExecuteOutput {
    #[serde(rename = "type")]
    pub r#type: models::ExecuteOutputType,

    /// The stream name (stdout or stderr), for stream output
    #[serde(rename = "stream_name")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_name: Option<String>,

    /// The text content, for stream output
    #[serde(rename = "text")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// MIME-keyed data, for display_data output
    #[serde(rename = "data")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<std::collections::HashMap<String, String>>,

    /// Metadata dictionary, for display_data output
    #[serde(rename = "metadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// The error name, for error output
    #[serde(rename = "error_name")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_name: Option<String>,

    /// The error message, for error output
    #[serde(rename = "error_message")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    /// The error traceback lines, for error output
    #[serde(rename = "error_traceback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_traceback: Option<Vec<String>>,
}

impl ExecuteOutput {
    #[allow(clippy::new_without_default)]
    pub fn new(r#type: models::ExecuteOutputType) -> ExecuteOutput {
        ExecuteOutput {
            r#type,
            stream_name: None,
            text: None,
            data: None,
            metadata: None,
            error_name: None,
            error_message: None,
            error_traceback: None,
        }
    }
}

/// Converts the ExecuteOutput value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ExecuteOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            // Skipping non-primitive type type in query parameter serialization
            self.stream_name
                .as_ref()
                .map(|stream_name| ["stream_name".to_string(), stream_name.to_string()].join(",")),
            self.text
                .as_ref()
                .map(|text| ["text".to_string(), text.to_string()].join(",")),
            // Skipping map data in query parameter serialization
            // Skipping non-primitive type metadata in query parameter serialization
            self.error_name
                .as_ref()
                .map(|error_name| ["error_name".to_string(), error_name.to_string()].join(",")),
            self.error_message.as_ref().map(|error_message| {
                ["error_message".to_string(), error_message.to_string()].join(",")
            }),
            self.error_traceback.as_ref().map(|error_traceback| {
                [
                    "error_traceback".to_string(),
                    error_traceback
                        .iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                ]
                .join(",")
            }),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ExecuteOutput value
/// as specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ExecuteOutput {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub r#type: Vec<models::ExecuteOutputType>,
            pub stream_name: Vec<String>,
            pub text: Vec<String>,
            pub data: Vec<std::collections::HashMap<String, String>>,
            pub metadata: Vec<serde_json::Value>,
            pub error_name: Vec<String>,
            pub error_message: Vec<String>,
            pub error_traceback: Vec<Vec<String>>,
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
                        "Missing value while parsing ExecuteOutput".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "type" => intermediate_rep.r#type.push(
                        <models::ExecuteOutputType as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "stream_name" => intermediate_rep.stream_name.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "text" => intermediate_rep.text.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "data" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ExecuteOutput"
                                .to_string(),
                        )
                    }
                    #[allow(clippy::redundant_clone)]
                    "metadata" => intermediate_rep.metadata.push(
                        <serde_json::Value as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "error_name" => intermediate_rep.error_name.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "error_message" => intermediate_rep.error_message.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "error_traceback" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ExecuteOutput"
                                .to_string(),
                        )
                    }
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ExecuteOutput".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ExecuteOutput {
            r#type: intermediate_rep
                .r#type
                .into_iter()
                .next()
                .ok_or_else(|| "type missing in ExecuteOutput".to_string())?,
            stream_name: intermediate_rep.stream_name.into_iter().next(),
            text: intermediate_rep.text.into_iter().next(),
            data: intermediate_rep.data.into_iter().next(),
            metadata: intermediate_rep.metadata.into_iter().next(),
            error_name: intermediate_rep.error_name.into_iter().next(),
            error_message: intermediate_rep.error_message.into_iter().next(),
            error_traceback: intermediate_rep.error_traceback.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ExecuteOutput> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ExecuteOutput>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ExecuteOutput>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ExecuteOutput - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ExecuteOutput> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ExecuteOutput as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into ExecuteOutput - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ExecuteOutput>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ExecuteOutput>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ExecuteOutput>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ExecuteOutput> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ExecuteOutput as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ExecuteOutput - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// The output message type
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum ExecuteOutputType {
    #[serde(rename = "stream")]
    Stream,
    #[serde(rename = "display_data")]
    DisplayData,
    #[serde(rename = "error")]
    Error,
}

impl std::fmt::Display for ExecuteOutputType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ExecuteOutputType::Stream => write!(f, "stream"),
            ExecuteOutputType::DisplayData => write!(f, "display_data"),
            ExecuteOutputType::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for ExecuteOutputType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "stream" => std::result::Result::Ok(ExecuteOutputType::Stream),
            "display_data" => std::result::Result::Ok(ExecuteOutputType::DisplayData),
            "error" => std::result::Result::Ok(ExecuteOutputType::Error),
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<ExecuteOutputType> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ExecuteOutputType>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ExecuteOutputType>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ExecuteOutputType - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<ExecuteOutputType>
{
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ExecuteOutputType as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into ExecuteOutputType - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ExecuteOutputType>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ExecuteOutputType>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ExecuteOutputType>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ExecuteOutputType> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ExecuteOutputType as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ExecuteOutputType - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// The result of executing code in a session
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ExecuteReply {
    #[serde(rename = "status")]
    pub status: models::ExecuteReplyStatus,

    /// The kernel's execution counter
    #[serde(rename = "execution_count")]
    pub execution_count: i32,

    /// All output messages produced during execution, in order
    #[serde(rename = "output")]
    pub output: Vec<models::ExecuteOutput>,

    /// The execution result as a MIME-keyed dictionary (from execute_result), if the execution produced a result
    #[serde(rename = "data")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<std::collections::HashMap<String, String>>,

    /// The error name, if the execution failed
    #[serde(rename = "error_name")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_name: Option<String>,

    /// The error message, if the execution failed
    #[serde(rename = "error_message")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    /// The error traceback, if the execution failed
    #[serde(rename = "error_traceback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_traceback: Option<Vec<String>>,
}

impl ExecuteReply {
    #[allow(clippy::new_without_default)]
    pub fn new(
        status: models::ExecuteReplyStatus,
        execution_count: i32,
        output: Vec<models::ExecuteOutput>,
    ) -> ExecuteReply {
        ExecuteReply {
            status,
            execution_count,
            output,
            data: None,
            error_name: None,
            error_message: None,
            error_traceback: None,
        }
    }
}

/// Converts the ExecuteReply value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ExecuteReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            // Skipping non-primitive type status in query parameter serialization
            Some("execution_count".to_string()),
            Some(self.execution_count.to_string()),
            // Skipping non-primitive type output in query parameter serialization
            // Skipping map data in query parameter serialization
            self.error_name
                .as_ref()
                .map(|error_name| ["error_name".to_string(), error_name.to_string()].join(",")),
            self.error_message.as_ref().map(|error_message| {
                ["error_message".to_string(), error_message.to_string()].join(",")
            }),
            self.error_traceback.as_ref().map(|error_traceback| {
                [
                    "error_traceback".to_string(),
                    error_traceback
                        .iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                ]
                .join(",")
            }),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ExecuteReply value
/// as specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ExecuteReply {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub status: Vec<models::ExecuteReplyStatus>,
            pub execution_count: Vec<i32>,
            pub output: Vec<Vec<models::ExecuteOutput>>,
            pub data: Vec<std::collections::HashMap<String, String>>,
            pub error_name: Vec<String>,
            pub error_message: Vec<String>,
            pub error_traceback: Vec<Vec<String>>,
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
                        "Missing value while parsing ExecuteReply".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "status" => intermediate_rep.status.push(
                        <models::ExecuteReplyStatus as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "execution_count" => intermediate_rep.execution_count.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "output" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ExecuteReply"
                                .to_string(),
                        )
                    }
                    "data" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ExecuteReply"
                                .to_string(),
                        )
                    }
                    #[allow(clippy::redundant_clone)]
                    "error_name" => intermediate_rep.error_name.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "error_message" => intermediate_rep.error_message.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    "error_traceback" => {
                        return std::result::Result::Err(
                            "Parsing a container in this style is not supported in ExecuteReply"
                                .to_string(),
                        )
                    }
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ExecuteReply".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ExecuteReply {
            status: intermediate_rep
                .status
                .into_iter()
                .next()
                .ok_or_else(|| "status missing in ExecuteReply".to_string())?,
            execution_count: intermediate_rep
                .execution_count
                .into_iter()
                .next()
                .ok_or_else(|| "execution_count missing in ExecuteReply".to_string())?,
            output: intermediate_rep
                .output
                .into_iter()
                .next()
                .ok_or_else(|| "output missing in ExecuteReply".to_string())?,
            data: intermediate_rep.data.into_iter().next(),
            error_name: intermediate_rep.error_name.into_iter().next(),
            error_message: intermediate_rep.error_message.into_iter().next(),
            error_traceback: intermediate_rep.error_traceback.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ExecuteReply> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ExecuteReply>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ExecuteReply>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ExecuteReply - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ExecuteReply> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ExecuteReply as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into ExecuteReply - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ExecuteReply>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ExecuteReply>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ExecuteReply>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ExecuteReply> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ExecuteReply as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ExecuteReply - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// Whether the execution succeeded or errored
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum ExecuteReplyStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "error")]
    Error,
}

impl std::fmt::Display for ExecuteReplyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ExecuteReplyStatus::Ok => write!(f, "ok"),
            ExecuteReplyStatus::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for ExecuteReplyStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "ok" => std::result::Result::Ok(ExecuteReplyStatus::Ok),
            "error" => std::result::Result::Ok(ExecuteReplyStatus::Error),
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<ExecuteReplyStatus> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ExecuteReplyStatus>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ExecuteReplyStatus>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ExecuteReplyStatus - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<ExecuteReplyStatus>
{
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ExecuteReplyStatus as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into ExecuteReplyStatus - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ExecuteReplyStatus>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ExecuteReplyStatus>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ExecuteReplyStatus>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ExecuteReplyStatus> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ExecuteReplyStatus as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ExecuteReplyStatus - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// A request to execute code in a session
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ExecuteRequest {
    /// The code to execute
    #[serde(rename = "code")]
    pub code: String,

    /// If true, signals the kernel to execute quietly: no broadcast on iopub, no execute_result, and the execution_count is not incremented. Defaults to false.
    #[serde(rename = "silent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silent: Option<bool>,

    /// If true (default), the code is stored in the kernel's history. Set to false for throwaway executions.
    #[serde(rename = "store_history")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_history: Option<bool>,

    /// If true (default), abort the execution queue on error. If false, queued execute requests will still be processed even if this one fails.
    #[serde(rename = "stop_on_error")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_on_error: Option<bool>,

    /// Maximum number of seconds to wait for execution to complete. If not specified, the request will block indefinitely until execution finishes.
    #[serde(rename = "timeout_seconds")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
}

impl ExecuteRequest {
    #[allow(clippy::new_without_default)]
    pub fn new(code: String) -> ExecuteRequest {
        ExecuteRequest {
            code,
            silent: Some(false),
            store_history: Some(true),
            stop_on_error: Some(true),
            timeout_seconds: None,
        }
    }
}

/// Converts the ExecuteRequest value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ExecuteRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            Some("code".to_string()),
            Some(self.code.to_string()),
            self.silent
                .as_ref()
                .map(|silent| ["silent".to_string(), silent.to_string()].join(",")),
            self.store_history.as_ref().map(|store_history| {
                ["store_history".to_string(), store_history.to_string()].join(",")
            }),
            self.stop_on_error.as_ref().map(|stop_on_error| {
                ["stop_on_error".to_string(), stop_on_error.to_string()].join(",")
            }),
            self.timeout_seconds.as_ref().map(|timeout_seconds| {
                ["timeout_seconds".to_string(), timeout_seconds.to_string()].join(",")
            }),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ExecuteRequest value
/// as specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ExecuteRequest {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub code: Vec<String>,
            pub silent: Vec<bool>,
            pub store_history: Vec<bool>,
            pub stop_on_error: Vec<bool>,
            pub timeout_seconds: Vec<i32>,
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
                        "Missing value while parsing ExecuteRequest".to_string(),
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
                    "silent" => intermediate_rep.silent.push(
                        <bool as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "store_history" => intermediate_rep.store_history.push(
                        <bool as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "stop_on_error" => intermediate_rep.stop_on_error.push(
                        <bool as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "timeout_seconds" => intermediate_rep.timeout_seconds.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ExecuteRequest".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ExecuteRequest {
            code: intermediate_rep
                .code
                .into_iter()
                .next()
                .ok_or_else(|| "code missing in ExecuteRequest".to_string())?,
            silent: intermediate_rep.silent.into_iter().next(),
            store_history: intermediate_rep.store_history.into_iter().next(),
            stop_on_error: intermediate_rep.stop_on_error.into_iter().next(),
            timeout_seconds: intermediate_rep.timeout_seconds.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ExecuteRequest> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ExecuteRequest>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ExecuteRequest>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ExecuteRequest - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ExecuteRequest> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ExecuteRequest as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into ExecuteRequest - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ExecuteRequest>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ExecuteRequest>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ExecuteRequest>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ExecuteRequest> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ExecuteRequest as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ExecuteRequest - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ExecutionQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            // Skipping non-primitive type active in query parameter serialization
            Some("length".to_string()),
            Some(self.length.to_string()),
            // Skipping non-primitive type pending in query parameter serialization
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ExecutionQueue value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
                "Invalid header value for ExecutionQueue - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into ExecutionQueue - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ExecutionQueue>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ExecutionQueue>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ExecutionQueue>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ExecutionQueue> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ExecutionQueue as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ExecutionQueue - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
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
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<InterruptMode> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<InterruptMode>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<InterruptMode>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for InterruptMode - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<InterruptMode> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <InterruptMode as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into InterruptMode - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<InterruptMode>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<InterruptMode>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<InterruptMode>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<InterruptMode> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <InterruptMode as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into InterruptMode - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
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

    #[serde(rename = "session_mode")]
    pub session_mode: models::SessionMode,

    /// The working directory in which to start the session.
    #[serde(rename = "working_directory")]
    pub working_directory: String,

    /// For notebook sessions, the URI of the notebook file
    #[serde(rename = "notebook_uri")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_uri: Option<String>,

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

    #[serde(rename = "startup_environment")]
    pub startup_environment: models::StartupEnvironment,

    /// The command or script to run before starting the session
    #[serde(rename = "startup_environment_arg")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_environment_arg: Option<String>,
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
        session_mode: models::SessionMode,
        working_directory: String,
        env: Vec<models::VarAction>,
        interrupt_mode: models::InterruptMode,
        startup_environment: models::StartupEnvironment,
    ) -> NewSession {
        NewSession {
            session_id,
            display_name,
            language,
            username,
            input_prompt,
            continuation_prompt,
            argv,
            session_mode,
            working_directory,
            notebook_uri: None,
            env,
            connection_timeout: Some(30),
            interrupt_mode,
            protocol_version: Some("5.3".to_string()),
            startup_environment,
            startup_environment_arg: None,
        }
    }
}

/// Converts the NewSession value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for NewSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            // Skipping non-primitive type session_mode in query parameter serialization
            Some("working_directory".to_string()),
            Some(self.working_directory.to_string()),
            self.notebook_uri.as_ref().map(|notebook_uri| {
                ["notebook_uri".to_string(), notebook_uri.to_string()].join(",")
            }),
            // Skipping non-primitive type env in query parameter serialization
            self.connection_timeout.as_ref().map(|connection_timeout| {
                [
                    "connection_timeout".to_string(),
                    connection_timeout.to_string(),
                ]
                .join(",")
            }),
            // Skipping non-primitive type interrupt_mode in query parameter serialization
            self.protocol_version.as_ref().map(|protocol_version| {
                ["protocol_version".to_string(), protocol_version.to_string()].join(",")
            }),
            // Skipping non-primitive type startup_environment in query parameter serialization
            self.startup_environment_arg
                .as_ref()
                .map(|startup_environment_arg| {
                    [
                        "startup_environment_arg".to_string(),
                        startup_environment_arg.to_string(),
                    ]
                    .join(",")
                }),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a NewSession value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
            pub session_mode: Vec<models::SessionMode>,
            pub working_directory: Vec<String>,
            pub notebook_uri: Vec<String>,
            pub env: Vec<Vec<models::VarAction>>,
            pub connection_timeout: Vec<i32>,
            pub interrupt_mode: Vec<models::InterruptMode>,
            pub protocol_version: Vec<String>,
            pub startup_environment: Vec<models::StartupEnvironment>,
            pub startup_environment_arg: Vec<String>,
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
                    "session_mode" => intermediate_rep.session_mode.push(
                        <models::SessionMode as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "working_directory" => intermediate_rep.working_directory.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "notebook_uri" => intermediate_rep.notebook_uri.push(
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
                    "startup_environment" => intermediate_rep.startup_environment.push(
                        <models::StartupEnvironment as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "startup_environment_arg" => intermediate_rep.startup_environment_arg.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
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
            session_mode: intermediate_rep
                .session_mode
                .into_iter()
                .next()
                .ok_or_else(|| "session_mode missing in NewSession".to_string())?,
            working_directory: intermediate_rep
                .working_directory
                .into_iter()
                .next()
                .ok_or_else(|| "working_directory missing in NewSession".to_string())?,
            notebook_uri: intermediate_rep.notebook_uri.into_iter().next(),
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
            startup_environment: intermediate_rep
                .startup_environment
                .into_iter()
                .next()
                .ok_or_else(|| "startup_environment missing in NewSession".to_string())?,
            startup_environment_arg: intermediate_rep.startup_environment_arg.into_iter().next(),
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
                "Invalid header value for NewSession - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into NewSession - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<NewSession>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<NewSession>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<NewSession>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<NewSession> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <NewSession as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into NewSession - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for NewSession200Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            Some("session_id".to_string()),
            Some(self.session_id.to_string()),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a NewSession200Response value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Invalid header value for NewSession200Response - value: {hdr_value} is invalid {e}"))
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
                        std::result::Result::Ok(value) => std::result::Result::Ok(header::IntoHeaderValue(value)),
                        std::result::Result::Err(err) => std::result::Result::Err(
                            format!("Unable to convert header value '{value}' into NewSession200Response - {err}"))
                    }
             },
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Unable to convert header: {hdr_value:?} to string: {e}"))
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<NewSession200Response>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<NewSession200Response>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<NewSession200Response>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<NewSession200Response> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <NewSession200Response as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into NewSession200Response - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct ResourceUsage {
    /// The percentage of CPU used by the kernel process and its child processes
    #[serde(rename = "cpu_percent")]
    pub cpu_percent: i64,

    /// The amount of memory used by the kernel process and all of its child processes in bytes
    #[serde(rename = "memory_bytes")]
    pub memory_bytes: i64,

    /// The total number of threads used by the kernel process and its child processes (Linux only)
    #[serde(rename = "thread_count")]
    pub thread_count: i64,

    /// The sampling period in milliseconds over which the resource usage was measured
    #[serde(rename = "sampling_period_ms")]
    pub sampling_period_ms: i64,

    /// A Unix timestamp in milliseconds indicating when the resource usage was sampled
    #[serde(rename = "timestamp")]
    pub timestamp: i64,
}

impl ResourceUsage {
    #[allow(clippy::new_without_default)]
    pub fn new(
        cpu_percent: i64,
        memory_bytes: i64,
        thread_count: i64,
        sampling_period_ms: i64,
        timestamp: i64,
    ) -> ResourceUsage {
        ResourceUsage {
            cpu_percent,
            memory_bytes,
            thread_count,
            sampling_period_ms,
            timestamp,
        }
    }
}

/// Converts the ResourceUsage value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ResourceUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            Some("cpu_percent".to_string()),
            Some(self.cpu_percent.to_string()),
            Some("memory_bytes".to_string()),
            Some(self.memory_bytes.to_string()),
            Some("thread_count".to_string()),
            Some(self.thread_count.to_string()),
            Some("sampling_period_ms".to_string()),
            Some(self.sampling_period_ms.to_string()),
            Some("timestamp".to_string()),
            Some(self.timestamp.to_string()),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ResourceUsage value
/// as specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ResourceUsage {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub cpu_percent: Vec<i64>,
            pub memory_bytes: Vec<i64>,
            pub thread_count: Vec<i64>,
            pub sampling_period_ms: Vec<i64>,
            pub timestamp: Vec<i64>,
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
                        "Missing value while parsing ResourceUsage".to_string(),
                    )
                }
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "cpu_percent" => intermediate_rep.cpu_percent.push(
                        <i64 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "memory_bytes" => intermediate_rep.memory_bytes.push(
                        <i64 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "thread_count" => intermediate_rep.thread_count.push(
                        <i64 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "sampling_period_ms" => intermediate_rep.sampling_period_ms.push(
                        <i64 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "timestamp" => intermediate_rep.timestamp.push(
                        <i64 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    _ => {
                        return std::result::Result::Err(
                            "Unexpected key while parsing ResourceUsage".to_string(),
                        )
                    }
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(ResourceUsage {
            cpu_percent: intermediate_rep
                .cpu_percent
                .into_iter()
                .next()
                .ok_or_else(|| "cpu_percent missing in ResourceUsage".to_string())?,
            memory_bytes: intermediate_rep
                .memory_bytes
                .into_iter()
                .next()
                .ok_or_else(|| "memory_bytes missing in ResourceUsage".to_string())?,
            thread_count: intermediate_rep
                .thread_count
                .into_iter()
                .next()
                .ok_or_else(|| "thread_count missing in ResourceUsage".to_string())?,
            sampling_period_ms: intermediate_rep
                .sampling_period_ms
                .into_iter()
                .next()
                .ok_or_else(|| "sampling_period_ms missing in ResourceUsage".to_string())?,
            timestamp: intermediate_rep
                .timestamp
                .into_iter()
                .next()
                .ok_or_else(|| "timestamp missing in ResourceUsage".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<ResourceUsage> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ResourceUsage>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ResourceUsage>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for ResourceUsage - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<ResourceUsage> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <ResourceUsage as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into ResourceUsage - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ResourceUsage>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ResourceUsage>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ResourceUsage>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ResourceUsage> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ResourceUsage as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ResourceUsage - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for RestartSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            self.working_directory.as_ref().map(|working_directory| {
                [
                    "working_directory".to_string(),
                    working_directory.to_string(),
                ]
                .join(",")
            }),
            // Skipping non-primitive type env in query parameter serialization
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a RestartSession value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
                "Invalid header value for RestartSession - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into RestartSession - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<RestartSession>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<RestartSession>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<RestartSession>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<RestartSession> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <RestartSession as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into RestartSession - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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

    /// The interval in milliseconds at which resource usage is sampled. A value of 0 disables resource usage sampling.
    #[serde(rename = "resource_sample_interval_ms")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_sample_interval_ms: Option<i32>,

    #[serde(rename = "log_level")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<models::ServerConfigurationLogLevel>,
}

impl ServerConfiguration {
    #[allow(clippy::new_without_default)]
    pub fn new() -> ServerConfiguration {
        ServerConfiguration {
            idle_shutdown_hours: None,
            resource_sample_interval_ms: None,
            log_level: None,
        }
    }
}

/// Converts the ServerConfiguration value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ServerConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            self.resource_sample_interval_ms
                .as_ref()
                .map(|resource_sample_interval_ms| {
                    [
                        "resource_sample_interval_ms".to_string(),
                        resource_sample_interval_ms.to_string(),
                    ]
                    .join(",")
                }),
            // Skipping non-primitive type log_level in query parameter serialization
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ServerConfiguration value
/// as specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde deserializer
impl std::str::FromStr for ServerConfiguration {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub idle_shutdown_hours: Vec<i32>,
            pub resource_sample_interval_ms: Vec<i32>,
            pub log_level: Vec<models::ServerConfigurationLogLevel>,
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
                    "resource_sample_interval_ms" => {
                        intermediate_rep.resource_sample_interval_ms.push(
                            <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                        )
                    }
                    #[allow(clippy::redundant_clone)]
                    "log_level" => intermediate_rep.log_level.push(
                        <models::ServerConfigurationLogLevel as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
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
            resource_sample_interval_ms: intermediate_rep
                .resource_sample_interval_ms
                .into_iter()
                .next(),
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
                "Invalid header value for ServerConfiguration - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into ServerConfiguration - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ServerConfiguration>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ServerConfiguration>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ServerConfiguration>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ServerConfiguration> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ServerConfiguration as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ServerConfiguration - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// The current log level
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum ServerConfigurationLogLevel {
    #[serde(rename = "trace")]
    Trace,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "error")]
    Error,
}

impl std::fmt::Display for ServerConfigurationLogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ServerConfigurationLogLevel::Trace => write!(f, "trace"),
            ServerConfigurationLogLevel::Debug => write!(f, "debug"),
            ServerConfigurationLogLevel::Info => write!(f, "info"),
            ServerConfigurationLogLevel::Warn => write!(f, "warn"),
            ServerConfigurationLogLevel::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for ServerConfigurationLogLevel {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "trace" => std::result::Result::Ok(ServerConfigurationLogLevel::Trace),
            "debug" => std::result::Result::Ok(ServerConfigurationLogLevel::Debug),
            "info" => std::result::Result::Ok(ServerConfigurationLogLevel::Info),
            "warn" => std::result::Result::Ok(ServerConfigurationLogLevel::Warn),
            "error" => std::result::Result::Ok(ServerConfigurationLogLevel::Error),
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<ServerConfigurationLogLevel> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<ServerConfigurationLogLevel>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<ServerConfigurationLogLevel>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
             std::result::Result::Ok(value) => std::result::Result::Ok(value),
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Invalid header value for ServerConfigurationLogLevel - value: {hdr_value} is invalid {e}"))
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<ServerConfigurationLogLevel>
{
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
             std::result::Result::Ok(value) => {
                    match <ServerConfigurationLogLevel as std::str::FromStr>::from_str(value) {
                        std::result::Result::Ok(value) => std::result::Result::Ok(header::IntoHeaderValue(value)),
                        std::result::Result::Err(err) => std::result::Result::Err(
                            format!("Unable to convert header value '{value}' into ServerConfigurationLogLevel - {err}"))
                    }
             },
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Unable to convert header: {hdr_value:?} to string: {e}"))
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ServerConfigurationLogLevel>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ServerConfigurationLogLevel>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ServerConfigurationLogLevel>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ServerConfigurationLogLevel> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ServerConfigurationLogLevel as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ServerConfigurationLogLevel - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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

    /// The number of seconds the server has been running
    #[serde(rename = "uptime_seconds")]
    pub uptime_seconds: i32,

    /// The version of the server
    #[serde(rename = "version")]
    pub version: String,

    /// The server's operating system process identifier
    #[serde(rename = "process_id")]
    pub process_id: i32,

    /// An ISO 8601 timestamp of when the server was started
    #[serde(rename = "started")]
    pub started: chrono::DateTime<chrono::Utc>,

    /// A unique identifier generated when the server starts. Clients can compare this against a previously observed value to detect that they are talking to a different server instance (e.g. one that was restarted), and therefore that any persisted bearer token may be stale.
    #[serde(rename = "server_id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
}

impl ServerStatus {
    #[allow(clippy::new_without_default)]
    pub fn new(
        sessions: i32,
        active: i32,
        busy: bool,
        idle_seconds: i32,
        busy_seconds: i32,
        uptime_seconds: i32,
        version: String,
        process_id: i32,
        started: chrono::DateTime<chrono::Utc>,
    ) -> ServerStatus {
        ServerStatus {
            sessions,
            active,
            busy,
            idle_seconds,
            busy_seconds,
            uptime_seconds,
            version,
            process_id,
            started,
            server_id: None,
        }
    }
}

/// Converts the ServerStatus value to the Query Parameters representation (style=form, explode=false)
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for ServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            Some("uptime_seconds".to_string()),
            Some(self.uptime_seconds.to_string()),
            Some("version".to_string()),
            Some(self.version.to_string()),
            Some("process_id".to_string()),
            Some(self.process_id.to_string()),
            // Skipping non-primitive type started in query parameter serialization
            self.server_id
                .as_ref()
                .map(|server_id| ["server_id".to_string(), server_id.to_string()].join(",")),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a ServerStatus value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
            pub uptime_seconds: Vec<i32>,
            pub version: Vec<String>,
            pub process_id: Vec<i32>,
            pub started: Vec<chrono::DateTime<chrono::Utc>>,
            pub server_id: Vec<String>,
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
                    "uptime_seconds" => intermediate_rep.uptime_seconds.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "version" => intermediate_rep.version.push(
                        <String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "process_id" => intermediate_rep.process_id.push(
                        <i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "started" => intermediate_rep.started.push(
                        <chrono::DateTime<chrono::Utc> as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
                    ),
                    #[allow(clippy::redundant_clone)]
                    "server_id" => intermediate_rep.server_id.push(
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
            uptime_seconds: intermediate_rep
                .uptime_seconds
                .into_iter()
                .next()
                .ok_or_else(|| "uptime_seconds missing in ServerStatus".to_string())?,
            version: intermediate_rep
                .version
                .into_iter()
                .next()
                .ok_or_else(|| "version missing in ServerStatus".to_string())?,
            process_id: intermediate_rep
                .process_id
                .into_iter()
                .next()
                .ok_or_else(|| "process_id missing in ServerStatus".to_string())?,
            started: intermediate_rep
                .started
                .into_iter()
                .next()
                .ok_or_else(|| "started missing in ServerStatus".to_string())?,
            server_id: intermediate_rep.server_id.into_iter().next(),
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
                "Invalid header value for ServerStatus - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into ServerStatus - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<ServerStatus>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<ServerStatus>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<ServerStatus>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<ServerStatus> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <ServerStatus as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into ServerStatus - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for SessionList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            Some("total".to_string()),
            Some(self.total.to_string()),
            // Skipping non-primitive type sessions in query parameter serialization
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a SessionList value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
                "Invalid header value for SessionList - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into SessionList - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<SessionList>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<SessionList>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<SessionList>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<SessionList> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <SessionList as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into SessionList - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// The mode in which the session is running
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum SessionMode {
    #[serde(rename = "console")]
    Console,
    #[serde(rename = "notebook")]
    Notebook,
    #[serde(rename = "background")]
    Background,
}

impl std::fmt::Display for SessionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            SessionMode::Console => write!(f, "console"),
            SessionMode::Notebook => write!(f, "notebook"),
            SessionMode::Background => write!(f, "background"),
        }
    }
}

impl std::str::FromStr for SessionMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "console" => std::result::Result::Ok(SessionMode::Console),
            "notebook" => std::result::Result::Ok(SessionMode::Notebook),
            "background" => std::result::Result::Ok(SessionMode::Background),
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<SessionMode> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<SessionMode>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<SessionMode>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for SessionMode - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<SessionMode> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <SessionMode as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into SessionMode - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<SessionMode>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<SessionMode>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<SessionMode>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<SessionMode> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <SessionMode as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into SessionMode - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}

/// The kernel's startup environment: 'none' for normal startup, 'shell' for a login shell, 'command' for a preflight command, 'script' to run a script. Only relevant on POSIX-like systems.
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum StartupEnvironment {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "shell")]
    Shell,
    #[serde(rename = "command")]
    Command,
    #[serde(rename = "script")]
    Script,
}

impl std::fmt::Display for StartupEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            StartupEnvironment::None => write!(f, "none"),
            StartupEnvironment::Shell => write!(f, "shell"),
            StartupEnvironment::Command => write!(f, "command"),
            StartupEnvironment::Script => write!(f, "script"),
        }
    }
}

impl std::str::FromStr for StartupEnvironment {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "none" => std::result::Result::Ok(StartupEnvironment::None),
            "shell" => std::result::Result::Ok(StartupEnvironment::Shell),
            "command" => std::result::Result::Ok(StartupEnvironment::Command),
            "script" => std::result::Result::Ok(StartupEnvironment::Script),
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<StartupEnvironment> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<StartupEnvironment>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<StartupEnvironment>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for StartupEnvironment - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<StartupEnvironment>
{
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <StartupEnvironment as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into StartupEnvironment - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<StartupEnvironment>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<StartupEnvironment>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<StartupEnvironment>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<StartupEnvironment> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <StartupEnvironment as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into StartupEnvironment - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            self.exit_code
                .as_ref()
                .map(|exit_code| ["exit_code".to_string(), exit_code.to_string()].join(",")),
            self.output
                .as_ref()
                .map(|output| ["output".to_string(), output.to_string()].join(",")),
            // Skipping non-primitive type error in query parameter serialization
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a StartupError value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
                "Invalid header value for StartupError - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into StartupError - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<StartupError>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<StartupError>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<StartupError>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<StartupError> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <StartupError as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into StartupError - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
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
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<Status> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<Status>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<Status>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for Status - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<Status> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <Status as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into Status - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<Status>>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<Status>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<Vec<Status>> {
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<Status> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <Status as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into Status - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
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
/// specified in <https://swagger.io/docs/specification/serialization/>
/// Should be implemented in a serde serializer
impl std::fmt::Display for VarAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<Option<String>> = vec![
            // Skipping non-primitive type action in query parameter serialization
            Some("name".to_string()),
            Some(self.name.to_string()),
            Some("value".to_string()),
            Some(self.value.to_string()),
        ];

        write!(
            f,
            "{}",
            params.into_iter().flatten().collect::<Vec<_>>().join(",")
        )
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a VarAction value
/// as specified in <https://swagger.io/docs/specification/serialization/>
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
                "Invalid header value for VarAction - value: {hdr_value} is invalid {e}"
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
                        "Unable to convert header value '{value}' into VarAction - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<VarAction>>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<VarAction>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<Vec<VarAction>> {
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<VarAction> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <VarAction as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into VarAction - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
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
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
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
            _ => std::result::Result::Err(format!("Value not valid: {s}")),
        }
    }
}

// Methods for converting between header::IntoHeaderValue<VarActionType> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<VarActionType>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(
        hdr_value: header::IntoHeaderValue<VarActionType>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
            std::result::Result::Ok(value) => std::result::Result::Ok(value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Invalid header value for VarActionType - value: {hdr_value} is invalid {e}"
            )),
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<VarActionType> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
            std::result::Result::Ok(value) => {
                match <VarActionType as std::str::FromStr>::from_str(value) {
                    std::result::Result::Ok(value) => {
                        std::result::Result::Ok(header::IntoHeaderValue(value))
                    }
                    std::result::Result::Err(err) => std::result::Result::Err(format!(
                        "Unable to convert header value '{value}' into VarActionType - {err}"
                    )),
                }
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert header: {hdr_value:?} to string: {e}"
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<header::IntoHeaderValue<Vec<VarActionType>>>
    for hyper::header::HeaderValue
{
    type Error = String;

    fn try_from(
        hdr_values: header::IntoHeaderValue<Vec<VarActionType>>,
    ) -> std::result::Result<Self, Self::Error> {
        let hdr_values: Vec<String> = hdr_values
            .0
            .into_iter()
            .map(|hdr_value| hdr_value.to_string())
            .collect();

        match hyper::header::HeaderValue::from_str(&hdr_values.join(", ")) {
            std::result::Result::Ok(hdr_value) => std::result::Result::Ok(hdr_value),
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to convert {hdr_values:?} into a header - {e}",
            )),
        }
    }
}

#[cfg(feature = "server")]
impl std::convert::TryFrom<hyper::header::HeaderValue>
    for header::IntoHeaderValue<Vec<VarActionType>>
{
    type Error = String;

    fn try_from(hdr_values: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_values.to_str() {
            std::result::Result::Ok(hdr_values) => {
                let hdr_values : std::vec::Vec<VarActionType> = hdr_values
                .split(',')
                .filter_map(|hdr_value| match hdr_value.trim() {
                    "" => std::option::Option::None,
                    hdr_value => std::option::Option::Some({
                        match <VarActionType as std::str::FromStr>::from_str(hdr_value) {
                            std::result::Result::Ok(value) => std::result::Result::Ok(value),
                            std::result::Result::Err(err) => std::result::Result::Err(
                                format!("Unable to convert header value '{hdr_value}' into VarActionType - {err}"))
                        }
                    })
                }).collect::<std::result::Result<std::vec::Vec<_>, String>>()?;

                std::result::Result::Ok(header::IntoHeaderValue(hdr_values))
            }
            std::result::Result::Err(e) => std::result::Result::Err(format!(
                "Unable to parse header: {hdr_values:?} as a string - {e}"
            )),
        }
    }
}
