use anyhow::{bail, Context, Result};
use clap::{Args};
use colored::Colorize;

use crate::cli_utils::format_url;

#[derive(Args, Debug)]
pub struct GetArgs {
    /// name of service
    name: String,
}

pub(crate) async fn handle_get(addr: &str, args: &GetArgs) -> Result<()> {
    let mut client = crate::ipc::client::IpcClient::connect(addr)
        .await
        .context("Failed to connect to deport daemon. Is it running? Try `deport proxy start`.")?;

    let host_to_find = format!("{}.localhost", args.name);
    let get_route_request = crate::ipc::Request::GetRoute { hostname: host_to_find.clone() };
    let response = crate::ipc::client::IpcClient::send_request(&mut client, get_route_request).await?;

    let route = match response {
        crate::ipc::Response::Route(route) => route,
        crate::ipc::Response::Error { message } => {
            bail!("Error fetching routes: {}", message);
        }
        _ => {
            bail!("Unexpected response from daemon");
        }
    };

    if let Some(route) = route {
        let url = format_url(&host_to_find, route.port, true);
        println!("{}", url);
    } else {
        println!("{}", format!("No service found with name: {}", &args.name).yellow());
    }
    Ok(())
}