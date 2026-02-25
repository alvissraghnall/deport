
use tokio::net::windows::named_pipe::{
    NamedPipeServer,
    ServerOptions,
};
use std::io;

pub struct IpcListener {
    name: String,
}

pub type IpcStream = NamedPipeServer;

impl IpcListener {
    pub async fn bind(name: &str) -> io::Result<Self> {
        Ok(Self {
            name: name.to_string(),
        })
    }

    pub async fn accept(&self) -> io::Result<IpcStream> {
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&self.name)?;

        server.connect().await?;

        Ok(server)
    }
}

