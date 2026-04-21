#[cfg(unix)]
use std::path::Path;
use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[cfg(unix)]
use daemonizr::Daemonizr;
#[cfg(unix)]
use daemonizr::DaemonizrError;

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use windows_service::{
    Result as WinResult, define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

static SHUTDOWN: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));

#[allow(dead_code)]
pub async fn run_server_components(proxy_port: Option<u16>) -> anyhow::Result<()> {
    tracing::info!("Initializing server components...");

    let state = crate::APP_STATE.clone();
    let process_manager = crate::PROCESS_MANAGER.clone();

    tokio::spawn(async move {
        crate::proxy::start_proxy(state, proxy_port).await;
    });

    let (tx, rx) = tokio::sync::mpsc::channel::<crate::ipc::worker::WorkItem>(100);
    tokio::spawn(crate::ipc::worker::worker_loop(rx, process_manager));

    crate::ipc::run(tx).await?;
    Ok(())
}

#[cfg(unix)]
pub fn start(path: &Path, proxy_port: Option<u16>) -> anyhow::Result<()> {
    let stdout = daemonizr::Stdout::Redirect(path.join("daemon.out"));
    let stderr = daemonizr::Stderr::Redirect(path.join("daemon.err"));
    println!("{:?}", path);

    let daemon = Daemonizr::new()
        .work_dir(path.to_path_buf())
        .expect("invalid path")
        .pidfile(path.join("deport.pid"))
        .stdout(stdout)
        .stderr(stderr)
        .umask(0o027)
        .expect("invalid umask");

    match daemon.spawn() {
        Err(DaemonizrError::AlreadyRunning) => {
            eprintln!("Daemon already running");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Daemonization error: {}", e);
            std::process::exit(1);
        }
        Ok(()) => {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                tracing::info!("Daemon process started.");
                let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(16);
                tokio::spawn(signal_handler(shutdown_tx.clone()));
                if let Err(e) = run_server(proxy_port, shutdown_tx.clone()).await {
                    tracing::error!("Server exited with error: {:?}", e);
                }
                tracing::info!("Daemon exiting cleanly.");
            });
        }
    }
    Ok(())
}

async fn signal_handler(shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate()).unwrap();
        let mut sigint = signal(SignalKind::interrupt()).unwrap();

        tokio::select! {
            _ = sigterm.recv() => tracing::info!("SIGTERM received"),
            _ = sigint.recv() => tracing::info!("SIGINT received"),
        }
    }

    // notify all tasks
    let _ = shutdown_tx.send(());
}

pub async fn run_server(
    proxy_port: Option<u16>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> anyhow::Result<()> {
    tracing::info!("Initializing server components...");

    let state = crate::APP_STATE.clone();
    let process_manager = crate::PROCESS_MANAGER.clone();

    let mut shutdown_rx = shutdown_tx.subscribe();

    println!("{:?}", "1");

    let mut proxy_task = tokio::spawn({
        let mut shutdown_rx = shutdown_tx.subscribe();
        async move {
            tokio::select! {
                _ = crate::proxy::start_proxy(state, proxy_port) => {},
                _ = shutdown_rx.recv() => {
                    tracing::info!("Proxy shutting down");
                }
            }
        }
    });

    let (tx, rx) = tokio::sync::mpsc::channel(100);

    println!("{:?}", "2");

    let mut worker_task = tokio::spawn({
        let mut shutdown_rx = shutdown_tx.subscribe();
        async move {
            tokio::select! {
                _ = crate::ipc::worker::worker_loop(rx, process_manager) => {},
                _ = shutdown_rx.recv() => {
                    tracing::info!("Worker shutting down");
                }
            }
        }
    });
    println!("{:?}", "3");

    let mut ipc_task = tokio::spawn({
        let mut shutdown_rx = shutdown_tx.subscribe();
        async move {
            tokio::select! {
                res = crate::ipc::run(tx) => res,
                _ = shutdown_rx.recv() => {
                    tracing::info!("IPC shutting down");
                    Ok(())
                }
            }
        }
    });

    println!("{:?}", "4");

    tokio::select! {
        _ = shutdown_rx.recv() => {
            tracing::info!("Shutdown signal received, propagating to tasks...");
        }
        res = &mut proxy_task => {
            match res {
                Ok(_) => tracing::info!("Proxy task shut down gracefully."),
                Err(e) => tracing::error!("Proxy task panicked: {:?}", e),
            }
            let _ = shutdown_tx.send(());
        }
        res = &mut worker_task => {
            match res {
                Ok(_) => tracing::info!("Worker task shut down gracefully."),
                Err(e) => tracing::error!("Worker task panicked: {:?}", e),
            }
            let _ = shutdown_tx.send(());
        }
        res = &mut ipc_task => {
            match res {
                Ok(_) => tracing::info!("IPC task shut down gracefully."),
                Err(e) => tracing::error!("IPC task panicked: {:?}", e),
            }
            let _ = shutdown_tx.send(());
        }
    }

    // Wait for all tasksksksks to finish before returning
    let _ = proxy_task.await;
    let _ = worker_task.await;
    let _ = ipc_task.await;

    tracing::info!("All daemon tasks shut down.");

    Ok(())
}

/// WE ARE NOT PASSING PROXY_PORT INTO WINDOWS SERVICE YET COS I CAN'T FIGURE IT OUT
/// RN, JAJAJAJAJAJAJAJAJAJAJAJAJAJAJAJAJAJAJAAJAAJAJJAJAAJA

#[cfg_attr(unix, allow(dead_code))]
async fn setup_unix_signal_handler() {
    let signal_shutdown = SHUTDOWN.clone();

    let signals = tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate()).unwrap();
        let mut sigint = signal(SignalKind::interrupt()).unwrap();

        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM");
                signal_shutdown.store(true, Ordering::Relaxed);
            }
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT");
                signal_shutdown.store(true, Ordering::Relaxed);
            }
        }
    });

    while !SHUTDOWN.load(Ordering::Relaxed) {
        // state updates
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    tracing::info!("Daemon exiting gracefully");

    // esure signal handler task finishes
    let _ = signals.await;
}

#[cfg(windows)]
fn daemon_loop_win(proxy_port: Option<u16>) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        tokio::spawn(async {
            while !SHUTDOWN.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });

        // Execute the long-running daemon logic
        if let Err(e) = run_server_components(proxy_port).await {
            tracing::error!("Daemon server crashed: {}", e);
        }
    });
    tracing::info!("Windows service exiting.");
}

#[cfg(windows)]
pub fn start(proxy_port: Option<u16>) -> WinResult<()> {
    define_windows_service!(ffi_service_main, daemon_service_main);
    service_dispatcher::start("deportservice", ffi_service_main)?;
    daemon_loop_win(proxy_port);
    Ok(())
}

#[cfg(windows)]
fn daemon_service_main(arguments: Vec<OsString>) {
    if let Err(_e) = run_service(arguments) {
        // Handle errors somehow
    }
}

#[cfg(windows)]
fn run_service(arguments: Vec<OsString>) -> WinResult<()> {
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                // Handle stop event and return control back to the system.
                SHUTDOWN.store(true, Ordering::Relaxed);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Pause => {
                println!("Received Pause event");
                ServiceControlHandlerResult::NoError
            }
            // All services must accept Interrogate even if it's a no-op.
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    // Register system service event handler
    let status_handle = service_control_handler::register("deportservice", event_handler)?;

    let running_status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    };
    status_handle.set_service_status(running_status)?;
    // daemon_loop_win();
    Ok(())
}
