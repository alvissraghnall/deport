#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_family = "unix")]
mod unix;

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_family = "unix")]
pub use unix::*;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

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