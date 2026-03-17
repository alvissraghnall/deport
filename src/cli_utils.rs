use std::{
    io::{self, Write as _},
    path::{Path},
};

use anyhow::anyhow;

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





#[inline(always)]
pub(crate) fn format_url(hostname: &str, port: u16, tls: bool) -> String {
    if tls {
        format!("https://{}:{}", hostname, port)
    } else {
        format!("http://{}:{}", hostname, port)
    }
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
