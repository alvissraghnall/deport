use anyhow::bail;
use clap::{Args, arg};

use crate::{cli_utils::format_url, routes::RouteManager};

#[derive(Args, Debug)]
pub struct GetArgs {
    /// name of service
    #[arg(short, long)]
    name: Option<String>,
}

pub(crate) fn handle_get(args: &GetArgs, rm: &RouteManager) -> anyhow::Result<()> {
    if let Some(name) = &args.name {
        if let Some(route) = rm.get(name) {
            let url = format_url(format!("{}.localhost", name).as_str(), route.port, true);
            print!("{}", url);
            return Ok(());
        }
    }

    bail!("No service found with name: {}", &args.name.clone().unwrap_or_default())
}