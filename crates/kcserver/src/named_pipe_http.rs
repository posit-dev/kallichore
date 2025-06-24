//
// named_pipe_http.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

//! HTTP server implementation over Windows named pipes

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::pin::Pin;
#[cfg(windows)]
use std::task::{Context, Poll};
#[cfg(windows)]
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, OPEN_EXISTING},
    Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_ACCESS_DUPLEX,
        PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    },
    Win32::System::IO::OVERLAPPED,
};

/// A Windows named pipe server
#[cfg(windows)]
pub struct NamedPipeServer {
    pipe_name: String,
}

#[cfg(windows)]
impl NamedPipeServer {
    pub fn bind(pipe_name: &str) -> std::io::Result<Self> {
        // Validate pipe name format
        if !pipe_name.starts_with("\\\\.\\pipe\\") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid pipe name format: {}", pipe_name),
            ));
        }

        log::info!("Creating named pipe server at: {}", pipe_name);

        Ok(NamedPipeServer {
            pipe_name: pipe_name.to_string(),
        })
    }

    pub async fn accept(&self) -> std::io::Result<NamedPipeStream> {
        // Create a new pipe instance for each connection
        let pipe_handle = self.create_pipe_instance()?;

        // Wait for a client to connect
        self.wait_for_connection(pipe_handle).await?;

        Ok(NamedPipeStream::new(pipe_handle))
    }

    fn create_pipe_instance(&self) -> std::io::Result<HANDLE> {
        unsafe {
            // Convert pipe name to wide string
            let wide_name: Vec<u16> = OsStr::new(&self.pipe_name)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let handle = CreateNamedPipeW(
                PCWSTR(wide_name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                4096, // Output buffer size
                4096, // Input buffer size
                0,    // Default timeout
                None, // Default security
            );

            if handle == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }

            Ok(handle)
        }
    }

    async fn wait_for_connection(&self, handle: HANDLE) -> std::io::Result<()> {
        // For now, use a simplified connection mechanism
        // In a full implementation, this would use overlapped I/O
        unsafe {
            let result = ConnectNamedPipe(handle, None);
            if result.is_err() {
                let error = std::io::Error::last_os_error();
                // ERROR_PIPE_CONNECTED means client is already connected
                if error.raw_os_error() != Some(535) {
                    CloseHandle(handle);
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

/// A Windows named pipe stream
#[cfg(windows)]
pub struct NamedPipeStream {
    handle: HANDLE,
}

#[cfg(windows)]
impl NamedPipeStream {
    fn new(handle: HANDLE) -> Self {
        NamedPipeStream { handle }
    }
}

#[cfg(windows)]
impl Drop for NamedPipeStream {
    fn drop(&mut self) {
        unsafe {
            DisconnectNamedPipe(self.handle);
            CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
impl AsyncRead for NamedPipeStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // For now, implement a basic read operation
        // In a full implementation, this would use overlapped I/O
        use std::ptr;
        use windows::Win32::Storage::FileSystem::ReadFile;

        unsafe {
            let mut bytes_read = 0u32;
            let buffer = buf.unfilled_mut();

            let result = ReadFile(
                self.handle,
                Some(buffer.as_mut_ptr() as *mut u8 as *mut std::ffi::c_void),
                buffer.len() as u32,
                Some(&mut bytes_read),
                None,
            );

            if result.is_ok() {
                buf.advance(bytes_read as usize);
                Poll::Ready(Ok(()))
            } else {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    // Register for wake-up when data is available
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Err(error))
                }
            }
        }
    }
}

#[cfg(windows)]
impl AsyncWrite for NamedPipeStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        // For now, implement a basic write operation
        use windows::Win32::Storage::FileSystem::WriteFile;

        unsafe {
            let mut bytes_written = 0u32;

            let result = WriteFile(
                self.handle,
                Some(buf.as_ptr() as *const std::ffi::c_void),
                buf.len() as u32,
                Some(&mut bytes_written),
                None,
            );

            if result.is_ok() {
                Poll::Ready(Ok(bytes_written as usize))
            } else {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Err(error))
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        // Named pipes don't typically need explicit flushing
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

/// Create a hyper service that accepts connections from a named pipe server
#[cfg(windows)]
pub struct NamedPipeIncoming {
    server: NamedPipeServer,
}

#[cfg(windows)]
impl NamedPipeIncoming {
    pub fn new(pipe_name: &str) -> std::io::Result<Self> {
        let server = NamedPipeServer::bind(pipe_name)?;
        Ok(NamedPipeIncoming { server })
    }
}

#[cfg(windows)]
impl hyper::server::accept::Accept for NamedPipeIncoming {
    type Conn = NamedPipeStream;
    type Error = std::io::Error;

    fn poll_accept(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        // This is a simplified implementation
        // For a production version, this should be properly async
        match futures::executor::block_on(self.server.accept()) {
            Ok(stream) => Poll::Ready(Some(Ok(stream))),
            Err(e) => Poll::Ready(Some(Err(e))),
        }
    }
}
