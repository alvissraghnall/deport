use mio::{Events, Poll};
use std::{
    fs::{Permissions, set_permissions},
    io,
    os::unix::fs::PermissionsExt as _,
};
use tokio::{
    io::AsyncReadExt as _,
    net::{UnixListener, UnixStream},
};

use crate::ipc::{IpcListenerTrait, Request};

pub struct IpcListener {
    inner: UnixListener,

    poll: Poll,
    events: Events,
}

pub type IpcStream = UnixStream;

impl IpcListenerTrait for IpcListener {
    async fn bind(path: &str) -> io::Result<Self> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;

        set_permissions(path, Permissions::from_mode(0o600))?;

        let poll = Poll::new()?;
        let events = Events::with_capacity(128);

        poll.registry()
            .register(&mut listener, Token(0), Interest::READABLE)?;

        Ok(Self {
            inner: listener,
            poll,
            events,
        })
    }

    async fn accept(&self) -> io::Result<IpcStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(stream)
    }

    async fn handle_new_messages(&mut self) -> io::Result<()> {
        self.poll
            .poll(&mut self.events, Some(Duration::from_nanos(10)))?;

        for event in &self.events {
            if event.token() == Token(0) {
                let (stream, _) = self.inner.accept().await?;
                context.handle_new_client(stream).await?;
            } else {
                context.handle_client_message(event.token()).await?;
            }
        }
    }
}

impl IpcListener {
    async fn handle_new_client(&mut self, mut stream: UnixStream) -> io::Result<()> {
        let mut buf = [0u8; 1024];

        loop {
            let n = match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };

            // Process the command and send a response
            let command: C = bincode::deserialize(&buf[..n])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            self.process_command(command, context, &mut stream)?;
        }

        Ok(())
    }

    #[inline(always)]
    fn process_command(
        &self,
        command: Request,
        stream: &mut UnixStream,
    ) -> io::Result<()> {
        let response = command.process(context);
        loop {
            match bincode::serialize_into(&mut *stream, &response)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            {
                Ok(()) => return Ok(()),
                Err(ref err) if would_block(err) => {
                    std::hint::spin_loop();
                    continue;
                }
                e => return e,
            }
        }
    }
}
