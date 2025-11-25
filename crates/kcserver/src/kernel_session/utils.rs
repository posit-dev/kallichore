//
// utils.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Utility functions for kernel session management.

use rand::Rng;
use std::iter;

/// Escape a string for use in a shell command.
///
/// This function escapes a string so that it can be safely used as an argument
/// to a shell command. It uses single quotes to wrap the string, which is the
/// safest option in most Unix shells. If the string contains single quotes,
/// they are escaped by replacing them with the sequence '\'' (close quote,
/// escaped quote, open quote).
///
/// # Arguments
///
/// * `s` - The string to escape
///
/// # Returns
///
/// The escaped string
#[cfg(not(target_os = "windows"))]
pub fn escape_for_shell(s: &str) -> String {
    // If the string is empty, return ''
    if s.is_empty() {
        return "''".to_string();
    }

    // If the string doesn't contain any special characters,
    // we can return it as-is
    if !s.chars().any(|c| "\\\"'`${}()*?! \t\n;&|<>[]".contains(c)) {
        return s.to_string();
    }

    // Otherwise, wrap in single quotes and escape any internal single quotes
    let mut result = String::with_capacity(s.len() + 2);
    result.push('\'');

    // Replace any single quotes in the input with '\''
    for part in s.split('\'') {
        if !result.ends_with('\'') {
            result.push('\'');
        }

        result.push_str(part);

        if !part.is_empty() {
            result.push('\'');
        }

        // Add the escaped single quote sequence if this isn't the last part
        if part.len() < s.len() {
            result.push_str("\\'");
        }
    }

    // Ensure the string ends with a quote
    if !result.ends_with('\'') {
        result.push('\'');
    }

    result
}

/// Generate a unique message ID for Jupyter messages.
///
/// # Returns
///
/// A random hexadecimal string of 10 characters.
pub fn make_message_id() -> String {
    let mut rng = rand::thread_rng();
    iter::repeat_with(|| format!("{:x}", rng.gen_range(0..16)))
        .take(10)
        .collect()
}

/// Strip startup markers from output to prevent them from leaking to users.
///
/// The markers KALLICHORE_STARTUP_BEGIN and KALLICHORE_STARTUP_SUCCESS are used
/// internally to determine what failed during startup, but should never be visible
/// in user-facing output or error messages.
pub fn strip_startup_markers(output: &str) -> String {
    output
        .lines()
        .filter(|line| {
            !line.contains("KALLICHORE_STARTUP_BEGIN")
                && !line.contains("KALLICHORE_STARTUP_SUCCESS")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
