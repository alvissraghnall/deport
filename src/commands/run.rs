use anyhow::{Result, bail};
use clap::Args;
use rand::RngExt as _;

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Process Command to run
    cmd: String,

    /// args the process
    args: Vec<String>,

    /// name for subdomain
    #[arg(short, long)]
    name: Option<String>,

    #[arg(short, long)]
    force: bool,

    #[arg(short, long)]
    port: Option<u16>,

    #[arg(short, long)]
    env: Option<Vec<String>>,
}

pub fn handle_run(args: RunArgs) -> Result<()> {
    let base_name: String;

    if args.args.is_empty() {
        return Ok(());
    }

    if let Some(name) = args.name {
        let sanitized = crate::cli_utils::sanitize_rfc1035(&name);
        if sanitized.is_empty() {
            bail!("Invalid name: {}", name);
        }
        base_name = sanitized;
    } else {
        base_name = std::env::current_dir()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| crate::cli_utils::sanitize_rfc1035(s))
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                use rand::distr::Alphanumeric;

                let mut rng = rand::rng();
                let rand_name: String = (&mut rng)
                    .sample_iter(Alphanumeric)
                    .take(7)
                    .map(char::from)
                    .collect();

                format!("deported-{}", rand_name.to_lowercase())
            });
    }

    Ok(())
}
