//
// body_reader.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//
use futures::Stream;
use hyper::body::Buf;
use hyper::Body;
use hyper::Request;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::ReadBuf;
use tokio::io::{AsyncRead, AsyncWrite};

pub struct RequestBodyReader {
    body: Body,
    buffer: Option<hyper::body::Bytes>,
}

impl RequestBodyReader {
    pub fn new(req: Request<Body>) -> Self {
        RequestBodyReader {
            body: req.into_body(),
            buffer: None,
        }
    }
}

impl AsyncRead for RequestBodyReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        loop {
            if let Some(mut buffer) = self.buffer.take() {
                let len = buf.remaining();
                buf.put_slice(&buffer.split_to(len));
                if buffer.has_remaining() {
                    self.buffer = Some(buffer);
                }
                return Poll::Ready(Ok(()));
            }

            match futures::ready!(Pin::new(&mut self.body).poll_next(cx)) {
                Some(Ok(chunk)) => {
                    self.buffer = Some(chunk);
                }
                Some(Err(err)) => {
                    return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err)));
                }
                None => return Poll::Ready(Ok(())), // EOF
            }
        }
    }
}

impl AsyncWrite for RequestBodyReader {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "write not supported",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
