use std::{ops::Deref, sync::{Arc, LazyLock, OnceLock}};

use dashmap::DashMap;
use rama::proxy::Proxy;
use tokio::sync::mpsc;

use crate::{commands::{Arguments, run::handle_run}, ipc::worker::{WorkItem, worker_loop}, process_man::ProcessManager, routes::RouteManager, sni::load_ca, state::{ProxyState, SharedState, app_data_dir, init_app_storage}};

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

static ROUTES_MANAGER: LazyLock<Arc<RouteManager>> = LazyLock::new(|| Arc::new(routes::RouteManager::new()));

static APP_STATE: LazyLock<Arc<ProxyState>> = LazyLock::new(|| {
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

static PROCESS_MANAGER: LazyLock<Arc<ProcessManager>> = LazyLock::new(|| Arc::new(ProcessManager::new()));

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state_dir = app_data_dir();

    let args = <Arguments as clap::Parser>::parse();

    #[cfg(windows)]
    {
        daemon::start()?
    }
    
    #[cfg(unix)]
    {
        daemon::start(&state_dir)
    }

    println!("Hello, world!");
    let state = APP_STATE.clone();
    let process_manager = PROCESS_MANAGER.clone();

    state.routes
        .insert(
            "localhost".into(),
            routes::Route {
                port: 8000,
                pid: 1234,
            },
        );


    tokio::spawn(async {
        proxy::start_proxy(state).await;
    })
    .await?;

    let (tx, rx) = mpsc::channel::<WorkItem>(100);

    tokio::spawn(worker_loop(rx, process_manager));

    ipc::run(tx).await?;

    match args.command {
        commands::Commands::Run(run_args) => {
            handle_run(run_args);
        },
        commands::Commands::Hosts => todo!(),
        commands::Commands::Trust => todo!(),
        commands::Commands::Proxy => todo!(),
        commands::Commands::List => todo!(),
        commands::Commands::Stab => todo!(),
    }

    Ok(())
}
