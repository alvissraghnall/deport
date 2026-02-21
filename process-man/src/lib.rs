use std::net::{Ipv4Addr, TcpListener};
use std::process::Stdio;

const DEFAULT_PROXY_PORT: u16 = 1999;

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

pub fn spawn(command: &str, args: &[String], port: u16) -> tokio::process::Child {
    let child = tokio::process::Command::new(command)
        .args(args) 
        .env("PORT", port.to_string()) 
        .stdout(Stdio::piped()) 
        .stderr(Stdio::piped()) 
        .spawn()
        .expect("Failed to spawn child process");

    child
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[tokio::test]
    async fn test_spawn_and_capture() {
        let args = vec!["6".to_string()];

        let mut child = spawn("sleep", &args, 1999);

        // Verify we can capture stderr and stdout handles
        // Since 'sleep' is quiet, stderr will be empty, but the handle must exist.
        let mut stderr = child.stderr.take().expect("Failed to capture stderr");
        let mut stdout = child.stdout.take().expect("Failed to capture stdout");

        let status = child.wait().await.expect("Failed to wait for child");
        
        assert!(status.success(), "Process did not exit successfully");

        // Verify we can read from the pipes (even if empty)
        let mut out_str = String::new();
        let mut err_str = String::new();

        let mut stdout_reader = BufReader::new(&mut stdout).lines();
        while let Some(line) = stdout_reader.next_line().await.unwrap() {
            out_str.push_str(&line);
        }

        let mut stderr_reader = BufReader::new(&mut stderr).lines();
        while let Some(line) = stderr_reader.next_line().await.unwrap() {
            err_str.push_str(&line);
        }

        println!("Exit: {:?}, Out: '{}', Err: '{}'", status, out_str, err_str);
    }
}