use std::{
    net::{SocketAddr, TcpStream},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use colored::*;

use super::scripts_def::Readiness;

/// Wait for a post-command readiness check to pass.
pub fn wait_for_readiness(readiness: &Readiness, task_name: &str) -> Result<()> {
    if readiness.port.is_some() && readiness.exec.is_some() {
        return Err(anyhow!(
            "{task_name}: readiness must set only one of 'port' or 'exec'"
        ));
    }

    let started = Instant::now();
    let timeout = Duration::from_millis(readiness.timeout_ms);
    let mut delay = Duration::from_millis(100);

    loop {
        if started.elapsed() > timeout {
            return Err(anyhow!(
                "{task_name}: readiness timed out after {}ms",
                readiness.timeout_ms
            ));
        }

        let ready = match (readiness.port, readiness.exec.as_deref()) {
            (Some(port), None) => check_tcp_port(&readiness.host, port),
            (None, Some(command)) => check_exec(command),
            (None, None) => {
                return Err(anyhow!(
                    "{task_name}: readiness requires either 'port' or 'exec'"
                ))
            }
            (Some(_), Some(_)) => unreachable!(),
        };

        if ready {
            println!(
                "{} {} ({}ms)",
                task_name,
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
    let Ok(address) = format!("{host}:{port}").parse::<SocketAddr>() else {
        return false;
    };

    TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok()
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
