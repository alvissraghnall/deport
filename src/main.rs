use std::{sync::{Arc, LazyLock}};

use dashmap::DashMap;
use futures::executor;

use crate::{commands::{Arguments, get::handle_get, list::handle_list, proxy::handle_proxy_command, run::handle_run, trust::handle_trust}, ipc::ClientIpcStream, process_man::ProcessManager, routes::RouteManager, sni::load_ca, state::{ProxyState, app_data_dir, init_app_storage}};

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
    let routes_manager_clone = APP_STATE.routes.clone();

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
            let _: anyhow::Result<()> = tokio::runtime::Runtime::new().unwrap().block_on(async {
                let stream = ipc::client::IpcClient::connect(addr).await;
                handle_run(&mut run_args, &mut stream?, &routes_manager_clone).await
            });
            // let resp = ipc::client::IpcClient::send_request(stream, Request::Spawn {...}).await?;
            // let stream: std::io::Result<ClientIpcStream> = tokio::runtime::Runtime::new().unwrap().block_on(async {
            //     ipc::client::IpcClient::connect(addr).await
            // });
            // match stream {
            //     Ok(sni) => println!("{:?}", sni),
            //     Err(e) => print!("{:?}", e),
            // }
            // let _ = executor::block_on(handle_run(&mut run_args, &mut stream?, &routes_manager_clone));
        },
        commands::Commands::Hosts => todo!(),
        commands::Commands::Trust => handle_trust()?,
        commands::Commands::List => handle_list(&routes_manager_clone, true)?,
        commands::Commands::Stab => todo!(),
        commands::Commands::Get(get_args) => handle_get(&get_args, &routes_manager_clone)?,
    }

    Ok(())
}
