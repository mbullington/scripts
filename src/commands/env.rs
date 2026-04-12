use std::path::Path;

use anyhow::Result;

use crate::helpers::{
    graph::{build_task_graph, TaskGraph},
    path::build_path_var,
    resolve::parse_target,
};

pub fn cmd_env_command(target: &str) -> Result<()> {
    let (unit, task_name) = parse_target(target);
    let unit_path = Path::new(&unit);

    let graph: TaskGraph = build_task_graph(unit_path, &task_name)?;

    let mut bins = Vec::new();
    for node in &graph.scripts {
        if let Some(bin) = &node.task.bin {
            for path in bin {
                bins.push(node.unit_path.join(path));
            }
        }
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/sh"));
    std::process::Command::new(shell)
        .env("PATH", build_path_var(unit_path, &bins)?)
        .env("PS1", format!("({unit}) := "))
        .status()?;
    Ok(())
}
