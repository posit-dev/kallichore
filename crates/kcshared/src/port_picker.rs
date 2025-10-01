//
// port_picker.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6, TcpListener, ToSocketAddrs};

// Try to bind to a socket using TCP
fn test_bind_tcp<A: ToSocketAddrs>(addr: A) -> Option<u16> {
    Some(TcpListener::bind(addr).ok()?.local_addr().ok()?.port())
}

/// Asks the OS for a free port.
///
/// Tries to bind to both IPv4 and IPv6 addresses with port 0, which lets the OS
/// pick an available port.  Returns the port number if successful, or None if
/// no port is available.
fn ask_free_tcp_port() -> Option<u16> {
    let ipv4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    let ipv6 = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0);

    test_bind_tcp(ipv6).or_else(|| test_bind_tcp(ipv4))
}

/// Picks an available TCP port
pub fn pick_unused_tcp_port() -> Option<u16> {
    // Try up to 5 times to find a free port
    for _ in 0..5 {
        if let Some(port) = ask_free_tcp_port() {
            return Some(port);
        }
    }
    None
}
