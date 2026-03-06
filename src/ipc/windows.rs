
use tokio::net::windows::named_pipe::{
    NamedPipeServer,
    ServerOptions,
};
use std::io;

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
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&self.name)?;

        server.connect().await?;

        Ok(server)
    }

    async fn handle_new_messages(&mut self) -> io::Result<()> {
        // would need to handle each connection in a separate task as windows doesn't support poll-esquw
        Ok(())
    }
}

