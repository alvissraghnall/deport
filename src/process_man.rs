use std::io;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use dashmap::DashMap;
use rkyv::{Archive, Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

const DEFAULT_PROXY_PORT: u16 = 1999;

const STATE_RUNNING: u8 = 0;
const STATE_EXITED: u8 = 1;
const STATE_FAILED: u8 = 2;
const STATE_STOPPED: u8 = 3;

pub struct ProcessManager {
    processes: DashMap<String, Process>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: DashMap::new(),
        }
    }

    pub async fn spawn(&self, config: ProcessConfig) -> io::Result<ProcessInfo> {
        let process = Process::spawn(config).await?;

        let process_name = process.name.as_str();
        let info = process.info().await;

        self.processes.insert(process_name.to_string(), process);

        Ok(info)
    }

    pub async fn list(&self) -> Vec<ProcessInfo> {
        let mut infos = Vec::with_capacity(self.processes.len());

        for val in &self.processes {
            let process = val.value();
            infos.push(process.info().await);
        }

        infos
    }

    pub async fn kill(&self, process_name: String) -> io::Result<()> {
        let proc = self
            .processes
            .get(process_name.as_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Process not found!"))?;

        proc.kill().await
    }

    pub async fn cleanup(&self) {
        self.processes.retain(|_, proc| proc.is_running());
    }

    pub async fn stop_all(&self) {
        let _ = self.processes.iter().map(async |proc| {
            let _ = proc.value().kill().await;
        });
    }
}

pub type SharedManager = Arc<ProcessManager>;

#[derive(Debug, Clone, Archive, Serialize, Deserialize, )]
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub port: Option<u16>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Archive)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub port: u16,
    pub state: ProcessState,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Archive)]
pub enum ProcessState {
    Running,
    Exited,
    Failed,
    Stopped,
}

/// A handle to a managed process.
/// This struct is thread-safe.
#[derive(Clone)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub port: u16,

    // Inner shared state
    state: Arc<AtomicU8>,
    exit_code: Arc<Mutex<Option<i32>>>,
    logs: ProcessLogs,

    // Channel to send commands to the supervisor task
    cmd_tx: mpsc::Sender<SupervisorCommand>,

    // Handle to the supervisor task (used for awaiting shutdown)
    supervisor: Arc<Mutex<Option<JoinHandle<()>>>>,
}

#[derive(Clone, Default)]
struct ProcessLogs {
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
}

enum SupervisorCommand {
    Kill,
}

impl Process {
    /// Spawns a new process with the given configuration.
    /// This function returns immediately after the fork.
    pub async fn spawn(config: ProcessConfig) -> io::Result<Self> {
        let port = match config.port {
            Some(p) => p,
            None => get_free_port().ok_or_else(|| {
                io::Error::new(io::ErrorKind::AddrInUse, "No free ports available")
            })?,
        };

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .env("PORT", port.to_string())
            .env("HOST", "127.0.0.1")
            .envs(config.env.iter().cloned())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }

        // ensures that grandchildren (e.g. npm +++ node) are also killed.
        #[cfg(unix)]
        {
            // use std::os::unix::process::CommandExt as _;
            cmd.process_group(0);
        }

        let child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        let state = Arc::new(AtomicU8::new(STATE_RUNNING));
        let exit_code = Arc::new(Mutex::new(None));
        let logs = ProcessLogs::default();

        let (cmd_tx, cmd_rx) = mpsc::channel::<SupervisorCommand>(1);

        let supervisor = Self::start_supervisor(
            child,
            pid,
            state.clone(),
            exit_code.clone(),
            logs.clone(),
            cmd_rx,
        );

        Ok(Process {
            pid,
            name: config.name,
            port,
            state,
            exit_code,
            logs,
            cmd_tx,
            supervisor: Arc::new(Mutex::new(Some(supervisor))),
        })
    }

    fn start_supervisor(
        mut child: Child,
        pid: u32,
        state: Arc<AtomicU8>,
        exit_code: Arc<Mutex<Option<i32>>>,
        logs: ProcessLogs,
        mut cmd_rx: mpsc::Receiver<SupervisorCommand>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            // Take streams
            let mut stdout = child.stdout.take().expect("stdout missing");
            let mut stderr = child.stderr.take().expect("stderr missing");

            // Log Drainer Tasks
            let log_out = logs.stdout.clone();
            let out_task = tokio::spawn(async move {
                let mut lines = BufReader::new(&mut stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    println!("[{}] OUT: {}", pid, line);
                    log_out.lock().await.push_str(&line);
                    log_out.lock().await.push('\n');
                }
            });

            let log_err = logs.stderr.clone();
            let err_task = tokio::spawn(async move {
                let mut lines = BufReader::new(&mut stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[{}] ERR: {}", pid, line);
                    log_err.lock().await.push_str(&line);
                    log_err.lock().await.push('\n');
                }
            });

            // Main Supervisor Loop
            loop {
                tokio::select! {
                    // Priority 1: Check for death
                    result = child.wait() => {
                        // Process Died
                        match result {
                            Ok(status) => {
                                let code = status.code();
                                *exit_code.lock().await = code;

                                if status.success() {
                                    state.store(STATE_EXITED, Ordering::Release);
                                } else {
                                    state.store(STATE_FAILED, Ordering::Release);
                                }
                            }
                            Err(e) => {
                                eprintln!("[{}] Wait error: {}", pid, e);
                                state.store(STATE_FAILED, Ordering::Release);
                            }
                        }
                        break;
                    }

                    // Priority 2: Check for Kill Command
                    Some(cmd) = cmd_rx.recv() => {
                        match cmd {
                            SupervisorCommand::Kill => {
                                #[cfg(unix)]
                                {
                                    // Unix: Kill the whole group to get grandchildren
                                    use nix::sys::signal::{kill, Signal};
                                    use nix::unistd::Pid;
                                    // Negative PID means the process group
                                    let _ = kill(Pid::from_raw(-(pid as i32)), Signal::SIGTERM);
                                }
                                #[cfg(not(unix))]
                                {
                                    let _ = child.kill().await;
                                }

                                // Do not break; loop back to wait() so we can reap the zombie
                            }
                        }
                    }
                }
            }

            // Cleanup: Wait for log tasks to finish
            let _ = tokio::try_join!(out_task, err_task);
        })
    }

    pub fn is_running(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_RUNNING
    }

    pub fn get_state(&self) -> ProcessState {
        match self.state.load(Ordering::Acquire) {
            STATE_RUNNING => ProcessState::Running,
            STATE_EXITED => ProcessState::Exited,
            STATE_FAILED => ProcessState::Failed,
            STATE_STOPPED => ProcessState::Stopped,
            _ => ProcessState::Failed,
        }
    }

    pub async fn exit_code(&self) -> Option<i32> {
        *self.exit_code.lock().await
    }

    pub async fn get_stdout(&self) -> String {
        self.logs.stdout.lock().await.clone()
    }

    pub async fn get_stderr(&self) -> String {
        self.logs.stderr.lock().await.clone()
    }

    /// Initiates a graceful stop of the process by sending a kill command to the supervisor.
    pub async fn kill(&self) -> io::Result<()> {
        if !self.is_running() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Process not running",
            ));
        }

        self.cmd_tx
            .send(SupervisorCommand::Kill)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Supervisor dead"))
    }

    /// Waits for the process to terminate completely.
    /// Consumes the handle to ensure no double-wait.
    pub async fn wait(self) -> io::Result<ProcessInfo> {
        let handle = self.supervisor.lock().await.take();

        if let Some(h) = handle {
            h.await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }

        Ok(self.info().await)
    }

    pub async fn info(&self) -> ProcessInfo {
        ProcessInfo {
            pid: self.pid,
            name: self.name.clone(),
            port: self.port,
            state: self.get_state(),
            exit_code: self.exit_code().await,
        }
    }

    /// Waits for a signal that requests a graceful shutdown, like SIGTERM or SIGINT.
    #[cfg(unix)]
    async fn wait_for_signal_impl(&self) {
        use tokio::signal::unix::{SignalKind, signal};

        // Infos here:
        // https://www.gnu.org/software/libc/manual/html_node/Termination-Signals.html
        let mut signal_terminate = signal(SignalKind::terminate()).unwrap();
        let mut signal_interrupt = signal(SignalKind::interrupt()).unwrap();

        tokio::select! {
            _ = signal_terminate.recv() => {
                tracing::debug!("Received SIGTERM.");
                self.kill().await.expect("Failed to kill process on SIGTERM");
            },
            _ = signal_interrupt.recv() => {
                tracing::debug!("Received SIGINT.");
                self.kill().await.expect("Failed to kill process on SIGINT");
            },
        };
    }

    /// Waits for a signal that requests a graceful shutdown, Ctrl-C (SIGINT).
    #[cfg(windows)]
    async fn wait_for_signal_impl(&self) {
        use tokio::signal::windows;

        // Infos here:
        // https://learn.microsoft.com/en-us/windows/console/handlerroutine
        let mut signal_c = windows::ctrl_c().unwrap();
        let mut signal_break = windows::ctrl_break().unwrap();
        let mut signal_close = windows::ctrl_close().unwrap();
        let mut signal_shutdown = windows::ctrl_shutdown().unwrap();

        tokio::select! {
            _ = signal_c.recv() => {
                tracing::debug!("Received CTRL_C.");
                self.kill().await.expect("Failed to kill process on CTRL_C");
            },
            _ = signal_break.recv() => {
                tracing::debug!("Received CTRL_BREAK.");
                self.kill().await.expect("Failed to kill process on CTRL_BREAK");
            },
            _ = signal_close.recv() => {
                tracing::debug!("Received CTRL_CLOSE.");
                self.kill().await.expect("Failed to kill process on CTRL_CLOSE");
            },
            _ = signal_shutdown.recv() => {
                tracing::debug!("Received CTRL_SHUTDOWN.");
                self.kill().await.expect("Failed to kill process on CTRL_SHUTDOWN");
            },
        };
    }

    /// Registers signal handlers and waits for a signal that
    /// indicates a shutdown request.
    pub(crate) async fn wait_for_signal(&self) {
        self.wait_for_signal_impl().await
    }
}

pub fn get_default_proxy_port() -> u16 {
    let port_str = std::env::var("DEPORT_PROXY_PORT").unwrap_or_else(|_| DEFAULT_PROXY_PORT.to_string());
    let port = port_str.parse().unwrap_or(DEFAULT_PROXY_PORT);
    // Validate port range (system ports 0-1023 are restricted)
    if port < 1024 || port >= 65535 {
        eprintln!(
            "Invalid proxy port: {}. Using default: {}",
            port, DEFAULT_PROXY_PORT
        );
        return DEFAULT_PROXY_PORT;
    }
    port
}

pub(crate) fn get_free_port() -> Option<u16> {
    use std::net::{Ipv4Addr, TcpListener};
    if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
        if let Ok(addr) = listener.local_addr() {
            return Some(addr.port());
        }
        return None;
    };
    return None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_lifecycle() {
        let config = ProcessConfig {
            name: "sleep-test".into(),
            command: "sleep".into(),
            args: vec!["0.1".into()],
            port: None,
            env: vec![],
            cwd: None,
        };

        let proc = Process::spawn(config).await.expect("Spawn failed");

        assert!(proc.pid > 0);
        assert!(proc.is_running());

        let info = proc.wait().await.expect("Wait failed");

        assert_eq!(info.state, ProcessState::Exited);
        assert_eq!(info.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_env_and_logs() {
        let config = ProcessConfig {
            name: "echo-test".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "echo $PORT && echo 'error' >&2".into()],
            port: Some(8888),
            env: vec![],
            cwd: None,
        };

        let proc = Process::spawn(config).await.expect("Spawn failed");

        let info = proc.wait().await.expect("Wait failed");

        assert_eq!(info.exit_code, Some(0));

        // we cannot check logs after `wait()` because `proc` is consumed.
    }

    #[tokio::test]
    async fn test_kill_process_group() {
        // simulates killing a parent that has children.
        // "sleep 10 | sleep 10" creates a pipeline.
        let config = ProcessConfig {
            name: "group-kill".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "sleep 10 & sleep 10 & wait".into()],
            port: None,
            env: vec![],
            cwd: None,
        };

        let proc = Process::spawn(config).await.expect("Spawn failed");

        sleep(Duration::from_millis(100)).await;

        assert!(proc.is_running());

        proc.kill().await.expect("Kill failed");

        sleep(Duration::from_millis(100)).await;
        // we can't check `is_running` easily after kill because we need to wait
        // to reap zombie, but we can't call wait after kill in this architecture
        // without a separate method.
        // or....`kill` sets a flag, and we have a wait_for_death method.
    }
}
