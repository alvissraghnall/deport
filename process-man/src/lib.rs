use std::net::{Ipv4Addr, TcpListener};
use std::process::Stdio;
use tokio::sync::oneshot::channel;
use tokio::io::AsyncReadExt as _;

const DEFAULT_PROXY_PORT: u16 = 1999;

struct Process {
    name: String,
    child: tokio::process::Child,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    state: ProcessState,
    pid: u32,
}

enum ProcessState {
    Running,
    Exited,
    Failed,
    Stopped,
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

pub async fn spawn(command: &str, args: &[String], port: u16) -> Process {
    let (_tx, rx) = channel::<()>();
    let mut child = tokio::process::Command::new(command)
        .args(args) 
        .env("PORT", port.to_string()) 
        .stdout(Stdio::piped()) 
        .stderr(Stdio::piped()) 
        .stdin(Stdio::null())
        .spawn()
        .expect("Failed to spawn child process. Enter a valid command and arguments.");

    let mut stdout = child.stdout.take().expect("stdout is not captured");
    let mut stderr = child.stderr.take().expect("stderr is not captured");

    let read_stdout = tokio::spawn(async move {
        let mut buff = Vec::new();
        let _ = stdout.read_to_end(&mut buff).await;

        buff
    });

    let read_stderr = tokio::spawn(async move {
        let mut buff = Vec::new();
        let _ = stderr.read_to_end(&mut buff).await;

        buff
    });

    

    tokio::select! {
        _ = child.wait() => {}
        _ = rx => { child.kill().await.expect("kill failed") },
    }

    let stdout = read_stdout.await.unwrap();
    let stderr = read_stderr.await.unwrap();

    assert!(stderr.is_empty(), "Expected stderr to be empty, got: {}", String::from_utf8_lossy(&stderr));

    let process = Process {
        name: format!("{} {}", command, args[0]),
        pid: child.id().unwrap_or(0),
        child,
        stdout: stdout,
        stderr,
        state: ProcessState::Running,
    };
    process
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[tokio::test]
    async fn test_spawn_and_capture() {
        let args = vec!["7".to_string()];

        let mut process = spawn("sleep", &args, 1999).await;
        println!("Process PID: {}, child pid: {}", process.pid, process.child.id().unwrap_or(0));

        let status = process.child.wait().await.expect("Failed to wait for child");
        
        assert!(status.success(), "Process did not exit successfully");

        println!("Exit: {:?}, Out: '{}', Err: '{}'", status, unsafe { String::from_utf8_unchecked(process.stdout) }, unsafe { String::from_utf8_unchecked(process.stderr) });
    }
}