use anyhow::Result;

use crate::helpers::wait::{wait_for_check, WaitCheck};

pub fn cmd_wait_command(
    port: Option<u16>,
    host: String,
    exec: Option<String>,
    timeout_ms: u64,
) -> Result<()> {
    let label = match (port, exec.as_deref()) {
        (Some(port), None) => format!("{host}:{port}"),
        (None, Some(command)) => command.to_string(),
        (Some(_), Some(_)) => String::from("wait"),
        (None, None) => String::from("wait"),
    };
    let check = WaitCheck {
        port,
        host,
        exec,
        timeout_ms,
    };

    wait_for_check(&check, &label)
}
