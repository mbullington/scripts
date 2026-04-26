use std::{collections::HashMap, ffi::OsString, path::PathBuf};

use anyhow::Result;

use crate::helpers::{
    cache::compute_task_hash,
    graph::TaskGraph,
    path::{build_path_var, collect_task_bins, resolve_workspace_bins},
    scripts_def::WorkspaceConfig,
};

#[derive(Clone, Debug)]
pub struct PlanEntry {
    pub index: usize,
    pub name: String,
    pub dir: PathBuf,
    pub command: Option<String>,
    pub path_var: OsString,
    pub dependencies: Vec<usize>,
    pub cache_key: String,
    pub cache_hash: Option<String>,
    pub should_run: bool,
}

#[derive(Debug)]
pub struct RunPlan {
    pub entries: Vec<PlanEntry>,
}

impl RunPlan {
    pub fn build(
        graph: &TaskGraph,
        git_root: &std::path::Path,
        cache: &HashMap<String, String>,
        force: bool,
        append_cmd: Option<&String>,
        workspace_config: Option<&WorkspaceConfig>,
    ) -> Result<Self> {
        let mut entries = Vec::with_capacity(graph.scripts.len());

        for (idx, node) in graph.scripts.iter().enumerate() {
            let command = if idx == graph.root {
                combine_command(&node.task.command, append_cmd)
            } else {
                node.task.command.clone()
            };
            let cache_key = format!("{}:{}", node.unit_path.display(), node.task_name);
            let cache_hash = compute_task_hash(node, command.as_deref())?;
            let should_run = should_run_task(cache, force, &cache_key, cache_hash.as_ref());

            let mut bins = collect_task_bins(graph, idx);
            bins.extend(resolve_workspace_bins(
                git_root,
                &node.unit_path,
                workspace_config,
            ));

            entries.push(PlanEntry {
                index: idx,
                name: task_display_name(graph, idx, git_root),
                dir: node.unit_path.clone(),
                command,
                path_var: build_path_var(&bins)?,
                dependencies: node.dependencies.clone(),
                cache_key,
                cache_hash,
                should_run,
            });
        }

        mark_dependents_dirty(&mut entries);
        Ok(Self { entries })
    }
}

fn should_run_task(
    cache: &HashMap<String, String>,
    force: bool,
    cache_key: &str,
    cache_hash: Option<&String>,
) -> bool {
    if force {
        return true;
    }

    match cache_hash {
        Some(hash) => cache.get(cache_key).is_none_or(|previous| previous != hash),
        None => true,
    }
}

fn mark_dependents_dirty(entries: &mut [PlanEntry]) {
    for idx in 0..entries.len() {
        if !entries[idx].should_run
            && entries[idx]
                .dependencies
                .iter()
                .any(|dep| entries[*dep].should_run)
        {
            entries[idx].should_run = true;
        }
    }
}

fn task_display_name(graph: &TaskGraph, idx: usize, relative_to: &std::path::Path) -> String {
    let node = &graph.scripts[idx];
    let path = node
        .unit_path
        .strip_prefix(relative_to)
        .unwrap_or(&node.unit_path);
    format!("{}:{}", path.display(), node.task_name)
}

fn combine_command(base: &Option<String>, appended: Option<&String>) -> Option<String> {
    match (base, appended) {
        (Some(base), Some(extra)) => {
            let mut combined = base.trim_end().to_string();
            let extra = extra.trim();
            if !combined.is_empty() && !extra.is_empty() {
                combined.push(' ');
            }
            combined.push_str(extra);
            Some(combined)
        }
        (Some(base), None) => Some(base.clone()),
        (None, Some(extra)) => Some(extra.trim().to_string()),
        (None, None) => None,
    }
}
