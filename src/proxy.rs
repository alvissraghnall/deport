use std::{convert::Infallible, sync::Arc, sync::LazyLock, sync::Mutex, time::Duration};

use crate::{
    process_man::get_default_proxy_port, routes::RouteManager, sni::AsyncTlsIssuerService,
    state::SharedState,
};
use rama::{
    Context, Layer as _, Service,
    context::RequestContextExt,
    graceful::{ShutdownGuard},
    http::{
        Request, Response, StatusCode,
        client::EasyHttpWebClient,
        layer::{
            remove_header::{RemoveRequestHeaderLayer, RemoveResponseHeaderLayer},
            trace::TraceLayer,
            upgrade::UpgradeLayer,
        },
        matcher::MethodMatcher,
        server::HttpServer,
        service::{web::response::IntoResponse as _},
    },
    layer::ConsumeErrLayer,
    net::{http::RequestContext, stream::ClientSocketInfo, tls::server::SelfSignedData},
    rt::Executor,
    service::service_fn,
    tcp::{client::service::Forwarder, server::TcpListener},
    tls::rustls::server::{TlsAcceptorDataBuilder, TlsAcceptorLayer},
};

use tokio::sync::{oneshot};

pub static PROXY_SHUTDOWN_TX: LazyLock<Mutex<Option<oneshot::Sender<()>>>> =
    LazyLock::new(|| Mutex::new(None));
// pub static NOTIFY: LazyLock<Arc<Notify>> = LazyLock::new(|| Arc::new(Notify::new()));

/// Programmatically triggers a graceful shutdown of the proxy
pub fn stop_proxy() {
    if let Ok(mut lock) = PROXY_SHUTDOWN_TX.lock() {
        if let Some(tx) = lock.take() {
            let _ = tx.send(()).ok();
            tracing::info!("Sent manual stop signal to proxy.");
        }
    }
}

async fn tls_term(guard: ShutdownGuard, state: SharedState, port: Option<u16>) {
    let executor = Executor::graceful(guard.clone());
    let client = Arc::new(EasyHttpWebClient::default());
    let port = port.unwrap_or_else(|| get_default_proxy_port());

    let state_cl = state.clone();

    let http_core = service_fn(move |req| {
        let routes = state.routes.clone();
        internal_http_service(req, routes, client.clone())
    });

    let http_stack = HttpServer::auto(executor).service(
        (
            TraceLayer::new_for_http(),
            UpgradeLayer::new(
                MethodMatcher::CONNECT,
                service_fn(http_connect_accept::<()>),
                ConsumeErrLayer::default().into_layer(Forwarder::ctx()),
            ),
            RemoveResponseHeaderLayer::hop_by_hop(),
            RemoveRequestHeaderLayer::hop_by_hop(),
        )
            .into_layer(http_core),
    );

    let acceptor_data = TlsAcceptorDataBuilder::new_self_signed(SelfSignedData::default())
        .expect("tls acceptor with self signed data")
        .with_alpn_protocols_http_auto()
        .with_env_key_logger()
        .expect("with env key logger")
        .build();

    // let http_service = HttpServer::auto(executor).service(service_fn(move |req| {
    //     internal_http_service(req, route_manager.clone(), client.clone())
    // }));
    let tls_service = AsyncTlsIssuerService::new(http_stack, state_cl);

    let tcp_service = (
        ConsumeErrLayer::default(),
        TlsAcceptorLayer::new(acceptor_data),
    )
        .into_layer(tls_service);

    println!(
        "Starting TLS termination proxy on port {}... ",
        port.to_string()
    );
    TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .expect("bind TCP Listener: http")
        .serve_graceful(guard, tcp_service)
        .await;
}

async fn http_connect_accept<S>(
    mut ctx: Context<S>,
    req: Request,
) -> Result<(Response, Context<S>, Request), Response>
where
    S: Clone + Send + Sync + 'static,
{
    match ctx.get_or_try_insert_with_ctx::<RequestContext, _>(|ctx| (ctx, &req).try_into()) {
        Ok(request_ctx) => tracing::info!("accept CONNECT to {}", request_ctx.authority),
        Err(err) => {
            tracing::error!(err = %err, "error extracting authority");
            return Err(StatusCode::BAD_REQUEST.into_response());
        }
    }

    Ok((StatusCode::OK.into_response(), ctx, req))
}

async fn internal_http_service(
    mut req: rama::http::Request,
    state: Arc<RouteManager>,
    client: Arc<EasyHttpWebClient>,
) -> Result<rama::http::Response, Infallible> {
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    tracing::info!("Received request for host: {}", host);

    let port = {
        let map = state.get(&host.to_owned());
        match map {
            Some(route) => route.port,
            None => {
                tracing::warn!("No route found for host: {}, returning 404", host);
                return Ok(rama::http::Response::builder()
                    .status(404)
                    .body("Not Found".into())
                    .unwrap());
            }
        }
    };

    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            req.extensions()
                .get::<RequestContextExt>()
                .and_then(|ctx| ctx.get::<ClientSocketInfo>())
                .map(|addr| addr.peer_addr().ip().to_string())
                .unwrap_or_else(|| "127.0.0.1".to_string())
        });

    let new_uri = format!(
        "http://127.0.0.1:{}{}",
        port,
        req.uri().path_and_query().map_or("", |p| p.as_str())
    );

    *req.uri_mut() = new_uri.parse().unwrap_or_else(|_| {
        tracing::error!("Failed to parse new URI: {}", new_uri);
        req.uri().clone()
    });

    req.headers_mut()
        .insert("x-forwarded-for", ip.parse().unwrap());
    req.headers_mut()
        .insert("x-deport", 1.to_string().parse().unwrap());

    let ctx = Context::default();
    let response = match client.serve(ctx, req).await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::error!(err = %err, "Error forwarding request to backend");
            return Ok(rama::http::Response::builder()
                .status(502)
                .body("Bad Gateway".into())
                .unwrap());
        }
    };

    Ok(response)
}

pub(crate) async fn start_proxy(state: SharedState, port: Option<u16>) {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut lock) = PROXY_SHUTDOWN_TX.lock() {
        *lock = Some(tx);
    }

    let shutdown = rama::graceful::Shutdown::new(async move {
        let _ = rx.await;
    });

    shutdown.spawn_task_fn({
        let state = state.clone();
        async move |guard: ShutdownGuard| {
            tls_term(guard, state, port).await;
        }
    });

    shutdown
        .shutdown_with_limit(Duration::from_secs(3))
        .await
        .expect("graceful shutdown");
}

pub(crate) async fn is_proxy_running(_port: Option<u16>, _tls: Option<bool>) -> bool {
    #[cfg(unix)]
    let addr = "/tmp/deport.sock";

    #[cfg(windows)]
    let addr = r"\\.\pipe\deport";

    match crate::ipc::client::IpcClient::connect(addr).await {
        Ok(_) => {
            tracing::info!("Proxy is running (IPC connected)");
            true
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                tracing::info!("Proxy is running (IPC owned by another user)");
                true
            } else {
                false
            }
        }
    }
}
