use std::{fs, path::Path, process::Command};

use anyhow::{Result, bail};
use rama::tls::boring::core::{
    stack::Stack,
    x509::{X509, X509StoreContext, store::X509StoreBuilder},
};

#[derive(Debug)]
enum LinuxDistro {
    DebianLike,
    FedoraLike,
    ArchLike,
    Unknown,
}

struct SystemCommandRuner;

trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<std::process::ExitStatus>;
}

impl CommandRunner for SystemCommandRuner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
        return Command::new(program).args(args).status();
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_distro() -> LinuxDistro {
    let content = fs::read_to_string("/etc/os-release").unwrap_or_default();

    if content.contains("ID=ubuntu")
        || content.contains("ID=debian")
        || content.contains("ID=linuxmint")
    {
        LinuxDistro::DebianLike
    } else if content.contains("ID=fedora")
        || content.contains("ID=rhel")
        || content.contains("ID=centos")
    {
        LinuxDistro::FedoraLike
    } else if content.contains("ID=arch") {
        LinuxDistro::ArchLike
    } else {
        LinuxDistro::Unknown
    }
}

#[cfg(target_os = "macos")]
fn trust_macos(runner: &dyn CommandRunner, ca_path: &Path) -> Result<()> {
    let status = runner.run(
        "security",
        &[
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-k",
            "/Library/Keychains/System.keychain",
            ca_path.to_str().unwrap(),
        ],
    )?;

    if status.success() {
        Ok(())
    } else {
        bail!("Failed to trust certificate on macOS")
    }
}

#[cfg(target_os = "macos")]
fn untrust_macos(runner: &dyn CommandRunner, common_name: &str) -> Result<()> {
    runner.run("security", &["delete-certificate", "-c", common_name])?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn trust_windows(runner: &dyn CommandRunner, ca_path: &Path) -> Result<()> {
    let status = runner.run(
        "certutil",
        &["-addstore", "Root", ca_path.to_str().unwrap()],
    )?;

    if status.success() {
        Ok(())
    } else {
        bail!("Failed to trust certificate on Windows")
    }
}

#[cfg(target_os = "windows")]
fn untrust_windows(runner: &dyn CommandRunner, common_name: &str) -> Result<()> {
    runner.run("certutil", &["-delstore", "Root", common_name])?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn trust_debian(runner: &dyn CommandRunner, ca_path: &Path) -> Result<()> {
    use std::fs;

    let dest = "/usr/local/share/ca-certificates/deport-ca.crt";
    fs::copy(ca_path, dest)?;

    let status = runner.run("update-ca-certificates", &[])?;
    if status.success() {
        Ok(())
    } else {
        bail!("update-ca-certificates failed")
    }
}

#[cfg(target_os = "linux")]
fn trust_fedora(runner: &dyn CommandRunner, ca_path: &Path) -> Result<()> {
    use std::fs;

    let dest = "/etc/pki/ca-trust/source/anchors/deport-ca.crt";
    fs::copy(ca_path, dest)?;

    let status = runner.run("update-ca-trust", &[])?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("update-ca-trust failed")
    }
}

#[cfg(target_os = "linux")]
fn trust_arch(runner: &dyn CommandRunner, ca_path: &Path) -> Result<()> {
    use std::fs;

    let dest = "/etc/ca-certificates/trust-source/anchors/deport-ca.crt";
    fs::copy(ca_path, dest)?;

    let status = runner.run("trust", &["extract-compat"])?;

    if status.success() {
        Ok(())
    } else {
        bail!("trust extract-compat failed")
    }
}

pub fn trust_ca(ca_path: &Path) -> Result<()> {
    let system_command_runner = SystemCommandRuner {};
    #[cfg(target_os = "macos")]
    {
        return trust_macos(&system_command_runner, ca_path);
    }

    #[cfg(target_os = "windows")]
    {
        return trust_windows(&system_command_runner, ca_path);
    }

    #[cfg(target_os = "linux")]
    {
        if !nix::unistd::Uid::effective().is_root() {
            bail!("Run with sudo to install system certificate");
        }
        match detect_linux_distro() {
            LinuxDistro::DebianLike => trust_debian(&system_command_runner, ca_path),
            LinuxDistro::FedoraLike => trust_fedora(&system_command_runner, ca_path),
            LinuxDistro::ArchLike => trust_arch(&system_command_runner, ca_path),
            LinuxDistro::Unknown => {
                bail!("Unsupported Linux distro. Install CA manually.")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    pub struct MockCommandRunner {
        pub calls: RefCell<Vec<(String, Vec<String>)>>,
        pub succeed: bool,
    }

    #[cfg(unix)]
    impl CommandRunner for MockCommandRunner {
        fn run(&self, program: &str, args: &[&str]) -> std::io::Result<ExitStatus> {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));

            let code = if self.succeed { 0 } else { 1 };

            Ok(ExitStatus::from_raw(code))
        }
        
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn debian_runs_correct_command() {
        let mock = MockCommandRunner {
            calls: RefCell::new(vec![]),
            succeed: true,
        };

        trust_debian(&mock, Path::new("/home/ca-cert.crt")).unwrap();

        let calls = mock.calls.borrow();
        assert_eq!(calls[0].0, "update-ca-certificates");
    }
}

pub(crate) fn is_ca_trusted(ca_path: &Path) -> Result<bool> {
    let pem = fs::read(ca_path)?;
    let cert = X509::from_pem(&pem)?;

    let mut builder = X509StoreBuilder::new()?;
    builder.set_default_paths()?;
    let store = builder.build();

    let mut ctx = X509StoreContext::new()?;
    let stack = Stack::<X509>::new()?;
    Ok(ctx.init(&store, &cert, &stack, |c| c.verify_cert())?)
}
