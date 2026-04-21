use std::{env::current_dir, io, str::FromStr};

use anyhow::{Context, Result, bail};
use clap::Args;
use colored::Colorize as _;
use rand::RngExt as _;

use crate::{
    cli_utils::{inject_framework_flags}, commands::proxy::StartArgs, ipc::{Request, Response, client::IpcClient}, process_man::{ProcessConfig, get_default_proxy_port, get_free_port}, proxy::is_proxy_running, routes::{Route}
};

/// Parse a single key-value pair
fn parse_key_val<T: FromStr, U: FromStr>(input: &str) -> Result<Vec<(T, U)>>
where
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
    <U as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    input
        .trim()
        .split(',')
        .map(|item| {
            let idx = item
                .find('=')
                .with_context(|| format!("invalid KEY=value: no `=` found in `{}`", input))?;
            Ok((input[..idx].parse()?, input[idx + 1..].parse()?))
        })
        .collect::<Result<Vec<(T, U)>>>()
}

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    /// Process Command to run
    cmd: String,

    /// args for the process command
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,

    /// name for subdomain
    #[arg(short, long)]
    name: Option<String>,

    #[arg(short, long)]
    force: bool,

    #[arg(short, long)]
    port: Option<u16>,

    #[arg(long)]
    proxy_port: Option<u16>,

    #[arg(short, long, value_name="NAME=VALUE", value_parser = parse_key_val::<String, String>)]
    env: Option<std::vec::Vec<(String, String)>>,
}

pub async fn handle_run(addr: &str, args: &mut RunArgs) -> Result<()> {
    let base_name: String;

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
            println!(
                "{}",
                "Start the proxy first (requires sudo for this port):".blue()
            );
            println!("{}", "    sudo deport proxy start -p 80   ".cyan());
            println!("{}", "Or use the default port (no sudo needed):".blue());
            println!("{}", "    deport proxy start   ".cyan());
            std::process::exit(1);
        } else {
            println!("{}", "Starting proxy...".yellow());
            let proxy_start_args = StartArgs::new(args.proxy_port, Some(true));

            match crate::commands::proxy::handle_proxy_command(
                &crate::commands::proxy::ProxyCommands::Start(proxy_start_args),
            ) {
                Ok(()) => {
                    println!("{}", "Proxy started! Waiting for it to initialize...".yellow());
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                }
                Err(e) => {
                    println!("{}", "Failed to start proxy!".bright_red());
                    println!("{}", "Start the proxy manually first: ".blue());
                    println!("{}", "    deport proxy start   ".cyan());
                    return Err(e);
                }
            }
        }
    } else {
        println!("{}", "Proxy is already running...".yellow());
    }

    let mut client = IpcClient::connect(addr).await
        .context("Failed to connect to deport daemon. Is it running? Try `deport proxy start`.")?;

    if args.port.is_some() {
        println!("{}", format!("Using custom port: {}", args.port.unwrap()).bright_green())
    }

    let port = match args.port {
        Some(p) => p,
        None => get_free_port().ok_or_else(|| {
            io::Error::new(io::ErrorKind::AddrInUse, "No free ports available")
        })?,
    };

    inject_framework_flags(&args.cmd, &mut args.args, port)?;

    println!("{}", format!("Running PORT={} HOST={} {:?}", port, "0.0.0.0", args.cmd.as_str().to_owned() + " " + &args.args.join(" ")).bright_cyan());
    
    let config = ProcessConfig {
        name: base_name.clone(),
        command: args.cmd.clone(),
        args: args.args.clone(),
        port: Some(port),
        env: args.env.take().unwrap_or_default(),
        cwd: current_dir()?.to_str().map(str::to_string),
    };

    let request = Request::Spawn { config };

    let response = IpcClient::send_request(&mut client, request).await?;
    
    if let Response::ProcessInfo(process_info) = response {
        println!("{}", format!("Process {} now running on port {} with PID: {}", process_info.name, process_info.port, process_info.pid).blue());
        let route = Route {
            pid: process_info.pid,
            port: process_info.port,
        };
        
        let host = format!("{}.localhost", base_name.as_str());

        let insert_request = Request::AddRoute { hostname: host.clone(), route };
        let insert_response = IpcClient::send_request(&mut client, insert_request).await?;

        if let Response::Ok { message } = insert_response {
            println!("{}", message.green());
        } else {
            println!("{}", "Failed to add route!".bright_red());
        }

        println!("{}", format!("Route added for {}.localhost -> {}", base_name.as_str(), process_info.port).green());
    }
    else if let Response::Error { message } = response {
        println!("{}", format!("Failed to start process: {}", message).bright_red());
    } else {
        println!("{}", "Unexpected response from daemon".bright_red());
    }
    Ok(())
}
