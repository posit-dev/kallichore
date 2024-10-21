/*
 * kernel_info.rs
 *
 * Copyright (C) 2024 Posit Software, PBC. All rights reserved.
 *
 */

use serde::Deserialize;
use serde::Serialize;

/// Represents a help link in a Jupyter message
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HelpLink {
    /// The text to display for the link
    pub text: String,

    /// The location (URL) of the help link
    pub url: String,
}

/// Represents information about the language that the kernel implements
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LanguageInfo {
    /// The name of the programming language the kernel implements
    pub name: String,

    /// The version of the language
    pub version: String,

    /// The MIME type for script files in the language
    pub mimetype: String,

    /// The file extension for script files in the language
    pub file_extension: String,

    /// Pygments lexer (for highlighting), if different than `name`
    pub pygments_lexer: Option<String>,

    /// Codemirror mode (for editing), if different than `name`
    pub codemirror_mode: Option<String>,

    /// Nbconvert exporter, if not the default 'script' exporter
    pub nbconvert_exporter: Option<String>,

    /// Posit extension
    pub positron: Option<LanguageInfoPositron>,
}

/// Represents Positron-specific language information stored in `LanguageInfo`
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LanguageInfoPositron {
    /// Initial input prompt
    pub input_prompt: Option<String>,

    /// Initial continuation prompt
    pub continuation_prompt: Option<String>,
}

/// Represents a reply to a kernel_info_request
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KernelInfoReply {
    /// The execution status ("ok" or "error")
    pub status: String,

    /// Version of messaging protocol
    pub protocol_version: String,

    /// Information about the language the kernel supports
    pub language_info: LanguageInfo,

    /// A startup banner
    pub banner: String,

    /// Whether debugging is supported
    pub debugger: bool,

    /// A list of help links
    pub help_links: Vec<HelpLink>,
}
