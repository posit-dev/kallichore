#![allow(missing_docs, unused_variables, trivial_casts)]

use clap::{Arg, Command};
#[allow(unused_imports)]
use futures::{future, stream, Stream};
#[allow(unused_imports)]
use kallichore_api::{
    models, AdoptSessionResponse, Api, ApiNoContext, ChannelsUpgradeResponse, Claims, Client,
    ClientHeartbeatResponse, ConnectionInfoResponse, ContextWrapperExt, DeleteSessionResponse,
    GetServerConfigurationResponse, GetSessionResponse, InterruptSessionResponse,
    KillSessionResponse, ListSessionsResponse, NewSessionResponse, RestartSessionResponse,
    ServerStatusResponse, SetServerConfigurationResponse, ShutdownServerResponse,
    StartSessionResponse,
};

// NOTE: Set environment variable RUST_LOG to the name of the executable (or "cargo run") to activate console logging for all loglevels.
//     See https://docs.rs/env_logger/latest/env_logger/  for more details

#[allow(unused_imports)]
use log::info;

// swagger::Has may be unused if there are no examples
#[allow(unused_imports)]
use swagger::{AuthData, ContextBuilder, EmptyContext, Has, Push, XSpanIdString};

type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

mod client_auth;
use client_auth::build_token;

// rt may be unused if there are no examples
#[allow(unused_mut)]
fn main() {
    env_logger::init();

    let matches = Command::new("client")
        .arg(
            Arg::new("operation")
                .help("Sets the operation to run")
                .value_parser([
                    "ClientHeartbeat",
                    "GetServerConfiguration",
                    "ListSessions",
                    "NewSession",
                    "ServerStatus",
                    "SetServerConfiguration",
                    "ShutdownServer",
                    "AdoptSession",
                    "ChannelsUpgrade",
                    "ConnectionInfo",
                    "DeleteSession",
                    "GetSession",
                    "InterruptSession",
                    "KillSession",
                    "RestartSession",
                    "StartSession",
                ])
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("http")
                .long("http")
                .help("Whether to use HTTPS or not"),
        )
        .arg(
            Arg::new("host")
                .long("host")
                .default_value("localhost")
                .help("Hostname to contact"),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .default_value("8080")
                .help("Port to contact"),
        )
        .get_matches();

    // Create Bearer-token with a fixed key (secret) for test purposes.
    // In a real (production) system this Bearer token should be obtained via an external Identity/Authentication-server
    // Ensure that you set the correct algorithm and encodingkey that matches what is used on the server side.
    // See https://github.com/Keats/jsonwebtoken for more information
    let auth_token = build_token(
        Claims {
            sub: "tester@acme.com".to_owned(),
            company: "ACME".to_owned(),
            iss: "my_identity_provider".to_owned(),
            // added a very long expiry time
            aud: "org.acme.Resource_Server".to_string(),
            exp: 10000000000,
            // In this example code all available Scopes are added, so the current Bearer Token gets fully authorization.
            scopes: "".to_owned(),
        },
        b"secret",
    )
    .unwrap();

    let auth_data = if !auth_token.is_empty() {
        Some(AuthData::Bearer(auth_token))
    } else {
        // No Bearer-token available, so return None
        None
    };

    let is_https = matches.contains_id("http");
    let base_url = format!(
        "{}://{}:{}",
        if is_https { "http" } else { "http" },
        matches.get_one::<String>("host").unwrap(),
        matches.get_one::<u16>("port").unwrap()
    );

    let context: ClientContext = swagger::make_context!(
        ContextBuilder,
        EmptyContext,
        auth_data,
        XSpanIdString::default()
    );

    let mut client: Box<dyn ApiNoContext<ClientContext>> = if is_https {
        // Using Simple HTTPS
        let client =
            Box::new(Client::try_new_http(&base_url).expect("Failed to create HTTP client"));
        Box::new(client.with_context(context))
    } else {
        // Using HTTP
        let client =
            Box::new(Client::try_new_http(&base_url).expect("Failed to create HTTP client"));
        Box::new(client.with_context(context))
    };

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    match matches.get_one::<String>("operation").map(String::as_str) {
        /* Disabled because there's no example.
        Some("ClientHeartbeat") => {
            let result = rt.block_on(client.client_heartbeat(
                  ???
            ));
            info!("{:?} (X-Span-ID: {:?})", result, (client.context() as &dyn Has<XSpanIdString>).get().clone());
        },
        */
        Some("GetServerConfiguration") => {
            let result = rt.block_on(client.get_server_configuration());
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("ListSessions") => {
            let result = rt.block_on(client.list_sessions());
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        /* Disabled because there's no example.
        Some("NewSession") => {
            let result = rt.block_on(client.new_session(
                  ???
            ));
            info!("{:?} (X-Span-ID: {:?})", result, (client.context() as &dyn Has<XSpanIdString>).get().clone());
        },
        */
        Some("ServerStatus") => {
            let result = rt.block_on(client.server_status());
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        /* Disabled because there's no example.
        Some("SetServerConfiguration") => {
            let result = rt.block_on(client.set_server_configuration(
                  ???
            ));
            info!("{:?} (X-Span-ID: {:?})", result, (client.context() as &dyn Has<XSpanIdString>).get().clone());
        },
        */
        Some("ShutdownServer") => {
            let result = rt.block_on(client.shutdown_server());
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        /* Disabled because there's no example.
        Some("AdoptSession") => {
            let result = rt.block_on(client.adopt_session(
                  "session_id_example".to_string(),
                  ???
            ));
            info!("{:?} (X-Span-ID: {:?})", result, (client.context() as &dyn Has<XSpanIdString>).get().clone());
        },
        */
        Some("ChannelsUpgrade") => {
            let result = rt.block_on(client.channels_upgrade("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("ConnectionInfo") => {
            let result = rt.block_on(client.connection_info("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("DeleteSession") => {
            let result = rt.block_on(client.delete_session("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("GetSession") => {
            let result = rt.block_on(client.get_session("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("InterruptSession") => {
            let result = rt.block_on(client.interrupt_session("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("KillSession") => {
            let result = rt.block_on(client.kill_session("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("RestartSession") => {
            let result =
                rt.block_on(client.restart_session("session_id_example".to_string(), None));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("StartSession") => {
            let result = rt.block_on(client.start_session("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        _ => {
            panic!("Invalid operation provided")
        }
    }
}
