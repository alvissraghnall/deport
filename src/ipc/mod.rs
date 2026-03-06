#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_family = "unix")]
mod unix;

use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_family = "unix")]
pub use unix::*;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::process_man::{Process, ProcessConfig, ProcessInfo};

trait IpcListenerTrait {
    async fn bind(addr: &str) -> std::io::Result<Self>
    where
        Self: Sized;

    async fn accept(&self) -> std::io::Result<IpcStream>;

    async fn handle_new_messages(&mut self) -> std::io::Result<()>;
}

async fn run() -> std::io::Result<()> {
    #[cfg(unix)]
    let addr = "/tmp/mydaemon.sock";

    #[cfg(windows)]
    let addr = r"\\.\pipe\mydaemon";

    let listener = IpcListener::bind(addr).await?;

    loop {
        let mut stream = listener.accept().await?;

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];

            loop {
                let n = match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };

                let _ = stream.write_all(&buf[..n]).await;
            }
        });
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Request {
    Spawn {
        name: String,
        command: String,
        args: Vec<String>,
        port: Option<u16>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
    },
    List,
    Stop {
        name: String,
    },
    KillDaemon,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Response {
    List { processes: Vec<ProcessInfo> },
    ProcessInfo(Process),
    Error { message: String },
    Ok { message: String },
}

impl Request {
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub async fn process(self) -> Response {
        match self {
            Request::Spawn {
                name,
                command,
                args,
                port,
                env,
                cwd,
            } => {
                let config = ProcessConfig {
                    name,
                    command,
                    args,
                    port,
                    env: vec![],
                    cwd: None,
                };
                
                match Process::spawn(config).await {
                    Ok(info) => Response::ProcessInfo(info),
                    Err(e) => Response::Error {
                        message: e.to_string(),
                    },
                }
            }
            Request::List => match crate::process_man::list_processes().await {
                Ok(processes) => Response::List { processes },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::Stop { name } => match crate::process_man::stop_process(name).await {
                Ok(()) => Response::Ok {
                    message: "Process stopped successfully".into(),
                },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::KillDaemon => {
                crate::process_man::kill_daemon().await;
                Response::Ok {
                    message: "Daemon killed successfully".into(),
                }
            }
        }
    }
}
