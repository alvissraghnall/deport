#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
mod windows_client;

#[cfg(target_family = "unix")]
mod unix;

use std::{io::{self}, sync::Arc};

use anyhow::bail;
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
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc::{self, Sender}, oneshot},
};

use crate::{
    APP_STATE, ipc::worker::WorkItem, process_man::{ProcessConfig, ProcessInfo, ProcessManager}, state::AppStateTrait
};

trait IpcListenerTrait {
    async fn bind(addr: &str) -> std::io::Result<Self>
    where
        Self: Sized;

    async fn accept(&self) -> std::io::Result<IpcStream>;
}

// pub trait IpcStreamTrait: AsyncRead + AsyncWrite + Unpin + Send {}

#[derive(Debug, Serialize, Deserialize, Archive)]
pub(crate) enum Request {
    Spawn { config: ProcessConfig },
    List,
    Stop { name: String },
    KillDaemon,
    AddRoute { hostname: String, route: crate::routes::Route },
    ListRoutes,
    GetRoute { hostname: String },
    DeleteRoute { hostname: String },
    SetProxyPort(u16),
}

#[derive(Debug, Serialize, Deserialize, Archive)]
pub(crate) enum Response {
    List { processes: Vec<ProcessInfo> },
    ProcessInfo(ProcessInfo),
    Error { message: String },
    Ok { message: String },
    Routes { routes: Vec<(String, crate::routes::Route)> },
    Route(Option<crate::routes::Route>),
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
                
                tokio::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    crate::proxy::stop_proxy();
                });

                Response::Ok {
                    message: "Daemon killed successfully".into(),
                }
            }
            Request::AddRoute { hostname, route } => {
                APP_STATE.routes.insert(Arc::from(hostname.clone()), route);
                return Response::Ok { message: format!("Route {} added successfully!", hostname) }
            },
            Request::ListRoutes => {
                let routes = APP_STATE.routes.list();
                return Response::Routes { routes };
            }
            Request::DeleteRoute { hostname } => {
                APP_STATE.routes.remove(&hostname);
                return Response::Ok { message: format!("Route {} deleted successfully!", hostname) }
            }
            Request::SetProxyPort(port) => {
                APP_STATE.set_proxy_port(port);
                return Response::Ok { message: format!("Proxy port set to {} successfully!", port) }
            },
            Request::GetRoute { hostname } => {
                let route = APP_STATE.routes.get(hostname.as_str());
                return Response::Route(route);
            },
        }
    }
}

pub async fn run(tx: mpsc::Sender<WorkItem>) -> std::io::Result<()> {
    #[cfg(unix)]
    let addr = "/tmp/deport.sock";

    #[cfg(windows)]
    let addr = r"\\.\pipe\deport";

    let listener = IpcListener::bind(addr).await?;

    loop {
        let mut stream: IpcStream = listener.accept().await?;

        let tx = tx.clone();

        println!("{:?}", tx);

        tokio::spawn(async move {
            if let Err(e) = handle_client(&mut stream, tx).await {
                eprintln!("[IPC] Connection error: {}", e);
            }
        });
    }
}

async fn handle_client<T>(mut stream: T, tx: Sender<WorkItem>) -> anyhow::Result<()>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
{
    loop {
        let msg_len = match stream.read_u32().await {
            Ok(n) => n as usize,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => bail!(e),
        };

        let mut buf = vec![0u8; msg_len];
        stream.read_exact(&mut buf).await?;

        let request = from_bytes::<Request, rancor::Error>(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let (resp_tx, resp_rx) = oneshot::channel();

        let work = WorkItem::new(request, resp_tx);

        if tx.send(work).await.is_err() {
            bail!("Unable to send work!");
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
