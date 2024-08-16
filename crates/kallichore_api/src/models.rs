#![allow(unused_qualifications)]

use validator::Validate;

use crate::models;
#[cfg(any(feature = "client", feature = "server"))]
use crate::header;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct Error {
    #[serde(rename = "code")]
    pub code: String,

    #[serde(rename = "message")]
    pub message: String,

    #[serde(rename = "details")]
    #[serde(skip_serializing_if="Option::is_none")]
    pub details: Option<String>,

}


impl Error {
    #[allow(clippy::new_without_default)]
    pub fn new(code: String, message: String, ) -> Error {
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


            self.details.as_ref().map(|details| {
                [
                    "details".to_string(),
                    details.to_string(),
                ].join(",")
            }),

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
                None => return std::result::Result::Err("Missing value while parsing Error".to_string())
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "code" => intermediate_rep.code.push(<String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    #[allow(clippy::redundant_clone)]
                    "message" => intermediate_rep.message.push(<String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    #[allow(clippy::redundant_clone)]
                    "details" => intermediate_rep.details.push(<String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    _ => return std::result::Result::Err("Unexpected key while parsing Error".to_string())
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(Error {
            code: intermediate_rep.code.into_iter().next().ok_or_else(|| "code missing in Error".to_string())?,
            message: intermediate_rep.message.into_iter().next().ok_or_else(|| "message missing in Error".to_string())?,
            details: intermediate_rep.details.into_iter().next(),
        })
    }
}

// Methods for converting between header::IntoHeaderValue<Error> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<Error>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(hdr_value: header::IntoHeaderValue<Error>) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
             std::result::Result::Ok(value) => std::result::Result::Ok(value),
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Invalid header value for Error - value: {} is invalid {}",
                     hdr_value, e))
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<Error> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
             std::result::Result::Ok(value) => {
                    match <Error as std::str::FromStr>::from_str(value) {
                        std::result::Result::Ok(value) => std::result::Result::Ok(header::IntoHeaderValue(value)),
                        std::result::Result::Err(err) => std::result::Result::Err(
                            format!("Unable to convert header value '{}' into Error - {}",
                                value, err))
                    }
             },
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Unable to convert header: {:?} to string: {}",
                     hdr_value, e))
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
    pub fn new(session_id: String, ) -> NewSession200Response {
        NewSession200Response {
            session_id,
        }
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
                None => return std::result::Result::Err("Missing value while parsing NewSession200Response".to_string())
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "session_id" => intermediate_rep.session_id.push(<String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    _ => return std::result::Result::Err("Unexpected key while parsing NewSession200Response".to_string())
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(NewSession200Response {
            session_id: intermediate_rep.session_id.into_iter().next().ok_or_else(|| "session_id missing in NewSession200Response".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<NewSession200Response> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<NewSession200Response>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(hdr_value: header::IntoHeaderValue<NewSession200Response>) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
             std::result::Result::Ok(value) => std::result::Result::Ok(value),
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Invalid header value for NewSession200Response - value: {} is invalid {}",
                     hdr_value, e))
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<NewSession200Response> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
             std::result::Result::Ok(value) => {
                    match <NewSession200Response as std::str::FromStr>::from_str(value) {
                        std::result::Result::Ok(value) => std::result::Result::Ok(header::IntoHeaderValue(value)),
                        std::result::Result::Err(err) => std::result::Result::Err(
                            format!("Unable to convert header value '{}' into NewSession200Response - {}",
                                value, err))
                    }
             },
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Unable to convert header: {:?} to string: {}",
                     hdr_value, e))
        }
    }
}


#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct Session {
    /// A unique identifier for the session
    #[serde(rename = "session_id")]
    pub session_id: String,

    /// The program and command-line parameters for the session
    #[serde(rename = "argv")]
    pub argv: Vec<String>,

    /// The working directory in which to start the session.
    #[serde(rename = "working_directory")]
    pub working_directory: String,

}


impl Session {
    #[allow(clippy::new_without_default)]
    pub fn new(session_id: String, argv: Vec<String>, working_directory: String, ) -> Session {
        Session {
            session_id,
            argv,
            working_directory,
        }
    }
}

/// Converts the Session value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for Session {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![

            Some("session_id".to_string()),
            Some(self.session_id.to_string()),


            Some("argv".to_string()),
            Some(self.argv.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")),


            Some("working_directory".to_string()),
            Some(self.working_directory.to_string()),

        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a Session value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for Session {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub session_id: Vec<String>,
            pub argv: Vec<Vec<String>>,
            pub working_directory: Vec<String>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => return std::result::Result::Err("Missing value while parsing Session".to_string())
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "session_id" => intermediate_rep.session_id.push(<String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    "argv" => return std::result::Result::Err("Parsing a container in this style is not supported in Session".to_string()),
                    #[allow(clippy::redundant_clone)]
                    "working_directory" => intermediate_rep.working_directory.push(<String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    _ => return std::result::Result::Err("Unexpected key while parsing Session".to_string())
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(Session {
            session_id: intermediate_rep.session_id.into_iter().next().ok_or_else(|| "session_id missing in Session".to_string())?,
            argv: intermediate_rep.argv.into_iter().next().ok_or_else(|| "argv missing in Session".to_string())?,
            working_directory: intermediate_rep.working_directory.into_iter().next().ok_or_else(|| "working_directory missing in Session".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<Session> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<Session>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(hdr_value: header::IntoHeaderValue<Session>) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
             std::result::Result::Ok(value) => std::result::Result::Ok(value),
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Invalid header value for Session - value: {} is invalid {}",
                     hdr_value, e))
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<Session> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
             std::result::Result::Ok(value) => {
                    match <Session as std::str::FromStr>::from_str(value) {
                        std::result::Result::Ok(value) => std::result::Result::Ok(header::IntoHeaderValue(value)),
                        std::result::Result::Err(err) => std::result::Result::Err(
                            format!("Unable to convert header value '{}' into Session - {}",
                                value, err))
                    }
             },
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Unable to convert header: {:?} to string: {}",
                     hdr_value, e))
        }
    }
}


#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct SessionList {
    #[serde(rename = "total")]
    pub total: i32,

    #[serde(rename = "sessions")]
    pub sessions: Vec<models::SessionListSessionsInner>,

}


impl SessionList {
    #[allow(clippy::new_without_default)]
    pub fn new(total: i32, sessions: Vec<models::SessionListSessionsInner>, ) -> SessionList {
        SessionList {
            total,
            sessions,
        }
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
            pub sessions: Vec<Vec<models::SessionListSessionsInner>>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => return std::result::Result::Err("Missing value while parsing SessionList".to_string())
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "total" => intermediate_rep.total.push(<i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    "sessions" => return std::result::Result::Err("Parsing a container in this style is not supported in SessionList".to_string()),
                    _ => return std::result::Result::Err("Unexpected key while parsing SessionList".to_string())
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(SessionList {
            total: intermediate_rep.total.into_iter().next().ok_or_else(|| "total missing in SessionList".to_string())?,
            sessions: intermediate_rep.sessions.into_iter().next().ok_or_else(|| "sessions missing in SessionList".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<SessionList> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<SessionList>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(hdr_value: header::IntoHeaderValue<SessionList>) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
             std::result::Result::Ok(value) => std::result::Result::Ok(value),
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Invalid header value for SessionList - value: {} is invalid {}",
                     hdr_value, e))
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
                        std::result::Result::Ok(value) => std::result::Result::Ok(header::IntoHeaderValue(value)),
                        std::result::Result::Err(err) => std::result::Result::Err(
                            format!("Unable to convert header value '{}' into SessionList - {}",
                                value, err))
                    }
             },
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Unable to convert header: {:?} to string: {}",
                     hdr_value, e))
        }
    }
}


#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, validator::Validate)]
#[cfg_attr(feature = "conversion", derive(frunk::LabelledGeneric))]
pub struct SessionListSessionsInner {
    /// A unique identifier for the session
    #[serde(rename = "session_id")]
    pub session_id: String,

    /// The program and command-line parameters for the session
    #[serde(rename = "argv")]
    pub argv: Vec<String>,

    /// The underlying process ID of the session, if the session is running.
    #[serde(rename = "process_id")]
    #[serde(skip_serializing_if="Option::is_none")]
    pub process_id: Option<i32>,

    #[serde(rename = "status")]
    pub status: models::Status,

}


impl SessionListSessionsInner {
    #[allow(clippy::new_without_default)]
    pub fn new(session_id: String, argv: Vec<String>, status: models::Status, ) -> SessionListSessionsInner {
        SessionListSessionsInner {
            session_id,
            argv,
            process_id: None,
            status,
        }
    }
}

/// Converts the SessionListSessionsInner value to the Query Parameters representation (style=form, explode=false)
/// specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde serializer
impl std::string::ToString for SessionListSessionsInner {
    fn to_string(&self) -> String {
        let params: Vec<Option<String>> = vec![

            Some("session_id".to_string()),
            Some(self.session_id.to_string()),


            Some("argv".to_string()),
            Some(self.argv.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")),


            self.process_id.as_ref().map(|process_id| {
                [
                    "process_id".to_string(),
                    process_id.to_string(),
                ].join(",")
            }),

            // Skipping status in query parameter serialization

        ];

        params.into_iter().flatten().collect::<Vec<_>>().join(",")
    }
}

/// Converts Query Parameters representation (style=form, explode=false) to a SessionListSessionsInner value
/// as specified in https://swagger.io/docs/specification/serialization/
/// Should be implemented in a serde deserializer
impl std::str::FromStr for SessionListSessionsInner {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        /// An intermediate representation of the struct to use for parsing.
        #[derive(Default)]
        #[allow(dead_code)]
        struct IntermediateRep {
            pub session_id: Vec<String>,
            pub argv: Vec<Vec<String>>,
            pub process_id: Vec<i32>,
            pub status: Vec<models::Status>,
        }

        let mut intermediate_rep = IntermediateRep::default();

        // Parse into intermediate representation
        let mut string_iter = s.split(',');
        let mut key_result = string_iter.next();

        while key_result.is_some() {
            let val = match string_iter.next() {
                Some(x) => x,
                None => return std::result::Result::Err("Missing value while parsing SessionListSessionsInner".to_string())
            };

            if let Some(key) = key_result {
                #[allow(clippy::match_single_binding)]
                match key {
                    #[allow(clippy::redundant_clone)]
                    "session_id" => intermediate_rep.session_id.push(<String as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    "argv" => return std::result::Result::Err("Parsing a container in this style is not supported in SessionListSessionsInner".to_string()),
                    #[allow(clippy::redundant_clone)]
                    "process_id" => intermediate_rep.process_id.push(<i32 as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    #[allow(clippy::redundant_clone)]
                    "status" => intermediate_rep.status.push(<models::Status as std::str::FromStr>::from_str(val).map_err(|x| x.to_string())?),
                    _ => return std::result::Result::Err("Unexpected key while parsing SessionListSessionsInner".to_string())
                }
            }

            // Get the next key
            key_result = string_iter.next();
        }

        // Use the intermediate representation to return the struct
        std::result::Result::Ok(SessionListSessionsInner {
            session_id: intermediate_rep.session_id.into_iter().next().ok_or_else(|| "session_id missing in SessionListSessionsInner".to_string())?,
            argv: intermediate_rep.argv.into_iter().next().ok_or_else(|| "argv missing in SessionListSessionsInner".to_string())?,
            process_id: intermediate_rep.process_id.into_iter().next(),
            status: intermediate_rep.status.into_iter().next().ok_or_else(|| "status missing in SessionListSessionsInner".to_string())?,
        })
    }
}

// Methods for converting between header::IntoHeaderValue<SessionListSessionsInner> and hyper::header::HeaderValue

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<header::IntoHeaderValue<SessionListSessionsInner>> for hyper::header::HeaderValue {
    type Error = String;

    fn try_from(hdr_value: header::IntoHeaderValue<SessionListSessionsInner>) -> std::result::Result<Self, Self::Error> {
        let hdr_value = hdr_value.to_string();
        match hyper::header::HeaderValue::from_str(&hdr_value) {
             std::result::Result::Ok(value) => std::result::Result::Ok(value),
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Invalid header value for SessionListSessionsInner - value: {} is invalid {}",
                     hdr_value, e))
        }
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl std::convert::TryFrom<hyper::header::HeaderValue> for header::IntoHeaderValue<SessionListSessionsInner> {
    type Error = String;

    fn try_from(hdr_value: hyper::header::HeaderValue) -> std::result::Result<Self, Self::Error> {
        match hdr_value.to_str() {
             std::result::Result::Ok(value) => {
                    match <SessionListSessionsInner as std::str::FromStr>::from_str(value) {
                        std::result::Result::Ok(value) => std::result::Result::Ok(header::IntoHeaderValue(value)),
                        std::result::Result::Err(err) => std::result::Result::Err(
                            format!("Unable to convert header value '{}' into SessionListSessionsInner - {}",
                                value, err))
                    }
             },
             std::result::Result::Err(e) => std::result::Result::Err(
                 format!("Unable to convert header: {:?} to string: {}",
                     hdr_value, e))
        }
    }
}


/// The status of the session
/// Enumeration of values.
/// Since this enum's variants do not hold data, we can easily define them as `#[repr(C)]`
/// which helps with FFI.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "conversion", derive(frunk_enum_derive::LabelledGenericEnum))]
pub enum Status {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "exited")]
    Exited,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Status::Idle => write!(f, "idle"),
            Status::Starting => write!(f, "starting"),
            Status::Exited => write!(f, "exited"),
        }
    }
}

impl std::str::FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "idle" => std::result::Result::Ok(Status::Idle),
            "starting" => std::result::Result::Ok(Status::Starting),
            "exited" => std::result::Result::Ok(Status::Exited),
            _ => std::result::Result::Err(format!("Value not valid: {}", s)),
        }
    }
}
