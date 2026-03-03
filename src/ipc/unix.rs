use std::io;
use tokio::net::{UnixListener, UnixStream};

pub struct IpcListener {
    inner: UnixListener,
}

pub type IpcStream = UnixStream;

impl IpcListener {
    pub async fn bind(path: &str) -> io::Result<Self> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        Ok(Self { inner: listener })
    }

    pub async fn accept(&self) -> io::Result<IpcStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(stream)
    }
}
