use std::sync::{
    Arc, LazyLock, atomic::{AtomicBool, Ordering}
};
use std::time::Duration;
#[cfg(unix)]
use std::{fs::File, path::Path};

#[cfg(unix)]
use daemonize::Daemonize;

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

pub async fn run_server_components() -> anyhow::Result<()> {
    tracing::info!("Initializing server components...");
    
    let state = crate::APP_STATE.clone();
    let process_manager = crate::PROCESS_MANAGER.clone();

    tokio::spawn(async {
        crate::proxy::start_proxy(state).await;
    });

    let (tx, rx) = tokio::sync::mpsc::channel::<crate::ipc::worker::WorkItem>(100);
    tokio::spawn(crate::ipc::worker::worker_loop(rx, process_manager));

    crate::ipc::run(tx).await?;
    Ok(())
}

#[cfg(unix)]
pub fn start(path: &Path) -> anyhow::Result<()> {
    let stdout = File::create("/tmp/deport.out").unwrap();
    let stderr = File::create("/tmp/deport.err").unwrap();

    let daemonize = Daemonize::new()
        .pid_file(Path::join(path, "deport.pid"))
        .chown_pid_file(true)
        .working_directory(path)
        .stdout(stdout) // Redirect stdout to `/tmp/daemon.out`.
        .stderr(stderr);

    match daemonize.start() {
        Ok(_) => println!("Success, daemonized"),
        Err(e) => eprintln!("Error, {}", e),
    }

    // safe to spin up tokio runtime as process has forked in bg
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        tokio::spawn(async {
            setup_unix_signal_handler().await;
        });
        
        run_server_components().await
    })?;
    
    Ok(())
}

#[cfg(unix)]
async fn setup_unix_signal_handler() {

    let signal_shutdown = SHUTDOWN.clone();

    let signals = tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};

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
fn daemon_loop_win() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        tokio::spawn(async {
            while !SHUTDOWN.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });

        // Execute the long-running daemon logic
        if let Err(e) = run_server_components().await {
            tracing::error!("Daemon server crashed: {}", e);
        }
    });
    tracing::info!("Windows service exiting.");
}

#[cfg(windows)]
pub fn start() -> WinResult<()> {
    define_windows_service!(ffi_service_main, daemon_service_main);
    service_dispatcher::start("DaemonService", ffi_service_main)?;
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
    let status_handle = service_control_handler::register("daemonservice", event_handler)?;

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
    daemon_loop_win();
    Ok(())
}
