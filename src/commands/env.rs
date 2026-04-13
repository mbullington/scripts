use std::path::Path;

use anyhow::Result;

use crate::helpers::{
    git::get_git_root,
    graph::{build_task_graph, TaskGraph},
    path::{build_path_var, collect_task_bins, resolve_workspace_bins},
    resolve::{parse_target, read_workspace_config},
    task_list::print_tasks_for_current_unit,
};

pub fn cmd_env_command(target: &str) -> Result<()> {
    let (unit, task_name) = parse_target(target)?;
    let unit_path = Path::new(&unit);

    let git_root = get_git_root(unit_path)?;
    let workspace_config = read_workspace_config(&git_root);
    let graph: TaskGraph = match build_task_graph(unit_path, &task_name) {
        Ok(graph) => graph,
        Err(error) => {
            print_tasks_for_current_unit();
            return Err(error.into());
        }
    };

    let absolute_unit_path = unit_path.canonicalize()?;
    let root_idx = graph.scripts.len().saturating_sub(1);
    let mut bins = collect_task_bins(&graph, root_idx);

    bins.extend(resolve_workspace_bins(
        &git_root,
        &absolute_unit_path,
        workspace_config.as_ref(),
    ));

    let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/sh"));
    std::process::Command::new(shell)
        .env("PATH", build_path_var(&bins)?)
        .env("PS1", format!("({unit}) := "))
        .status()?;
    Ok(())
}
