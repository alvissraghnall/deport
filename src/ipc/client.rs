use rkyv::{rancor, to_bytes};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::{io};

use crate::ipc::{ClientIpcStream, Request, Response};

pub struct IpcClient;

impl IpcClient {
    pub async fn send_request<T>(mut stream: T, req: Request) -> io::Result<Response>
    where
        T: AsyncReadExt + AsyncWriteExt + Unpin,
    {
        let msg = to_bytes::<rancor::Error>(&req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let len = msg.len() as u32;
        
        stream.write_u32(len).await?;
        stream.write_all(&msg).await?;

        let n = stream.read_u32().await?;

        let mut buf = vec![0u8; n as usize];
        stream.read_exact(&mut buf).await?;

        
        let resp = rkyv::from_bytes::<Response, rancor::Error>(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(resp)
    }

     pub async fn connect(path: &str) -> io::Result<ClientIpcStream> {
        crate::ipc::connect(path).await
    }
}
