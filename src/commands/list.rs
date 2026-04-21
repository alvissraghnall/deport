use crate::{APP_STATE, cli_utils::format_url, state::AppStateTrait};
use anyhow::{Context, Result};
use colored::Colorize;

pub(crate) async fn handle_list(addr: &str) -> Result<()> {
    let mut client = crate::ipc::client::IpcClient::connect(addr)
        .await
        .context("Failed to connect to deport daemon. Is it running? Try `deport proxy start`.")?;

    let list_request = crate::ipc::Request::ListRoutes;
    let response = crate::ipc::client::IpcClient::send_request(&mut client, list_request).await;

    let routes = match response {
        Ok(crate::ipc::Response::Routes { routes }) => routes,
        Ok(crate::ipc::Response::Error { message }) => {
            println!("{}", format!("Error fetching routes: {}", message).bright_red());
            return Ok(());
        }
        _ => {
            println!("{}", "Unexpected response from daemon".bright_red());
            return Ok(());
        }
    };
    println!("Active routes:");
    for (hostname, route) in routes {
        let url = format_url(hostname.as_str(), route.port, true);
        let pid_str = format!("pid {}", route.pid);
        let label = if route.pid == 0 { "inactive" } else { pid_str.as_str() };
        println!("{} -> {} ({})", url, format!("localhost:{}", APP_STATE.get_proxy_port()).green(), label);
    }
    Ok(())
}