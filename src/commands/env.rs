use anyhow::Result;

use crate::helpers::{
    graph::build_target_graph,
    path::{build_path_var, collect_task_bins, resolve_workspace_bins},
    resolve::read_workspace_config,
    task_list::print_tasks_for_current_unit,
};

pub fn cmd_env_command(target: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let (graph, git_root) = match build_target_graph(target, &cwd) {
        Ok(result) => result,
        Err(error) => {
            print_tasks_for_current_unit();
            return Err(error);
        }
    };
    let workspace_config = read_workspace_config(&git_root)?;

    let root_unit_path = &graph.scripts[graph.root].unit_path;
    let mut bins = collect_task_bins(&graph, graph.root);
    bins.extend(resolve_workspace_bins(
        &git_root,
        root_unit_path,
        workspace_config.as_ref(),
    ));

    let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/sh"));
    std::process::Command::new(shell)
        .env("PATH", build_path_var(&bins)?)
        .env("PS1", format!("({target}) := "))
        .status()?;
    Ok(())
}
