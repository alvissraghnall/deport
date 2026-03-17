use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use colored::Colorize as _;

use crate::daemon;
use crate::{process_man::get_default_proxy_port, proxy::{is_proxy_running, stop_proxy}, trust_ca::{is_ca_trusted}};

#[derive(Subcommand, Debug)]
pub enum ProxyCommands {
    /// Start the proxy
    Start(StartArgs),

    /// Stop the proxy
    Stop(StopArgs),
}

#[derive(Args, Debug)]
pub struct StartArgs {
    #[arg(short, long)]
    port: Option<u16>,

    #[arg(short, long)]
    use_https: Option<bool>,
}

#[derive(Args, Debug)]
pub struct StopArgs {
    #[arg(short, long)]
    force: bool,
}

impl StartArgs {
    pub fn new (port: Option<u16>, use_https: Option<bool>) -> Self {
        Self {
            port,
            use_https,
        }
    }
}

impl StopArgs {
    pub fn new () -> Self {
        Self {
            force: false,
        }
    }
}

pub async fn handle_proxy_command(cmd: &ProxyCommands) -> Result<()> {
    let state_dir = crate::state::app_data_dir();
    match cmd {
        ProxyCommands::Start(start_args) => {
            let proxy_port = start_args.port;
            let https = start_args.use_https;

            if is_proxy_running(proxy_port, https).await {
                let proxy_port = proxy_port.unwrap_or_else(|| get_default_proxy_port());
                let sudo_pfix = if proxy_port < 1024 { "sudo" } else { "" };
                let port_flag = if proxy_port != get_default_proxy_port() {
                    format!(" --port {}", proxy_port)
                } else {
                    String::new()
                };

                println!("{}", format!("Proxy is already running on port {}", proxy_port).bright_yellow());
                println!("{}", format!("To restart: {} deport proxy stop{} && {} deport proxy start{}", sudo_pfix, port_flag, sudo_pfix, port_flag).blue());
                bail!("Proxy is already running on port {}", proxy_port);
            }

            if proxy_port.is_some() && proxy_port.unwrap() < 1024 {
                let pp = proxy_port.unwrap_or_else(get_default_proxy_port);
                println!("{}", format!("Error: Port {} requires sudo.", pp).bright_red());
                println!("{}", "Either run with sudo:");
                println!("{}", "e.g.: sudo deport proxy start -p 443 --https".blue());
                println!("{}", "..or use default port (doesn't require sudo)".blink());
                println!("{}", "e.g.: deport proxy start".blue());
                bail!("Port {} requires sudo.", pp);
            }

            let ca_path = state_dir.join("ca.crt");
            if !is_ca_trusted(&ca_path)? {
                println!("{}", "CA not installed in system trust store, so browsers may show certificate errors/warnings.".yellow());
                println!("{}", "Add it by running:".yellow());
                println!("{}", "deport trust".blue());
            }

            println!("Starting deported proxy daemon...");

            #[cfg(windows)]
            daemon::start(proxy_port)?;

            #[cfg(unix)]
            daemon::start(&state_dir, proxy_port)?;
        }
        ProxyCommands::Stop(_) => {
            stop_proxy();
        }
    }

    Ok(())
}
