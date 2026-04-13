use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::helpers::{
    graph::TaskGraph,
    scripts_def::{BinAppendEntry, RelativeTo, WorkspaceConfig},
};

pub fn resolve_workspace_bins(
    git_root: &Path,
    unit_path: &Path,
    workspace_config: Option<&WorkspaceConfig>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let Some(config) = workspace_config else {
        return paths;
    };
    let Some(entries) = &config.bin_append else {
        return paths;
    };

    for entry in entries {
        let candidate = match entry {
            BinAppendEntry::String(path) => git_root.join(path),
            BinAppendEntry::Object(obj) => match obj.relative_to {
                RelativeTo::GitRoot => git_root.join(&obj.path),
                RelativeTo::Unit => unit_path.join(&obj.path),
            },
        };

        if candidate.is_dir() {
            paths.push(candidate);
        }
    }

    paths
}

pub fn collect_task_bins(graph: &TaskGraph, idx: usize) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut visited_nodes = HashSet::new();

    collect_task_bins_recursive(graph, idx, &mut paths, &mut seen_paths, &mut visited_nodes);
    paths
}

fn collect_task_bins_recursive(
    graph: &TaskGraph,
    idx: usize,
    paths: &mut Vec<PathBuf>,
    seen_paths: &mut HashSet<PathBuf>,
    visited_nodes: &mut HashSet<usize>,
) {
    if !visited_nodes.insert(idx) {
        return;
    }

    let node = &graph.scripts[idx];
    append_task_bins(paths, seen_paths, &node.unit_path, node.task.bin.as_deref());

    for dep_idx in &node.dependencies {
        collect_task_bins_recursive(graph, *dep_idx, paths, seen_paths, visited_nodes);
    }
}

fn append_task_bins(
    paths: &mut Vec<PathBuf>,
    seen_paths: &mut HashSet<PathBuf>,
    unit_path: &Path,
    bins: Option<&[String]>,
) {
    let Some(bins) = bins else {
        return;
    };

    for bin in bins {
        let path = unit_path.join(bin);
        if seen_paths.insert(path.clone()) {
            paths.push(path);
        }
    }
}

pub fn build_path_var(paths: &[PathBuf]) -> Result<OsString> {
    let mut parts = paths.to_vec();

    if let Some(existing) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&existing));
    }

    Ok(std::env::join_paths(parts)?)
}
