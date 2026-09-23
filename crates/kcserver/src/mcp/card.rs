//
// card.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! The server card each workspace's endpoint publishes.
//!
//! [SEP-2127](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2127)
//! reserves `GET <streamable-http-url>/server-card` for a static document
//! describing a server -- its identity, its transport, and the protocol
//! versions it speaks -- so that a client can see what it is about to talk to
//! before it opens a protocol session. Agents are handed Positron's endpoint as
//! a bare URL in an environment variable, so a harness that probes before
//! connecting has nowhere else to learn any of that.
//!
//! The card is served without a token, which is what makes it useful, so it
//! carries nothing the reader does not already have: the only route to it names
//! a workspace ID, and the ID is the one the requester's URL already contains.
//! The workspace's token appears only as a header the reader is told it has to
//! supply.

use rmcp::model::ProtocolVersion;
use serde_json::{json, Value};

use super::handler::{SERVER_DESCRIPTION, SERVER_NAME, SERVER_TITLE};
use super::listener::{endpoint_url, session_endpoint_url};

/// The schema a v1 card declares itself against.
const CARD_SCHEMA: &str =
    "https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json";

/// The media type of a server card, which a client asks for and we answer with.
pub const CARD_MEDIA_TYPE: &str = "application/mcp-server-card+json";

/// The path, relative to a workspace's endpoint, the card is served from.
pub const CARD_PATH_SUFFIX: &str = "/server-card";

/// Positron's home, for a reader deciding whether it wants to connect at all.
const WEBSITE_URL: &str = "https://positron.posit.co";

/// Build the card describing one workspace's endpoint.
///
/// A card's claims are advisory -- a client reconciles them against what it
/// sees once connected -- but they must not contradict the live server, so the
/// identity here is the same one `initialize` reports and the protocol versions
/// are the ones the handler will actually negotiate.
pub fn server_card(
    port: u16,
    workspace_id: &str,
    display_name: &str,
    caller: Option<&str>,
) -> Value {
    // A card names the endpoint it was read from, so a kernel's client is told
    // the URL that identifies it rather than the workspace's plain one.
    let url = match caller {
        Some(session_id) => session_endpoint_url(port, workspace_id, session_id),
        None => endpoint_url(port, workspace_id),
    };
    json!({
        "$schema": CARD_SCHEMA,
        // Reverse-DNS, as the card format requires; `serverInfo` reports the
        // bare name that this namespace qualifies.
        "name": format!("co.posit/{}", SERVER_NAME),
        "version": env!("CARGO_PKG_VERSION"),
        "title": SERVER_TITLE,
        "description": SERVER_DESCRIPTION,
        "websiteUrl": WEBSITE_URL,
        "remotes": [{
            "type": "streamable-http",
            "url": url,
            "headers": [{
                "name": "Authorization",
                "description": "Bearer token for this workspace, which Positron publishes to its terminals as POSITRON_MCP_TOKEN",
                "isRequired": true,
                "isSecret": true,
            }],
            // The handler does not narrow the SDK's list, so everything it
            // knows how to speak is on offer.
            "supportedProtocolVersions": ProtocolVersion::KNOWN_VERSIONS
                .iter()
                .map(ProtocolVersion::as_str)
                .collect::<Vec<_>>(),
        }],
        // Which of the workspaces sharing this listener the endpoint belongs
        // to, so a reader that found the URL in an environment it inherited can
        // tell whose sessions it is about to reach. Namespaced and advisory: a
        // client that does not know the key ignores it and still connects.
        "_meta": {
            "co.posit.positron/workspace": {
                "id": workspace_id,
                "displayName": display_name,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_the_workspace_endpoint() {
        assert_eq!(
            server_card(39000, "my-project-a1b2c3", "my-project", None),
            json!({
                "$schema": CARD_SCHEMA,
                "name": "co.posit/positron",
                "version": env!("CARGO_PKG_VERSION"),
                "title": "Positron",
                "description": SERVER_DESCRIPTION,
                "websiteUrl": WEBSITE_URL,
                "remotes": [{
                    "type": "streamable-http",
                    "url": "http://127.0.0.1:39000/mcp/w/my-project-a1b2c3",
                    "headers": [{
                        "name": "Authorization",
                        "description": "Bearer token for this workspace, which Positron publishes to its terminals as POSITRON_MCP_TOKEN",
                        "isRequired": true,
                        "isSecret": true,
                    }],
                    "supportedProtocolVersions": [
                        "2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25", "2026-07-28",
                    ],
                }],
                "_meta": {
                    "co.posit.positron/workspace": {
                        "id": "my-project-a1b2c3",
                        "displayName": "my-project",
                    },
                },
            })
        );
    }

    #[test]
    fn names_the_endpoint_it_was_read_from() {
        let card = server_card(39000, "my-project-a1b2c3", "my-project", Some("python-1"));
        assert_eq!(
            card["remotes"][0]["url"],
            json!("http://127.0.0.1:39000/mcp/w/my-project-a1b2c3/s/python-1")
        );
    }
}
