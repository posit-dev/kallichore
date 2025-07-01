//! CLI tool driving the API client
use anyhow::{anyhow, Context, Result};
use dialoguer::Confirm;
use log::{debug, info};
// models may be unused if all inputs are primitive types
#[allow(unused_imports)]
use kallichore_api::{
    models, AdoptSessionResponse, ApiNoContext, ChannelsUpgradeResponse, Client,
    ClientHeartbeatResponse, ConnectionInfoResponse, ContextWrapperExt, DeleteSessionResponse,
    GetServerConfigurationResponse, GetSessionResponse, InterruptSessionResponse,
    KillSessionResponse, ListSessionsResponse, NewSessionResponse, RestartSessionResponse,
    ServerStatusResponse, SetServerConfigurationResponse, ShutdownServerResponse,
    StartSessionResponse,
};
use simple_logger::SimpleLogger;
use structopt::StructOpt;
use swagger::{AuthData, ContextBuilder, EmptyContext, Push, XSpanIdString};

type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

#[derive(StructOpt, Debug)]
#[structopt(
    name = "Kallichore API",
    version = "1.0.0",
    about = "CLI access to Kallichore API"
)]
struct Cli {
    #[structopt(subcommand)]
    operation: Operation,

    /// Address or hostname of the server hosting this API, including optional port
    #[structopt(short = "a", long, default_value = "http://localhost")]
    server_address: String,

    /// Path to the client private key if using client-side TLS authentication
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios")))]
    #[structopt(long, requires_all(&["client-certificate", "server-certificate"]))]
    client_key: Option<String>,

    /// Path to the client's public certificate associated with the private key
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios")))]
    #[structopt(long, requires_all(&["client-key", "server-certificate"]))]
    client_certificate: Option<String>,

    /// Path to CA certificate used to authenticate the server
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios")))]
    #[structopt(long)]
    server_certificate: Option<String>,

    /// If set, write output to file instead of stdout
    #[structopt(short, long)]
    output_file: Option<String>,

    #[structopt(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,

    /// Don't ask for any confirmation prompts
    #[allow(dead_code)]
    #[structopt(short, long)]
    force: bool,

    /// Bearer token if used for authentication
    #[structopt(env = "KALLICHORE_API_BEARER_TOKEN", hide_env_values = true)]
    bearer_token: Option<String>,
}

#[derive(StructOpt, Debug)]
enum Operation {
    /// Notify the server that a client is connected
    ClientHeartbeat {
        #[structopt(parse(try_from_str = parse_json))]
        client_heartbeat: models::ClientHeartbeat,
    },
    /// Get the server configuration
    GetServerConfiguration {},
    /// List active sessions
    ListSessions {},
    /// Create a new session
    NewSession {
        #[structopt(parse(try_from_str = parse_json))]
        new_session: models::NewSession,
    },
    /// Get server status and information
    ServerStatus {},
    /// Change the server configuration
    SetServerConfiguration {
        #[structopt(parse(try_from_str = parse_json))]
        server_configuration: models::ServerConfiguration,
    },
    /// Shut down all sessions and the server itself
    ShutdownServer {},
    /// Adopt an existing session
    AdoptSession {
        session_id: String,
        #[structopt(parse(try_from_str = parse_json))]
        connection_info: models::ConnectionInfo,
    },
    /// Upgrade to a WebSocket or domain socket for channel communication
    ChannelsUpgrade { session_id: String },
    /// Get Jupyter connection information for the session
    ConnectionInfo { session_id: String },
    /// Delete session
    DeleteSession { session_id: String },
    /// Get session details
    GetSession { session_id: String },
    /// Interrupt session
    InterruptSession { session_id: String },
    /// Force quit session
    KillSession { session_id: String },
    /// Restart a session
    RestartSession {
        session_id: String,
        #[structopt(parse(try_from_str = parse_json))]
        restart_session: Option<models::RestartSession>,
    },
    /// Start a session
    StartSession { session_id: String },
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios")))]
fn create_client(
    args: &Cli,
    context: ClientContext,
) -> Result<Box<dyn ApiNoContext<ClientContext>>> {
    if args.client_certificate.is_some() {
        debug!("Using mutual TLS");
        let client = Client::try_new_https_mutual(
            &args.server_address,
            args.server_certificate.clone().unwrap(),
            args.client_key.clone().unwrap(),
            args.client_certificate.clone().unwrap(),
        )
        .context("Failed to create HTTPS client")?;
        Ok(Box::new(client.with_context(context)))
    } else if args.server_certificate.is_some() {
        debug!("Using TLS with pinned server certificate");
        let client = Client::try_new_https_pinned(
            &args.server_address,
            args.server_certificate.clone().unwrap(),
        )
        .context("Failed to create HTTPS client")?;
        Ok(Box::new(client.with_context(context)))
    } else {
        debug!("Using client without certificates");
        let client =
            Client::try_new(&args.server_address).context("Failed to create HTTP(S) client")?;
        Ok(Box::new(client.with_context(context)))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "ios"))]
fn create_client(
    args: &Cli,
    context: ClientContext,
) -> Result<Box<dyn ApiNoContext<ClientContext>>> {
    let client =
        Client::try_new(&args.server_address).context("Failed to create HTTP(S) client")?;
    Ok(Box::new(client.with_context(context)))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::from_args();
    if let Some(log_level) = args.verbosity.log_level() {
        SimpleLogger::new()
            .with_level(log_level.to_level_filter())
            .init()?;
    }

    debug!("Arguments: {:?}", &args);

    let mut auth_data: Option<AuthData> = None;

    if let Some(ref bearer_token) = args.bearer_token {
        debug!("Using bearer token");
        auth_data = Some(AuthData::bearer(bearer_token));
    }

    #[allow(trivial_casts)]
    let context = swagger::make_context!(
        ContextBuilder,
        EmptyContext,
        auth_data,
        XSpanIdString::default()
    );

    let client = create_client(&args, context)?;

    let result = match args.operation {
        Operation::ClientHeartbeat { client_heartbeat } => {
            info!("Performing a ClientHeartbeat request");

            let result = client.client_heartbeat(client_heartbeat).await?;
            debug!("Result: {:?}", result);

            match result {
                ClientHeartbeatResponse::HeartbeatReceived(body) => {
                    "HeartbeatReceived\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
            }
        }
        Operation::GetServerConfiguration {} => {
            info!("Performing a GetServerConfiguration request");

            let result = client.get_server_configuration().await?;
            debug!("Result: {:?}", result);

            match result {
                GetServerConfigurationResponse::TheCurrentServerConfiguration(body) => {
                    "TheCurrentServerConfiguration\n".to_string()
                        + &serde_json::to_string_pretty(&body)?
                }
                GetServerConfigurationResponse::FailedToGetConfiguration(body) => {
                    "FailedToGetConfiguration\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
            }
        }
        Operation::ListSessions {} => {
            info!("Performing a ListSessions request");

            let result = client.list_sessions().await?;
            debug!("Result: {:?}", result);

            match result {
                ListSessionsResponse::ListOfActiveSessions(body) => {
                    "ListOfActiveSessions\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
            }
        }
        Operation::NewSession { new_session } => {
            info!("Performing a NewSession request");

            let result = client.new_session(new_session).await?;
            debug!("Result: {:?}", result);

            match result {
                NewSessionResponse::TheSessionID(body) => {
                    "TheSessionID\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                NewSessionResponse::InvalidRequest(body) => {
                    "InvalidRequest\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                NewSessionResponse::Unauthorized => "Unauthorized\n".to_string(),
            }
        }
        Operation::ServerStatus {} => {
            info!("Performing a ServerStatus request");

            let result = client.server_status().await?;
            debug!("Result: {:?}", result);

            match result {
                ServerStatusResponse::ServerStatusAndInformation(body) => {
                    "ServerStatusAndInformation\n".to_string()
                        + &serde_json::to_string_pretty(&body)?
                }
                ServerStatusResponse::Error(body) => {
                    "Error\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
            }
        }
        Operation::SetServerConfiguration {
            server_configuration,
        } => {
            info!("Performing a SetServerConfiguration request");

            let result = client
                .set_server_configuration(server_configuration)
                .await?;
            debug!("Result: {:?}", result);

            match result {
                SetServerConfigurationResponse::ConfigurationUpdated(body) => {
                    "ConfigurationUpdated\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                SetServerConfigurationResponse::Error(body) => {
                    "Error\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
            }
        }
        Operation::ShutdownServer {} => {
            info!("Performing a ShutdownServer request");

            let result = client.shutdown_server().await?;
            debug!("Result: {:?}", result);

            match result {
                ShutdownServerResponse::ShuttingDown(body) => {
                    "ShuttingDown\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                ShutdownServerResponse::ShutdownFailed(body) => {
                    "ShutdownFailed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                ShutdownServerResponse::Unauthorized => "Unauthorized\n".to_string(),
            }
        }
        Operation::AdoptSession {
            session_id,
            connection_info,
        } => {
            info!("Performing a AdoptSession request on {:?}", (&session_id));

            let result = client.adopt_session(session_id, connection_info).await?;
            debug!("Result: {:?}", result);

            match result {
                AdoptSessionResponse::Adopted(body) => {
                    "Adopted\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                AdoptSessionResponse::AdoptionFailed(body) => {
                    "AdoptionFailed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                AdoptSessionResponse::SessionNotFound => "SessionNotFound\n".to_string(),
                AdoptSessionResponse::Unauthorized => "Unauthorized\n".to_string(),
            }
        }
        Operation::ChannelsUpgrade { session_id } => {
            info!(
                "Performing a ChannelsUpgrade request on {:?}",
                (&session_id)
            );

            let result = client.channels_upgrade(session_id).await?;
            debug!("Result: {:?}", result);

            match result {
                ChannelsUpgradeResponse::UpgradedConnection(body) => {
                    "UpgradedConnection\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                ChannelsUpgradeResponse::InvalidRequest(body) => {
                    "InvalidRequest\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                ChannelsUpgradeResponse::Unauthorized => "Unauthorized\n".to_string(),
                ChannelsUpgradeResponse::SessionNotFound => "SessionNotFound\n".to_string(),
            }
        }
        Operation::ConnectionInfo { session_id } => {
            info!("Performing a ConnectionInfo request on {:?}", (&session_id));

            let result = client.connection_info(session_id).await?;
            debug!("Result: {:?}", result);

            match result {
                ConnectionInfoResponse::ConnectionInfo(body) => {
                    "ConnectionInfo\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                ConnectionInfoResponse::Failed(body) => {
                    "Failed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                ConnectionInfoResponse::Unauthorized => "Unauthorized\n".to_string(),
                ConnectionInfoResponse::SessionNotFound => "SessionNotFound\n".to_string(),
            }
        }
        Operation::DeleteSession { session_id } => {
            prompt(
                args.force,
                "This will delete the given entry, are you sure?",
            )?;
            info!("Performing a DeleteSession request on {:?}", (&session_id));

            let result = client.delete_session(session_id).await?;
            debug!("Result: {:?}", result);

            match result {
                DeleteSessionResponse::SessionDeleted(body) => {
                    "SessionDeleted\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                DeleteSessionResponse::FailedToDeleteSession(body) => {
                    "FailedToDeleteSession\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                DeleteSessionResponse::Unauthorized => "Unauthorized\n".to_string(),
                DeleteSessionResponse::SessionNotFound => "SessionNotFound\n".to_string(),
            }
        }
        Operation::GetSession { session_id } => {
            info!("Performing a GetSession request on {:?}", (&session_id));

            let result = client.get_session(session_id).await?;
            debug!("Result: {:?}", result);

            match result {
                GetSessionResponse::SessionDetails(body) => {
                    "SessionDetails\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                GetSessionResponse::FailedToGetSession(body) => {
                    "FailedToGetSession\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                GetSessionResponse::SessionNotFound => "SessionNotFound\n".to_string(),
            }
        }
        Operation::InterruptSession { session_id } => {
            info!(
                "Performing a InterruptSession request on {:?}",
                (&session_id)
            );

            let result = client.interrupt_session(session_id).await?;
            debug!("Result: {:?}", result);

            match result {
                InterruptSessionResponse::Interrupted(body) => {
                    "Interrupted\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                InterruptSessionResponse::InterruptFailed(body) => {
                    "InterruptFailed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                InterruptSessionResponse::Unauthorized => "Unauthorized\n".to_string(),
                InterruptSessionResponse::SessionNotFound => "SessionNotFound\n".to_string(),
            }
        }
        Operation::KillSession { session_id } => {
            info!("Performing a KillSession request on {:?}", (&session_id));

            let result = client.kill_session(session_id).await?;
            debug!("Result: {:?}", result);

            match result {
                KillSessionResponse::Killed(body) => {
                    "Killed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                KillSessionResponse::KillFailed(body) => {
                    "KillFailed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                KillSessionResponse::Unauthorized => "Unauthorized\n".to_string(),
                KillSessionResponse::SessionNotFound => "SessionNotFound\n".to_string(),
            }
        }
        Operation::RestartSession {
            session_id,
            restart_session,
        } => {
            info!("Performing a RestartSession request on {:?}", (&session_id));

            let result = client.restart_session(session_id, restart_session).await?;
            debug!("Result: {:?}", result);

            match result {
                RestartSessionResponse::Restarted(body) => {
                    "Restarted\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                RestartSessionResponse::RestartFailed(body) => {
                    "RestartFailed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                RestartSessionResponse::Unauthorized => "Unauthorized\n".to_string(),
                RestartSessionResponse::SessionNotFound => "SessionNotFound\n".to_string(),
            }
        }
        Operation::StartSession { session_id } => {
            info!("Performing a StartSession request on {:?}", (&session_id));

            let result = client.start_session(session_id).await?;
            debug!("Result: {:?}", result);

            match result {
                StartSessionResponse::Started(body) => {
                    "Started\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                StartSessionResponse::StartFailed(body) => {
                    "StartFailed\n".to_string() + &serde_json::to_string_pretty(&body)?
                }
                StartSessionResponse::SessionNotFound => "SessionNotFound\n".to_string(),
                StartSessionResponse::Unauthorized => "Unauthorized\n".to_string(),
            }
        }
    };

    if let Some(output_file) = args.output_file {
        std::fs::write(output_file, result)?
    } else {
        println!("{}", result);
    }
    Ok(())
}

fn prompt(force: bool, text: &str) -> Result<()> {
    if force || Confirm::new().with_prompt(text).interact()? {
        Ok(())
    } else {
        Err(anyhow!("Aborting"))
    }
}

// May be unused if all inputs are primitive types
#[allow(dead_code)]
fn parse_json<'a, T: serde::de::Deserialize<'a>>(json_string: &'a str) -> Result<T> {
    serde_json::from_str(json_string).map_err(|err| anyhow!("Error parsing input: {}", err))
}
