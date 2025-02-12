//
// client_session_unix.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//
use std::os::unix::net::UnixStream;

#[allow(dead_code)]
pub struct UnixClientSession {
    stream: UnixStream,
}
