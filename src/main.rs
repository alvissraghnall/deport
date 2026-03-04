use crate::{sni::load_ca, state::{app_data_dir, init_app_storage}};

mod process_man;

mod routes;

mod sni;

mod proxy;

mod ipc;

mod certs;

mod trust_ca;

mod state;

static ROUTES_MANAGER = Lazy routes::RouteManager::new();


#[tokio::main]
async fn main() {
    let state_dir = app_data_dir();
    let (ca_cert, ca_key) = load_ca(&state_dir).expect("CA Certificates should be installed!");
    
    println!("Hello, world!");
    init_app_storage().unwrap();

    route_manager
        .insert(
            "localhost".to_string(),
            routes::Route {
                port: 8000,
                pid: 1234,
            },
        )
        .await;

    tokio::spawn(async {
        proxy::start_proxy(route_manager, ca_cert, ca_key).await;
    })
    .await
    .unwrap();
}
