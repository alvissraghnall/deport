use anyhow::{Result, bail};
use clap::Args;
use colored::Colorize as _;
use rand::RngExt as _;

use crate::{
    ipc::{ClientIpcStream, client::IpcClient},
    process_man::get_default_proxy_port,
    proxy::is_proxy_running,
};

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Process Command to run
    cmd: String,

    /// args the process
    args: Vec<String>,

    /// name for subdomain
    #[arg(short, long)]
    name: Option<String>,

    #[arg(short, long)]
    force: bool,

    #[arg(short, long)]
    port: Option<u16>,

    #[arg(short, long)]
    proxy_port: Option<u16>,

    #[arg(short, long)]
    env: Option<Vec<String>>,
}

pub async fn handle_run(args: &RunArgs, client: &ClientIpcStream) -> Result<()> {
    let base_name: String;

    if args.args.is_empty() {
        return Ok(());
    }

    if let Some(name) = &args.name {
        let sanitized = crate::cli_utils::sanitize_rfc1035(&name);
        if sanitized.is_empty() {
            bail!("Invalid name: {}", name);
        }
        base_name = sanitized;
    } else {
        base_name = std::env::current_dir()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| crate::cli_utils::sanitize_rfc1035(s))
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                use rand::distr::Alphanumeric;

                let mut rng = rand::rng();
                let rand_name: String = (&mut rng)
                    .sample_iter(Alphanumeric)
                    .take(7)
                    .map(char::from)
                    .collect();

                format!("deported-{}", rand_name.to_lowercase())
            });
    }

    if !is_proxy_running(args.proxy_port, Some(true)).await {
        let proxy_port = args.proxy_port.unwrap_or(get_default_proxy_port());
        if proxy_port < 1024 {
            println!("{}", "Proxy is not running.".red());
            println!("{}", "Start the proxy first (requires sudo for this port):".blue());
            println!("{}", "    sudo deport proxy start -p 80   ".cyan());
            println!("{}", "Or use the default port (no sudo needed):".blue());
            println!("{}", "    deport proxy start   ".cyan());
            std::process::exit(1);
        } else {
            println!("{}", "Starting proxy...".yellow());

            // proxy start, i just realizrd i should've led with this lmfaooo
            // grr

        }
    }

    Ok(())
}
