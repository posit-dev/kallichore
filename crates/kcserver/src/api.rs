//
// api.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Contains the API implementation for the Kallichore server

use async_trait::async_trait;
use std::error::Error;
use std::task::{Poll, Context};
use swagger::ApiError;
use openapi_client::models;

/// The API implementation for the Kallichore server
#[derive(Clone)]
pub struct KallichoreApi;

#[async_trait]
impl<C: Send + Sync> openapi_client::Api<C> for KallichoreApi {
    fn poll_ready(&self, _cx: &mut Context) -> Poll<Result<(), Box<dyn Error + Send + Sync + 'static>>> {
        Poll::Ready(Ok(()))
    }

    async fn list_sessions(
        &self,
        _context: &C) -> Result<openapi_client::ListSessionsResponse, ApiError> {
        Ok(openapi_client::ListSessionsResponse::ReturnsAListOfActiveSessions(models::SessionList {
            sessions: Some(vec![]),
            total: Some(0),
        }))
    }
}
