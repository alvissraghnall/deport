use std::{ops::Deref, sync::{Arc, LazyLock, OnceLock}};

use dashmap::DashMap;
use rama::proxy::Proxy;

use crate::{routes::RouteManager, sni::load_ca, state::{ProxyState, SharedState, app_data_dir, init_app_storage}};

mod process_man;

mod routes;

mod sni;

mod proxy;

mod ipc;

mod certs;

mod trust_ca;

mod state;

static ROUTES_MANAGER: LazyLock<Arc<RouteManager>> = LazyLock::new(|| Arc::new(routes::RouteManager::new()));

static APP_STATE: LazyLock<Arc<ProxyState>> = LazyLock::new(|| {
    let state_dir = app_data_dir();
    let (ca_cert, ca_key) = load_ca(&state_dir).expect("CA Certificates should be installed!");

    Arc::new(ProxyState {
        routes: Arc::from(RouteManager::new()),
        tls_cache: DashMap::new(),
        ca_cert,
        ca_key,
    })
});

#[tokio::main]
async fn main() {
    let state_dir = app_data_dir();
    
    println!("Hello, world!");
    init_app_storage().unwrap();
    let state = APP_STATE.clone();
    
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
    .await
    .unwrap();
}
