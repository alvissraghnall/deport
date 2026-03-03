use rama::tcp::server::TcpListener;

use crate::state::init_app_storage;

mod process_man;

mod routes;

mod proxy;

mod ipc;

mod certs;

mod trust_ca;

mod state;

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let route_manager = routes::RouteManager::new();
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

    // Start the TLS termination proxy in the background
    tokio::spawn(async {
        proxy::start_proxy(route_manager).await;
    })
    .await
    .unwrap();
    
}
