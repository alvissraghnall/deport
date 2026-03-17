use std::{
    io::{self, Write as _},
    path::{self, Path},
};

use anyhow::anyhow;
use colored::Colorize;

static FRAMEWORKS_NEEDING_PORT: [(&str, bool); 6] = [
    ("vite", true),
    ("react-router", true),
    ("astro", false),
    ("ng", false),
    ("react-native", false),
    ("expo", false),
];

pub(crate) fn inject_framework_flags(cmd: &str, args: &mut Vec<String>, port: u16) -> anyhow::Result<()> {
    let path = Path::new(cmd);

    let filename = path.file_name().ok_or(anyhow!("Invalid path to command provided!"))?;
    let filename = filename.to_str().ok_or(anyhow!("Couldn't construct string from path"))?;

    let framework_idx: Option<usize> = FRAMEWORKS_NEEDING_PORT.iter().position(|&val|  val.0 == filename);

    match framework_idx {
        Some(index) => {
            let framework = FRAMEWORKS_NEEDING_PORT[index];
            if !args.contains(&"--port".to_string()) {
                args.push(format!("--port {}", port));

                if framework.1 {
                    args.push("--strictPort".to_string());
                }
            }

            if !args.contains(&"--host".to_string()) {
                let host = if filename == "expo" {
                    "localhost"
                } else {
                    "0.0.0.0"
                };
                args.push(format!("--host {}", host));
            }

            Ok(())

        },
        None => return Ok(()),
    }
}

fn app_port_from_env() -> Option<u16> {
    let env_val = std::env::var("DEPORT_APP_PORT").ok();

    match env_val {
        Some(val) => {
            let port = val.parse::<u16>().ok();
            if port < Some(1 as u16) || port >= Some(65534) {
                panic!("Error: Invalid DEPORT_APP_PORT={}. Must be 1-65535.", val);
            }
            return port;
        }
        None => {
            return None;
        }
    }
}

fn parse_app_port(value: String) -> u16 {
    if value.is_empty() || value.starts_with("--") {
        panic!("Error: --app-port requires a port number.");
    };
    match value.parse::<u16>() {
        Ok(port) => return port,
        Err(_) => panic!("Error: Invalid app port = {}. Must be 1-65535.", value),
    }
}

pub(crate) fn format_url(hostname: &str, port: u16, tls: bool) -> String {
    if tls {
        format!("https://{}:{}", hostname, port)
    } else {
        format!("http://{}:{}", hostname, port)
    }
}

fn print_help() {
    println!(
        "{} - Replace port numbers with stable, named .localhost URLs.\n\n{}\n    {}\n\n{}\n    run <cmd>...            Infer name from project, run through proxy\n    <name> <cmd>...         Run your app through the proxy\n    proxy start [options]   Start the proxy (background daemon)\n    proxy stop              Stop the proxy\n    list                    Show active routes\n    get <name>              Print URL for a service\n    alias <name> <port>     Register a static route\n    trust                   Add local CA to system trust store\n    hosts sync              Add routes to /etc/hosts\n\n{}\n    deport proxy start\n    deport run next dev\n    deport myapp next dev",
        "deport".bold(),
        "Usage:".bold(),
        "deport <command> [options] <args...>".cyan(),
        "Commands:".bold(),
        "Examples:".bold()
    );
}

fn print_app_help() {
    println!(
        "{} - Run a project with a specific name through the proxy.\n\n{}\n    {}\n\n{}\n    --force                Override an existing route registered by another process\n    --app-port <number>    Use a fixed port for the app (skip auto-assignment)\n    --help, -h             Show this help\n\n{}\n    deport myapp next dev             # -> http://myapp.localhost:1999\n    deport myapp --app-port 3000 ...  # -> http://myapp.localhost:1999",
        "deport run <name>".bold(),
        "Usage:".bold(),
        "deport <name> [options] <command...>".cyan(),
        "Options:".bold(),
        "Examples:".bold()
    );
}

fn print_run_help() {
    println!(
        "{} - Infer project name and run through the proxy.\n\n{}\n    
        {}\n\n{}\n    
        --name <name>          Override the inferred base name (worktree prefix still applies)\n    
        --force                Override an existing route registered by another process\n    
        --app-port <number>    Use a fixed port for the app (skip auto-assignment)\n    
        --help, -h             Show this help\n\n{}\n    
        1. Git repo root directory name\n    
        2. Current directory basename\n\n
        Use --name to override the inferred name while keeping worktree prefixes.\n
        In git worktrees, the branch name is prepended as a subdomain prefix\n
        (e.g. feature-auth.myapp.localhost).\n\n{}\n    
        Examples:\n    
        deport run next dev               # -> http://<project>.localhost:1999\n    
        deport run --name myapp next dev  # -> http://myapp.localhost:1999\n    
        deport run vite dev               # -> http://<project>.localhost:1999\n    
        deport run --app-port 3000 pnpm start",
        "deport run".bold(),
        "Usage:".bold(),
        "deport run [options] <command...>".cyan(),
        "Options:".bold(),
        "Name inference (in order):".bold(),
        "Examples:".bold()
    );
}

pub fn sanitize_rfc1035(hostname: &str) -> String {
    let lower = hostname.to_lowercase();

    let re = regex::Regex::new(r"[^a-z0-9\.-]").unwrap();
    let sanitized = re.replace_all(&lower, "-");

    let labels: Vec<String> = sanitized
        .split('.')
        .map(|label| {
            let mut l = label.trim_matches('-').to_string();
            if l.is_empty() {
                return "".to_string();
            }
            if l.len() > 63 {
                l.truncate(63);
            }
            l
        })
        .collect();

    let result = labels.join(".");

    if result.len() > 255 {
        return result[..255].to_string();
    }

    result
}

pub fn prompt_input(prompt: &str) -> anyhow::Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

pub fn confirm_action(prompt: &str) -> anyhow::Result<bool> {
    loop {
        print!("{} [y/N]: ", prompt);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" | "" => return Ok(false),
            _ => println!("Please enter 'y' or 'n'"),
        }
    }
}
