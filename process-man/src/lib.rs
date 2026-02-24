use std::intrinsics::AtomicOrdering;
use std::net::{Ipv4Addr, TcpListener};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, watch};
use tokio::sync::oneshot::channel;

const DEFAULT_PROXY_PORT: u16 = 1999;

#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub port: Option<u16>, // None means "find one for me"
}

pub struct Process {
    pub name: String,
    // child: tokio::process::Child,
    // stdout: tokio::task::JoinHandle<String>,
    // stderr: tokio::task::JoinHandle<String>,

    pub stdout: Arc<Mutex<String>>,
    pub stderr: Arc<Mutex<String>>,

    pub pid: u32,
    pub port: u16,
    config: ProcessConfig,
    inner: Arc<ProcessInner>,
}

enum ProcessState {
    Running,
    Exited,
    Failed(Option<i32>),
    Stopped,

}
struct ProcessInner {
    state: Mutex<ProcessState>,
    // A channel to signal the watcher task to stop 
    stop_tx: watch::Sender<bool>,
}


fn get_default_proxy_port() -> u16 {
    let port_str = std::env::var("PROXY_PORT").unwrap_or_else(|_| DEFAULT_PROXY_PORT.to_string());
    let port = port_str.parse().unwrap_or(DEFAULT_PROXY_PORT);
    // Validate port range (system ports 0-1023 are usually restricted)
    if port < 1024 || port >= 65535 {
        eprintln!(
            "Invalid proxy port: {}. Using default: {}",
            port, DEFAULT_PROXY_PORT
        );
        return DEFAULT_PROXY_PORT;
    }
    port
}

fn find_free_port() -> Option<u16> {
    fn try_bind_port(port: u16) -> bool {
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
    }

    // Try a sparse check first ( 1024, 1124, 1224...)
    for i in 0..60 {
        let port = 1024 + (i * 100);
        if try_bind_port(port) {
            return Some(port);
        }
    }

    // ...or, exhaustive check
    for port in 1024..=65535 {
        if port == get_default_proxy_port() {
            continue;
        }
        if try_bind_port(port) {
            return Some(port);
        }
    }
    None
}

impl Process {
    pub async fn spawn(
        config: ProcessConfig,
    ) -> std::io::Result<Self> {

        let command = &config.command;
        let mut cmd = Command::new(command);
        
        #[cfg(unix)]
        cmd.process_group(0); 

        let args = config.args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        let port = match config.port {
            Some(p) => p,
            None => find_free_port().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::AddrInUse, "No free ports found")
            })?,
        };
        let mut child = cmd
            .args(args)
            .env("PORT", port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("Failed to spawn child process.");

        // capture PID while it is running
        let pid = child.id().unwrap_or(0);

        let (stop_tx, mut stop_rx) = watch::channel(false);
        let inner = Arc::new(ProcessInner {
            state: Mutex::new(ProcessState::Running),
            stop_tx,
        });

        if let Some(stdout) = child.stdout.take() {
            let name = config.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    println!("[{}] OUT: {}", name, line);
                }
            });
        }


        if let Some(stderr) = child.stderr.take() {
            let name = config.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[{}] ERR: {}", name, line);
                }
            });
        }

        let inner_clone = inner.clone();
        tokio::spawn(async move {
            let mut child = child;
            
            // Wait for death OR stop signal
            tokio::select! {
                status = child.wait() => {
                    // Process died naturally
                    let mut state = inner_clone.state.lock().await;
                    *state = match status {
                        Ok(s) if s.success() => ProcessState::Exited,
                        Ok(s) => ProcessState::Failed(s.code()),
                        Err(_) => ProcessState::Failed(None),
                    };
                }
                _ = stop_rx.changed() => {
                    // We were asked to stop
                    // SIGTERM -> Sleep -> SIGKILL here
                    let _ = child.kill().await;
                }
            }
        });

        Ok(Process {
            name: config.name.clone(),
            port,
            pid,
            config,
            inner,
        })
    }

    // pub fn is_running(&mut self) -> bool {
    //     matches!(self.child.try_wait(), Ok(None))
    // }

    // pub fn status(&mut self) -> Option<std::process::ExitStatus> {
    //     self.child.try_wait().ok().flatten()
    // }

    // pub async fn kill(&mut self) {
    //     let _ = self.child.kill().await;
    // }

    pub async fn restart (&mut self) -> std::io::Result<()> {
        // self.kill().await;
        let new_process = Process::spawn(self.config.clone()).await?;
        *self = new_process;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::AsyncReadExt, net::TcpStream};

    async fn is_port_open(port: u16) -> bool {
        TcpStream::connect(("127.0.0.1", port)).await.is_ok()
    }

    #[tokio::test]
    async fn test_spawn_and_capture() {
        let args = vec!["2".to_string()];

        let mut process = Process::spawn(ProcessConfig {
            name: "test".to_string(),
            command: "sleep".to_string(),
            args,
            port: Some(1999),
        }).await.expect("Failed to spawn process");
        println!("Child pid: {}", process.pid);

        assert!(process.pid > 0);

        let status = process
            .child
            .wait()
            .await
            .expect("Failed to wait for child");

        assert!(status.success(), "Process did not exit successfully");

        println!(
            "Exit: {:?}",
            status,
        );
    }

    #[tokio::test]
    async fn test_pid_is_assigned_and_non_zero() {
        let args: Vec<String> = vec!["-c".to_string(), "exit 0".to_string()];
        let mut proc = Process::spawn(ProcessConfig {
            name: "test".to_string(),
            command: "sh".to_string(),
            args,
            port: Some(9999),
        }).await.expect("Failed to spawn process");

        assert!(proc.pid > 0, "PID should be greater than 0");

        let _ = proc.child.wait().await;
    }

    #[tokio::test]
    async fn test_env_var_injection() {
        // Command: print the PORT env var to stderr
        let args = vec!["-c".to_string(), "echo $PORT >&2".to_string()];
        let target_port = 5555;

        let mut proc = Process::spawn(ProcessConfig {
            name: "test".to_string(),
            command: "sh".to_string(),
            args,
            port: Some(target_port),
        }).await.expect("Failed to spawn process");

        // Wait for process to finish so buffers are flushed
        let _ = proc.child.wait().await;

        // Await the stderr handle to get the captured text
        // let output = proc.child.stderr.expect("Failed to get stderr");

        // Verify the content
        // assert_eq!((), target_port.to_string());
    }

    #[tokio::test]
    async fn test_process_actually_listens() {
        let port = find_free_port().expect("No free port");

        assert!(
            is_port_open(port).await == false,
            "Port should be free before spawn"
        );
    }
}
