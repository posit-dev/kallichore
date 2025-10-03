//
// named_pipe_connection.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Named pipe connection implementation for direct hyper integration on
//! Windows. This allows us to use a named pipe as a transport for hyper-based
//! servers, similar to how `hyperlocal` works with Unix domain sockets.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

/// A wrapper around Windows named pipe that implements AsyncRead and AsyncWrite
/// for direct integration with hyper
pub struct NamedPipeConnection {
    pipe: NamedPipeServer,
}

impl NamedPipeConnection {
    pub fn new(pipe: NamedPipeServer) -> Self {
        Self { pipe }
    }
}

impl AsyncRead for NamedPipeConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.pipe).poll_read(cx, buf)
    }
}

impl AsyncWrite for NamedPipeConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.pipe).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.pipe).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.pipe).poll_shutdown(cx)
    }
}

/// Incoming connection stream for named pipes, similar to hyperlocal's SocketIncoming
pub struct NamedPipeIncoming {
    pipe_name: String,
    connect_future: Option<Pin<Box<dyn Future<Output = io::Result<NamedPipeServer>> + Send>>>,
}

impl NamedPipeIncoming {
    pub fn new(pipe_name: String) -> io::Result<Self> {
        Ok(Self {
            pipe_name,
            connect_future: None,
        })
    }

    fn create_connect_future(
        &self,
    ) -> Pin<Box<dyn Future<Output = io::Result<NamedPipeServer>> + Send>> {
        let pipe_name = self.pipe_name.clone();
        Box::pin(async move {
            let pipe = ServerOptions::new().create(&pipe_name)?;
            pipe.connect().await?;
            Ok(pipe)
        })
    }
}

impl hyper::server::accept::Accept for NamedPipeIncoming {
    type Conn = NamedPipeConnection;
    type Error = io::Error;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        // If we don't have a connection future, create one
        if self.connect_future.is_none() {
            self.connect_future = Some(self.create_connect_future());
        }

        // Poll the connection future
        if let Some(mut future) = self.connect_future.take() {
            match future.as_mut().poll(cx) {
                Poll::Ready(Ok(pipe)) => {
                    // Connection successful, return the connection
                    // and prepare for the next connection by clearing the future
                    Poll::Ready(Some(Ok(NamedPipeConnection::new(pipe))))
                }
                Poll::Ready(Err(e)) => {
                    // Connection failed
                    Poll::Ready(Some(Err(e)))
                }
                Poll::Pending => {
                    // Still waiting for connection, put the future back
                    self.connect_future = Some(future);
                    Poll::Pending
                }
            }
        } else {
            // This shouldn't happen, but handle it gracefully
            Poll::Ready(Some(Err(io::Error::new(
                io::ErrorKind::Other,
                "No connection future available",
            ))))
        }
    }
}
