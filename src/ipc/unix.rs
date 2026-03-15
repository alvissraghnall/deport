use std::{
    fs::{Permissions, set_permissions},
    io,
    os::unix::fs::PermissionsExt as _,
};
use tokio::{
    net::{UnixListener, UnixStream},
};

use crate::ipc::{IpcListenerTrait};

pub struct IpcListener {
    inner: UnixListener,
}

pub type IpcStream = UnixStream;
pub type ClientIpcStream = UnixStream;

impl IpcListenerTrait for IpcListener {
    async fn bind(path: &str) -> io::Result<Self> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;

        set_permissions(path, Permissions::from_mode(0o600))?;

        Ok(Self {
            inner: listener,
        })
    }

    async fn accept(&self) -> io::Result<IpcStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(stream)
    }
    
}

pub async fn connect(path: &str) -> io::Result<ClientIpcStream> {
    UnixStream::connect(path).await
}