//! kcclient
//!
//! Kallichore Client
#![allow(missing_docs, unused_variables, trivial_casts)]

#[allow(unused_imports)]
use futures::{future, stream, Stream};
#[allow(unused_imports)]
use kallichore_api::{models, Api, ApiNoContext, Client, ContextWrapperExt, ListSessionsResponse};

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

    let base_url = String::from("http://localhost:8080");

    let context: ClientContext = swagger::make_context!(
        ContextBuilder,
        EmptyContext,
        None as Option<AuthData>,
        XSpanIdString::default()
    );

    let client = Box::new(Client::try_new_http(&base_url).expect("Failed to create HTTP client"));
    let client = Box::new(client.with_context(context));

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let session = models::Session {
        id: String::from("1"),
        argv: vec![],
    };
    let result = rt.block_on(client.new_session(session));
    info!(
        "{:?} (X-Span-ID: {:?})",
        result,
        (client.context() as &dyn Has<XSpanIdString>).get().clone()
    );
    let result = rt.block_on(client.list_sessions());
    info!(
        "{:?} (X-Span-ID: {:?})",
        result,
        (client.context() as &dyn Has<XSpanIdString>).get().clone()
    );
}
