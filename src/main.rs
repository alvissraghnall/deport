use std::{sync::{atomic::AtomicU16, Arc, LazyLock}};

use dashmap::DashMap;
use colored::Colorize;

use crate::{commands::{Arguments, get::handle_get, list::handle_list, proxy::{handle_proxy_command, ProxyCommands, StartArgs}, run::handle_run, trust::handle_trust}, process_man::ProcessManager, proxy::is_proxy_running, routes::RouteManager, sni::load_ca, state::{ProxyState, app_data_dir, init_app_storage}};

mod process_man;

mod routes;

mod sni;

mod proxy;

mod ipc;

mod certs;

mod trust_ca;

mod state;

mod daemon;

mod cli_utils;

mod commands;

// static ROUTES_MANAGER: LazyLock<Arc<RouteManager>> = LazyLock::new(|| Arc::new(routes::RouteManager::new()));

pub(crate) static APP_STATE: LazyLock<Arc<ProxyState>> = LazyLock::new(|| {
    init_app_storage().unwrap();
    let state_dir = app_data_dir();
    let (ca_cert, ca_key) = load_ca(&state_dir).expect("CA Certificates should be installed!");

    Arc::new(ProxyState {
        routes: Arc::from(RouteManager::new()),
        tls_cache: DashMap::new(),
        ca_cert,
        ca_key,
        proxy_port: AtomicU16::new(0),
    })
});

pub(crate) static PROCESS_MANAGER: LazyLock<Arc<ProcessManager>> = LazyLock::new(|| Arc::new(ProcessManager::new()));

fn main() -> anyhow::Result<()> {

    #[cfg(unix)]
    let addr = "/tmp/deport.sock";

    #[cfg(windows)]
    let addr = r"\\.\pipe\deport";

    let args = <Arguments as clap::Parser>::parse();

    match args.command {
        commands::Commands::Proxy(cmd) => {
            let _ = handle_proxy_command(&cmd);
        }
        commands::Commands::Run(mut run_args) => {
            let is_running = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(is_proxy_running(run_args.get_proxy_port(), Some(true)));

            if !is_running {
                println!("{}", "Proxy not running, starting it...".yellow());
                let proxy_start_args = StartArgs::new(run_args.get_proxy_port(), Some(true));
                if let Err(e) = handle_proxy_command(&ProxyCommands::Start(proxy_start_args)) {
                    eprintln!("{}", format!("Failed to start proxy: {}", e).red());
                    std::process::exit(1);
                }
                println!("{}", "Proxy started. Waiting for it to initialize...".yellow());
                std::thread::sleep(std::time::Duration::from_millis(1500));
            } else {
                println!("{}", "Proxy is already running...".yellow());
            }

            if let Err(e) = tokio::runtime::Runtime::new().unwrap().block_on(async {
                handle_run(addr, &mut run_args).await
            }) {
                eprintln!("{}", format!("Error: {}", e).red());
                std::process::exit(1);
            }
        },
        commands::Commands::Hosts => todo!(),
        commands::Commands::Trust => handle_trust()?,
        commands::Commands::List => {
            if let Err(e) = tokio::runtime::Runtime::new().unwrap().block_on(async {
                handle_list(addr).await
            }) {
                eprintln!("{}", format!("Error: {}", e).red());
                std::process::exit(1);
            }
        },
        commands::Commands::Stab => todo!(),
        commands::Commands::Get(get_args) => {
            if let Err(e) = tokio::runtime::Runtime::new().unwrap().block_on(async {
                handle_get(addr, &get_args).await
            }) {
                eprintln!("{}", format!("Error: {}", e).red());
                std::process::exit(1);
            }
        },
    }

    Ok(())
}
