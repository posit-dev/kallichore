//
// main.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! kcclient
//!
//! Kallichore Client
#![allow(missing_docs, unused_variables, trivial_casts)]

use directories::BaseDirs;
#[allow(unused_imports)]
use futures::{future, stream, Stream};
use kallichore_api::NewSessionResponse;
#[allow(unused_imports)]
use kallichore_api::{models, Api, ApiNoContext, Client, ContextWrapperExt, ListSessionsResponse};

#[allow(unused_imports)]
use log::info;

use clap::Parser;
use clap_derive::{Parser, Subcommand};

mod kernel_spec;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Optional URL to use as the base for API requests
    #[arg(short, long, value_name = "URL")]
    url: Option<String>,

    /// Subcommands
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Lists active sessions
    List,

    /// Start a new session
    Start {
        /// The previously registered kernel to use
        #[arg(short, long)]
        kernel: String,
    },
}

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

    // Read command line arguments
    let args = Args::parse();

    // Determine the base URL for API requests
    let base_url = match args.url {
        Some(url) => url,
        None => String::from("http://localhost:8080"),
    };

    let context: ClientContext = swagger::make_context!(
        ContextBuilder,
        EmptyContext,
        None as Option<AuthData>,
        XSpanIdString::default()
    );

    let client = Box::new(Client::try_new_http(&base_url).expect("Failed to create HTTP client"));
    let client = Box::new(client.with_context(context));

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let session_id = format!("{:08x}", rand::random::<u32>());

    let working_directory = std::env::current_dir().unwrap();

    // Use the command froom the command line arguments to determine which operation to perform
    match args.command {
        Some(Commands::List) => {
            let result = rt.block_on(client.list_sessions());
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
            // If the result is successful, pretty-print the list of sessions as JSON
            if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                println!("{}", serde_json::to_string_pretty(&sessions).unwrap());
            }
        }
        Some(Commands::Start { kernel }) => {
            let base_dir = BaseDirs::new().unwrap();

            // On macOS, Jupyter doens't follow XDG Base Directory
            // Specification; it stores its data in `~/Library/Jupyter` instead
            // of the "correct" XDG location in `~/Library/Application Support`.
            #[cfg(target_os = "macos")]
            let mut jupyter_dir = base_dir.home_dir().join("Library").join("Jupyter");

            #[cfg(not(target_os = "macos"))]
            let mut jupyter_dir = directories::ProjectDirs::from("Jupyter", "", "")
                .unwrap()
                .data_dir();

            let kernel_spec_json = jupyter_dir
                .join("kernels")
                .join(kernel.clone())
                .join("kernel.json");

            // Parse the kernel spec from the JSON file using the serde json library
            let kernel_spec: kernel_spec::KernelSpec =
                serde_json::from_reader(std::fs::File::open(kernel_spec_json).unwrap())
                    .expect("Failed to parse kernel spec");

            let session = models::Session {
                session_id,
                argv: kernel_spec.argv,
                working_directory: working_directory.to_string_lossy().to_string(),
            };
            info!(
                "Creating new session for '{}' kernel ({}) with id {}",
                kernel,
                kernel_spec.display_name,
                session.session_id.clone()
            );

            let result = rt.block_on(client.new_session(session));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
            // Pretty print the result as JSON
            match result {
                Ok(NewSessionResponse::TheSessionID(session_id)) => {
                    println!("{}", serde_json::to_string_pretty(&session_id).unwrap());
                }
                Ok(NewSessionResponse::InvalidRequest(error)) => {
                    println!("{}", serde_json::to_string_pretty(&error).unwrap());
                }
                _ => {}
            }
        }
        None => {
            eprintln!("No command specified");
        }
    }
}
