#![allow(missing_docs, unused_variables, trivial_casts)]

use clap::{App, Arg};
#[allow(unused_imports)]
use futures::{future, stream, Stream};
#[allow(unused_imports)]
use kallichore_api::{
    models, AdoptSessionResponse, Api, ApiNoContext, ChannelsWebsocketResponse, Client,
    ClientHeartbeatResponse, ConnectionInfoResponse, ContextWrapperExt, DeleteSessionResponse,
    GetSessionResponse, InterruptSessionResponse, KillSessionResponse, ListSessionsResponse,
    NewSessionResponse, RestartSessionResponse, ServerStatusResponse, ShutdownServerResponse,
    StartSessionResponse,
};

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

// rt may be unused if there are no examples
#[allow(unused_mut)]
fn main() {
    env_logger::init();

    let matches = App::new("client")
        .arg(
            Arg::with_name("operation")
                .help("Sets the operation to run")
                .possible_values(&[
                    "ChannelsWebsocket",
                    "ClientHeartbeat",
                    "ConnectionInfo",
                    "DeleteSession",
                    "GetSession",
                    "InterruptSession",
                    "KillSession",
                    "ListSessions",
                    "RestartSession",
                    "ServerStatus",
                    "ShutdownServer",
                    "StartSession",
                ])
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("https")
                .long("https")
                .help("Whether to use HTTPS or not"),
        )
        .arg(
            Arg::with_name("host")
                .long("host")
                .takes_value(true)
                .default_value("localhost")
                .help("Hostname to contact"),
        )
        .arg(
            Arg::with_name("port")
                .long("port")
                .takes_value(true)
                .default_value("8080")
                .help("Port to contact"),
        )
        .get_matches();

    let is_https = matches.is_present("https");
    let base_url = format!(
        "{}://{}:{}",
        if is_https { "https" } else { "http" },
        matches.value_of("host").unwrap(),
        matches.value_of("port").unwrap()
    );

    let context: ClientContext = swagger::make_context!(
        ContextBuilder,
        EmptyContext,
        None as Option<AuthData>,
        XSpanIdString::default()
    );

    let mut client: Box<dyn ApiNoContext<ClientContext>> = {
        // Using HTTP
        let client =
            Box::new(Client::try_new_http(&base_url).expect("Failed to create HTTP client"));
        Box::new(client.with_context(context))
    };

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    match matches.value_of("operation") {
        /* Disabled because there's no example.
        Some("AdoptSession") => {
            let result = rt.block_on(client.adopt_session(
                  "session_id_example".to_string(),
                  ???
            ));
            info!("{:?} (X-Span-ID: {:?})", result, (client.context() as &dyn Has<XSpanIdString>).get().clone());
        },
        */
        Some("ChannelsWebsocket") => {
            let result = rt.block_on(client.channels_websocket("session_id_example".to_string()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("ClientHeartbeat") => {
            let result = rt.block_on(client.client_heartbeat());
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
        Some("RestartSession") => {
            let result =
                rt.block_on(client.restart_session("session_id_example".to_string(), None));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("ServerStatus") => {
            let result = rt.block_on(client.server_status());
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some("ShutdownServer") => {
            let result = rt.block_on(client.shutdown_server());
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
