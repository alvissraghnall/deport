
use tokio::net::windows::named_pipe::{
    NamedPipeServer,
    ServerOptions,
};
use std::io;
use crate::ipc::IpcListenerTrait;

pub struct IpcListener {
    name: String,
}

pub type IpcStream = NamedPipeServer;

impl IpcListenerTrait for IpcListener {
    async fn bind(name: &str) -> io::Result<Self> {
        Ok(Self {
            name: name.to_string(),
        })
    }

    async fn accept(&self) -> io::Result<IpcStream> {
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&self.name)?;

        server.connect().await?;

        Ok(server)
    }

}
