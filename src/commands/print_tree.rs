use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use termtree::Tree;

use crate::helpers::graph::{build_target_graph, TaskGraph};

fn get_tree(graph: &TaskGraph, handle: usize, relative_to: &Path) -> Tree<String> {
    let node = &graph.scripts[handle];
    let rel = node
        .unit_path
        .strip_prefix(relative_to)
        .unwrap_or(&node.unit_path);
    let mut tree = Tree::new(format!("{}:{}", rel.display(), node.task_name));
    for dep in &node.dependencies {
        tree.push(get_tree(graph, *dep, relative_to));
    }
    tree
}

#[derive(Serialize)]
struct JsonNode {
    name: String,
    deps: Vec<JsonNode>,
}

fn get_json(graph: &TaskGraph, handle: usize, relative_to: &Path) -> JsonNode {
    let node = &graph.scripts[handle];
    let rel = node
        .unit_path
        .strip_prefix(relative_to)
        .unwrap_or(&node.unit_path);
    JsonNode {
        name: format!("{}:{}", rel.display(), node.task_name),
        deps: node
            .dependencies
            .iter()
            .map(|d| get_json(graph, *d, relative_to))
            .collect(),
    }
}

fn get_flat(
    graph: &TaskGraph,
    handle: usize,
    relative_to: &Path,
    out: &mut std::collections::BTreeSet<String>,
) {
    let node = &graph.scripts[handle];
    let rel = node
        .unit_path
        .strip_prefix(relative_to)
        .unwrap_or(&node.unit_path);
    out.insert(format!("{}:{}", rel.display(), node.task_name));
    for dep in &node.dependencies {
        get_flat(graph, *dep, relative_to, out);
    }
}

pub fn cmd_print_tree_command(target: &str, json: bool, flat: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let (graph, git_root) = build_target_graph(target, &cwd)?;
    let root_handle = graph.root;
    if flat {
        let mut set = std::collections::BTreeSet::new();
        get_flat(&graph, root_handle, &git_root, &mut set);
        if json {
            let list: Vec<String> = set.into_iter().collect();
            let output = serde_json::to_string_pretty(&list)?;
            println!("{output}");
        } else {
            for item in set {
                println!("{item}");
            }
        }
    } else if json {
        let json_tree = get_json(&graph, root_handle, &git_root);
        let output = serde_json::to_string_pretty(&json_tree)?;
        println!("{output}");
    } else {
        let root = get_tree(&graph, root_handle, &git_root);
        println!("{root}");
    }
    Ok(())
}
