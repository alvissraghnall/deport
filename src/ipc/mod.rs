#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
mod windows_client;

#[cfg(target_family = "unix")]
mod unix;

use std::{
    io::{self},
    sync::Arc,
};

use anyhow::bail;
use colored::Colorize as _;
use rkyv::{Archive, Deserialize, Serialize, from_bytes, rancor, to_bytes};

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "windows")]
pub use windows_client::*;

#[cfg(target_family = "unix")]
pub use unix::*;

pub mod client;
pub mod worker;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{
        mpsc::{self, Sender},
        oneshot,
    },
};

#[cfg(unix)]
use crate::daemon;
use crate::{
    APP_STATE,
    commands::proxy::StartArgs,
    ipc::worker::WorkItem,
    process_man::{ProcessConfig, ProcessInfo, ProcessManager, get_default_proxy_port},
    proxy::is_proxy_running,
    state::AppStateTrait,
    trust_ca::is_ca_trusted,
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
    Spawn {
        config: ProcessConfig,
    },
    List,
    Stop {
        name: String,
    },
    KillDaemon,
    AddRoute {
        hostname: String,
        route: crate::routes::Route,
    },
    ListRoutes,
    GetRoute {
        hostname: String,
    },
    DeleteRoute {
        hostname: String,
    },
    SetProxyPort(u16),
    StartProxy(StartArgs),
}

#[derive(Debug, Serialize, Deserialize, Archive)]
pub(crate) enum Response {
    List {
        processes: Vec<ProcessInfo>,
    },
    ProcessInfo(ProcessInfo),
    Error {
        message: String,
    },
    Ok {
        message: String,
    },
    Routes {
        routes: Vec<(String, crate::routes::Route)>,
    },
    Route(Option<crate::routes::Route>),
    ProxyStarted {
        message: String,
    },
}

impl Request {
    pub async fn process(self, process_man: Arc<ProcessManager>) -> Response {
        let state_dir = crate::state::app_data_dir();

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
                return Response::Ok {
                    message: format!("Route {} added successfully!", hostname),
                };
            }
            Request::ListRoutes => {
                let routes = APP_STATE.routes.list();
                return Response::Routes { routes };
            }
            Request::DeleteRoute { hostname } => {
                APP_STATE.routes.remove(&hostname);
                return Response::Ok {
                    message: format!("Route {} deleted successfully!", hostname),
                };
            }
            Request::SetProxyPort(port) => {
                APP_STATE.set_proxy_port(port);
                return Response::Ok {
                    message: format!("Proxy port set to {} successfully!", port),
                };
            }
            Request::GetRoute { hostname } => {
                let route = APP_STATE.routes.get(hostname.as_str());
                return Response::Route(route);
            }
            Request::StartProxy(start_args) => {
                let proxy_port = start_args.get_port();
                let https = start_args.get_use_https();
                let mut stdout = String::new();

                let is_running = is_proxy_running(proxy_port, https).await;

                if is_running {
                    let proxy_port = proxy_port.unwrap_or_else(|| get_default_proxy_port());
                    let sudo_pfix = if proxy_port < 1024 { "sudo" } else { "" };
                    let port_flag = if proxy_port != get_default_proxy_port() {
                        format!(" --port {}", proxy_port)
                    } else {
                        String::new()
                    };

                    let first = format!("Proxy is already running on port {}", proxy_port).yellow();
                    let second = format!(
                        "To restart: {} deport proxy stop{} && {} deport proxy start{}",
                        sudo_pfix, port_flag, sudo_pfix, port_flag
                    )
                    .blue();
                    let stdout = format!("{}\n{}\n", first, second);
                    return Response::Ok { message: stdout };
                }
                if proxy_port.is_some() && proxy_port.unwrap() < 1024 {
                    let pp = proxy_port.unwrap_or_else(get_default_proxy_port);
                    let stdout = format!(
                        "\n{}\n{}\n{}\n{}\n{}\n",
                        format!("Error: Port {} requires sudo.", pp).bright_red(),
                        format!("{}", "Either run with sudo:"),
                        format!("{}", "e.g.: sudo deport proxy start -p 443 --https".blue()),
                        format!("{}", "..or use default port (doesn't require sudo)".blink()),
                        format!("{}", "e.g.: deport proxy start".blue())
                    );
                    return Response::Error { message: stdout };
                }

                let ca_path = state_dir.join("ca.crt");
                if !is_ca_trusted(&ca_path).unwrap_or(false) {
                    stdout.push_str(&format!(
                        "{}\n{}\n{}\n",
                        "CA not installed in system trust store, so browsers may show certificate errors/warnings.".yellow(),
                        "Add it by running:".yellow(),
                        "deport trust".blue()
                    ));
                }

                stdout.push_str("Starting deported proxy daemon...\n");

                #[cfg(windows)]
                if let Err(e) = daemon::start(proxy_port) {
                    return Response::Error { message: format!("Failed to start daemon: {}", e) };
                }

                #[cfg(unix)]
                if let Err(e) = daemon::start(&state_dir, proxy_port) {
                    return Response::Error { message: format!("Failed to start daemon: {}", e) };
                }

                return Response::ProxyStarted { message: stdout };
            }
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
