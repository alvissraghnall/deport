#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
mod windows_client;

#[cfg(target_family = "unix")]
mod unix;

use std::{io, sync::Arc};

use rkyv::{Archive, Deserialize, Serialize, from_bytes, rancor, to_bytes};

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "windows")]
pub use windows_client::*;

#[cfg(target_family = "unix")]
pub use unix::*;

pub mod worker;
pub mod client;

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{mpsc::{self, Sender}, oneshot},
};

use crate::{
    ipc::worker::WorkItem,
    process_man::{ProcessConfig, ProcessInfo, ProcessManager},
};

trait IpcListenerTrait {
    async fn bind(addr: &str) -> std::io::Result<Self>
    where
        Self: Sized;

    async fn accept(&self) -> std::io::Result<IpcStream>;
}

pub trait IpcStreamTrait: AsyncRead + AsyncWrite + Unpin + Send {}

#[derive(Debug, Serialize, Deserialize, Archive)]
pub(crate) enum Request {
    Spawn { config: ProcessConfig },
    List,
    Stop { name: String },
    KillDaemon,
}

#[derive(Debug, Serialize, Deserialize, Archive)]
pub(crate) enum Response {
    List { processes: Vec<ProcessInfo> },
    ProcessInfo(ProcessInfo),
    Error { message: String },
    Ok { message: String },
}

impl Request {
    pub async fn process(self, process_man: Arc<ProcessManager>) -> Response {
        match self {
            Request::Spawn { config } => match process_man.spawn(config).await {
                Ok(info) => Response::ProcessInfo(info),
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::List => Response::List {
                processes: (process_man.list().await),
            },
            Request::Stop { name } => match process_man.kill(name).await {
                Ok(()) => Response::Ok {
                    message: "Process stopped successfully".into(),
                },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::KillDaemon => {
                process_man.stop_all().await;
                Response::Ok {
                    message: "Daemon killed successfully".into(),
                }
            }
        }
    }
}

pub async fn run(tx: mpsc::Sender<WorkItem>) -> std::io::Result<()> {
    #[cfg(unix)]
    let addr = "/tmp/mydaemon.sock";

    #[cfg(windows)]
    let addr = r"\\.\pipe\mydaemon";

    let listener = IpcListener::bind(addr).await?;

    loop {
        let mut stream: IpcStream = listener.accept().await?;

        let tx = tx.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(&mut stream, tx).await {
                eprintln!("Connection error: {}", e);
            }
        });
    }
}

async fn handle_client<T>(mut stream: T, tx: Sender<WorkItem>) -> io::Result<()>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
{
    loop {
        let msg_len = match stream.read_u32().await {
            Ok(0) => return Ok(()),
            Ok(n) => n as usize,
            Err(e) => return Err(e),
        };

        let mut buf = vec![0u8; msg_len];
        stream.read_exact(&mut buf).await?;

        let request = from_bytes::<Request, rancor::Error>(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let (resp_tx, resp_rx) = oneshot::channel();

        let work = WorkItem::new(request, resp_tx);

        if tx.send(work).await.is_err() {
            return Ok(());
        }

        if let Ok(resp) = resp_rx.await {
            let reply = to_bytes::<rancor::Error>(&resp)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            let len = reply.len() as u32;
            stream.write_u32(len).await?;
            stream.write_all(&reply).await?;
        }
    }
}
