use rama::tcp::server::TcpListener;

mod process_man;

mod routes;

mod proxy;

mod ipc;

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let route_manager = routes::RouteManager::new();

    route_manager.insert("localhost".to_string(), routes::Route { port: 8000, pid: 1234 }).await;

    // Start the TLS termination proxy in the background
    tokio::spawn(async {
        proxy::start_proxy(route_manager).await;
    }).await.unwrap();

}