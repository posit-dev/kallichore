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

    /// Environment variables to set for the session
    #[serde(rename = "env")]
    pub env: std::collections::HashMap<String, String>,

    #[serde(rename = "interrupt_mode")]
    pub interrupt_mode: models::InterruptMode,
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
        env: std::collections::HashMap<String, String>,
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
            interrupt_mode,
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

            // Skipping interrupt_mode in query parameter serialization
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
            pub env: Vec<std::collections::HashMap<String, String>>,
            pub interrupt_mode: Vec<models::InterruptMode>,
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
                    "interrupt_mode" => intermediate_rep.interrupt_mode.push(
                        <models::InterruptMode as std::str::FromStr>::from_str(val)
                            .map_err(|x| x.to_string())?,
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
            interrupt_mode: intermediate_rep
                .interrupt_mode
                .into_iter()
                .next()
                .ok_or_else(|| "interrupt_mode missing in NewSession".to_string())?,
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
