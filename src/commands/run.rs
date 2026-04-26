use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};

use crate::helpers::{
    cache::{load_cache, save_cache},
    git::get_git_root,
    graph::{build_task_graph, TaskGraph},
    resolve::{parse_target, read_workspace_config},
    task_list::print_tasks_for_current_unit,
};

use super::{
    run_executor::{execute_plan, RunOutputMode, TaskEvent},
    run_plan::RunPlan,
};

struct RunOnceResult {
    watch_roots: Vec<PathBuf>,
}

fn output_mode(quiet: bool, verbose: bool) -> RunOutputMode {
    if verbose {
        RunOutputMode::Verbose
    } else if quiet {
        RunOutputMode::Quiet
    } else {
        RunOutputMode::Normal
    }
}

fn collect_watch_roots(graph: &TaskGraph) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    for node in &graph.scripts {
        if node.task.watch.is_some() && seen.insert(node.unit_path.clone()) {
            roots.push(node.unit_path.clone());
        }
    }

    roots
}

fn apply_cache_events(
    cache: &mut std::collections::HashMap<String, String>,
    events: &[TaskEvent],
) -> bool {
    let mut cache_changed = false;

    for event in events {
        match event {
            TaskEvent::Succeeded {
                cache_key,
                cache_hash: Some(cache_hash),
            } => {
                cache_changed |= cache.insert(cache_key.clone(), cache_hash.clone()).as_ref()
                    != Some(cache_hash);
            }
            TaskEvent::Succeeded {
                cache_hash: None, ..
            } => {}
            TaskEvent::Failed { cache_key } => {
                cache_changed |= cache.remove(cache_key).is_some();
            }
        }
    }

    cache_changed
}

fn run_once(
    target: &str,
    force: bool,
    output_mode: RunOutputMode,
    append_cmd: &Option<String>,
) -> Result<RunOnceResult> {
    let (unit, task_name) = parse_target(target)?;
    let unit_path = Path::new(&unit);
    let git_root = get_git_root(unit_path)?;
    let cache_path = git_root.join(".scripts_cache");
    let mut cache = load_cache(&cache_path)?;
    let workspace_config = read_workspace_config(&git_root);

    let graph: TaskGraph = match build_task_graph(unit_path, &task_name) {
        Ok(graph) => graph,
        Err(error) => {
            print_tasks_for_current_unit();
            return Err(error.into());
        }
    };

    let plan = RunPlan::build(
        &graph,
        &git_root,
        &cache,
        force,
        append_cmd.as_ref(),
        workspace_config.as_ref(),
    )?;
    let outcome = execute_plan(&plan, output_mode);

    if apply_cache_events(&mut cache, &outcome.events) {
        save_cache(&cache_path, &cache)?;
    }

    outcome.into_result()?;
    Ok(RunOnceResult {
        watch_roots: collect_watch_roots(&graph),
    })
}

fn event_is_relevant(path: &Path, git_root: &Path) -> bool {
    path != git_root.join(".scripts_cache") && !path.starts_with(git_root.join(".git"))
}

fn watch_target_graph(
    target: &str,
    output_mode: RunOutputMode,
    append_cmd: &Option<String>,
    watch_roots: Vec<PathBuf>,
) -> Result<()> {
    if watch_roots.is_empty() {
        eprintln!("watch mode requested, but no watched tasks were found in the target graph");
        return Ok(());
    }

    eprintln!("watching for changes... (Ctrl+C to exit)");

    let (unit, _) = parse_target(target)?;
    let git_root = get_git_root(Path::new(&unit))?;

    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(
        Duration::from_millis(500),
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )?;

    let mut watched_roots = HashSet::new();
    for root in watch_roots {
        if watched_roots.insert(root.clone()) {
            debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;
        }
    }

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let saw_relevant_change = events
                    .iter()
                    .any(|event| event_is_relevant(&event.path, &git_root));
                if !saw_relevant_change {
                    continue;
                }

                eprintln!("change detected; re-running target graph");
                if let Err(error) = run_once(target, false, output_mode, append_cmd) {
                    eprintln!("watch re-run failed: {error}");
                }
                eprintln!("watching for changes... (Ctrl+C to exit)");
            }
            Ok(Err(error)) => {
                eprintln!("watch error: {error}");
            }
            Err(_) => break,
        }
    }

    Ok(())
}

pub fn cmd_run_command(
    target: &str,
    force: bool,
    quiet: bool,
    verbose: bool,
    watch: bool,
    append_cmd: Option<String>,
) -> Result<()> {
    let output_mode = output_mode(quiet, verbose);

    let initial = run_once(target, force, output_mode, &append_cmd)?;
    if !watch {
        return Ok(());
    }

    watch_target_graph(target, output_mode, &append_cmd, initial.watch_roots)
}
