use colored::Colorize as _;

use crate::trust_ca::trust_ca;

pub(crate) fn handle_trust() -> anyhow::Result<()> {
    let state_dir = crate::state::app_data_dir();
    let ca_path = state_dir.join("ca.crt");

    if ca_path.exists() {
        match trust_ca(&ca_path) {
            Ok(_) => {
                print!("{}", "Local CA added to system trust store".green());
                Ok(())
                // std::process::exit(0);
            },
            Err(e) => {
                eprintln!("{}", "Failed to add local CA to system trust store".red());
                if e.to_string().contains("sudo") {
                    eprintln!("{}", "You might need to run with sudo: sudo deport trust".cyan());
                } else {
                    eprintln!("{}", e.to_string().red());
                }
                Err(e)
            }
        }
    } else {
        eprintln!("{}", "Local CA not found".red());
        Err(anyhow::anyhow!("Local CA not found."))
    }
}
