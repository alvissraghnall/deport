use std::{env::current_dir, io, str::FromStr, sync::Arc};

use anyhow::{Context, Result, bail};
use clap::Args;
use colored::Colorize as _;
use rand::RngExt as _;

use crate::{
    cli_utils::{inject_framework_flags}, commands::proxy::StartArgs, ipc::{ClientIpcStream, Request, Response, client::IpcClient}, process_man::{ProcessConfig, get_default_proxy_port, get_free_port}, proxy::is_proxy_running, routes::{Route, RouteManager}
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

    #[arg(short, long, value_name="NAME=VALUE", value_parser = parse_key_val::<String, String>)]
    env: Option<std::vec::Vec<(String, String)>>,
}

pub async fn handle_run(args: &mut RunArgs, client: &mut ClientIpcStream, routes_manager: &RouteManager) -> Result<()> {
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
            )
            .await
            {
                Ok(()) => {
                    println!("{}", "Proxy started!".green());
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
    
    // let cmd_env = match &mut args.env {
    //     Some(args) => {
    //         let mut _args = Vec::new();
    //         _args.push("HOST=127.0.0.1".to_string());
    //         _args.push(format!("PORT={}", port));
    //         args.iter().map(|v| _args.push(v.clone()));
    //         Some(_args)
    //     },
    //     None => {
    //         let mut _args = Vec::new();
    //         _args.push("HOST=127.0.0.1".to_string());
    //         _args.push(format!("PORT={}", port));
    //         args.env.replace(_args)
    //     },
    // };
    
    let config = ProcessConfig {
        name: base_name.clone(),
        command: args.cmd.clone(),
        args: args.args.clone(),
        port: Some(port),
        env: args.env.take().unwrap_or_default(),
        cwd: current_dir()?.to_str().map(str::to_string),
    };

    let request = Request::Spawn { config };
    
    let response = IpcClient::send_request(client, request).await?;
    
    if let Response::ProcessInfo(process_info) = response {
        println!("{}", format!("Process {} now running on port {} with PID: {}", process_info.name, process_info.port, process_info.pid).blue());
        let route = Route {
            pid: process_info.pid,
            port: process_info.port,
        };
        
        let host = format!("{}.localhost", base_name.as_str());
        routes_manager.insert(Arc::from(host), route);
    }
    // let final_url = format_url(&base_name, args.proxy_port, true);

    Ok(())
}
