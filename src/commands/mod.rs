use clap::{Parser, Subcommand};

pub mod run;
// pub mod delete;
// pub mod generate;
// pub mod get;
// pub mod list;
// pub mod update;
// pub mod utils;

/// deport - Replace port numbers with stable, named .localhost URLs.
/// deport run <cmd> <args...>          Infer name from project, run through proxy
/// deport <name> <cmd> <args...>       Run your app through the proxy
/// deport proxy start           Start the proxy (background daemon)
/// deport list                  Show active routes
/// deport trust                 Add local CA to system trust store
/// deport hosts sync            Add routes to /etc/hosts")]
#[derive(Parser, Default, Debug)]
#[command(name = "deport")]
#[command(version, author = "Alviss Raghnall", about, long_about = None)]
#[command(version = "1.0")]
#[command(
    help_template = "{about-section}Version: {version} \n {usage-heading} {usage} \n {all-args} {tab} \n\nWritten by: {author-with-newline}"
)]
#[command(propagate_version = true)]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Commands,

    // #[arg(long, global = true)]
    // db_path: Option<PathBuf>,
}

#[derive(Subcommand, Debug, Default)]
pub enum Commands {

    Run(run::RunArgs),

    Hosts,

    Trust,

    Proxy,

    List,

    #[default]
    Stab,
}