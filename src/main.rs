use std::{io, sync::{Arc, LazyLock}};

use dashmap::DashMap;

use crate::{commands::{Arguments, run::handle_run}, ipc::ClientIpcStream, process_man::ProcessManager, routes::RouteManager, sni::load_ca, state::{ProxyState, app_data_dir, init_app_storage}};

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
    })
});

pub(crate) static PROCESS_MANAGER: LazyLock<Arc<ProcessManager>> = LazyLock::new(|| Arc::new(ProcessManager::new()));

fn main() -> anyhow::Result<()> {
    let state_dir = app_data_dir();

    #[cfg(unix)]
    let addr = "/tmp/deport.sock";

    #[cfg(windows)]
    let addr = r"\\.\pipe\deport";

    let args = <Arguments as clap::Parser>::parse();

    match args.command {
        commands::Commands::Proxy => {
            println!("Starting deported proxy daemon...");
            
            #[cfg(windows)]
            daemon::start()?;
            
            #[cfg(unix)]
            daemon::start(&state_dir)?;
        },
        commands::Commands::Run(run_args) => {
            let stream: io::Result<ClientIpcStream> = tokio::runtime::Runtime::new().unwrap().block_on(async {
                ipc::client::IpcClient::connect(addr).await
            });
            // let resp = ipc::client::IpcClient::send_request(stream, Request::Spawn {...}).await?;
            let _ = handle_run(&run_args, &stream?);
        },
        commands::Commands::Hosts => todo!(),
        commands::Commands::Trust => todo!(),
        commands::Commands::List => todo!(),
        commands::Commands::Stab => todo!(),
    }

    Ok(())
}
