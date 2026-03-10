use tokio::net::windows::named_pipe::ClientOptions;
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::time::{sleep, Duration};
use std::io;
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
use crate::ipc::IpcStreamTrait;

impl IpcStreamTrait for NamedPipeClient {}

pub type ClientIpcStream = NamedPipeClient;

pub async fn connect(path: &str) -> io::Result<ClientIpcStream> {
    let client = loop {
        match ClientOptions::new().open(path) {
            Ok(client) => break client,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => (),
            Err(e) => return Err(e),
        }
        sleep(Duration::from_millis(50)).await;
    };

    Ok(client)
}