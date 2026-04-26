use std::{
    net::{TcpStream, ToSocketAddrs},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use colored::*;

#[derive(Debug, Clone)]
pub struct WaitCheck {
    /// TCP port to probe.
    pub port: Option<u16>,
    /// Host for TCP port probes.
    pub host: String,
    /// Shell command that exits 0 when ready.
    pub exec: Option<String>,
    /// Maximum time to wait.
    pub timeout_ms: u64,
}

/// Wait for a port or shell command check to pass.
pub fn wait_for_check(check: &WaitCheck, label: &str) -> Result<()> {
    if check.port.is_some() && check.exec.is_some() {
        return Err(anyhow!(
            "{label}: wait requires only one of '--port' or '--exec'"
        ));
    }

    let started = Instant::now();
    let timeout = Duration::from_millis(check.timeout_ms);
    let mut delay = Duration::from_millis(100);

    loop {
        if started.elapsed() > timeout {
            return Err(anyhow!("{label}: timed out after {}ms", check.timeout_ms));
        }

        let ready = match (check.port, check.exec.as_deref()) {
            (Some(port), None) => check_tcp_port(&check.host, port),
            (None, Some(command)) => check_exec(command),
            (None, None) => return Err(anyhow!("{label}: requires '--port' or '--exec'")),
            (Some(_), Some(_)) => unreachable!(),
        };

        if ready {
            println!(
                "{} {} ({}ms)",
                label,
                "ready".bright_green(),
                started.elapsed().as_millis()
            );
            return Ok(());
        }

        thread::sleep(delay);
        delay = (delay * 2).min(Duration::from_secs(2));
    }
}

fn check_tcp_port(host: &str, port: u16) -> bool {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };

    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok())
}

fn check_exec(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
