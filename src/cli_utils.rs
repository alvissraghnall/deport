use std::io::{self, Write as _};

use colored::Colorize;

use crate::{app_data_dir, trust_ca::trust_ca};

pub fn handle_command(args: &[String]) {
    if args.len() < 2 {
        print_help();
        std::process::exit(0);
    }
    let cmd = &args[1];
    if cmd == "--help" || cmd == "-h" {
        print_help();
        std::process::exit(0);
    }
}

struct ParsedRunArgs {
    force: bool,
    /** Fixed app port (overrides automatic assignment). */
    app_port: Option<u16>,
    /** Override the inferred base name (from --name flag). */
    name: Option<String>,
    /** The child command and its arguments, passed through untouched. */
    command_args: Vec<String>,
}

struct ParsedAppArgs {
    name: String,

    force: bool,
    /** Fixed app port (overrides automatic assignment). */
    app_port: Option<u16>,
    /** The child command and its arguments, passed through untouched. */
    command_args: Vec<String>,
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

/**
 * Parse named-mode arguments: `[--force] <name> [--force] [--] <command...>`
 *
 * `--force` is recognized before and after the name. `--` stops flag
 * parsing. Everything after the flag region is the child command.
 * Unrecognized `--` flags are rejected to catch typos.
 */
fn parse_app_args(args: &[String]) -> ParsedAppArgs {
    let mut force = false;
    let mut app_port: Option<u16> = None;
    let mut i = 0;

    while i < args.len() && args[i].starts_with("-") {
        if args[i] == "--" {
            i += 1;
            break;
        } else if args[i] == "--force" {
            force = true;
            i += 1;
        } else if args[i] == "--app-port" {
            app_port = Some(parse_app_port(args[i + 1].clone()));
            i += 2;
        } else {
            eprintln!("{}", format!("Error: Unknown flag {}", args[i]).red());
            eprintln!("{}", "  Known flags: --force, --app-port, --help".blue());
            std::process::exit(1);
        }
        i += 1;
    }

    let name = &args[i];
    i += 1;

    // also allow flags after app name

    while i < args.len() && args[i].starts_with("-") {
        if args[i] == "--" {
            i += 1;
            break;
        } else if args[i] == "--force" {
            force = true;
            i += 1;
        } else if args[i] == "--app-port" {
            app_port = Some(parse_app_port(args[i + 1].clone()));
            i += 2;
        } else {
            eprintln!("{}", format!("Error: Unknown flag {}", args[i]).red());
            eprintln!("{}", "  Known flags: --force, --app-port, --help".blue());
            std::process::exit(1);
        }
        i += 1;
    }

    if app_port.is_none() {
        app_port = app_port_from_env();
    }

    return ParsedAppArgs {
        name: name.to_owned(),
        force,
        app_port,
        command_args: args[i..].to_vec(),
    };
}

fn print_version() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    println!("deport {}", VERSION);
    std::process::exit(0);
}

fn handle_trust() {
    let data_dir = app_data_dir();
    let ca_path = data_dir.join("ca.crt");

    if ca_path.exists() {
        match trust_ca(&ca_path) {
            Ok(_) => {
                print!("{}", "Local CA added to system trust store".green());
                std::process::exit(0);
            },
            Err(e) => {
                eprintln!("{}", "Failed to add local CA to system trust store".red());
                if e.to_string().contains("sudo") {
                    eprintln!("{}", "You might need to run with sudo: sudo deport trust".cyan());
                } else {
                    eprintln!("{}", e.to_string().red());
                }
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("{}", "Local CA not found".red());
        std::process::exit(1);
    }
}

fn handle_list (routes_manager: &crate::routes::RouteManager, tls: bool) {
    list_routes(routes_manager, tls);
    std::process::exit(0);
}

fn handle_get (_args: &[String]) {

}

fn list_routes(routes_manager: &crate::routes::RouteManager, tls: bool) {

    let list = routes_manager.list();
    if list.is_empty() {
        println!("No active routes found");
        return;
    }
    println!("Active routes:");
    for (hostname, route) in list {
        let url = format_url(hostname.as_str(), route.port,  tls);
        let pid_str = format!("pid {}", route.pid);
        let label = if route.pid == 0 { "inactive" } else { pid_str.as_str() };
        println!("{} -> {} ({})", url, format!("localhost:{}", route.port), label);
    }

}

fn format_url (hostname: &str, port: u16, tls: bool) -> String {
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