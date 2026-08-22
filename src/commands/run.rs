use std::{
    collections::HashMap,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use notify_debouncer_mini::{
    new_debouncer,
    notify::{RecursiveMode, Watcher},
    DebounceEventResult,
};

use crate::helpers::{
    cache::TaskCache,
    graph::{build_target_graph, TaskGraph},
    resolve::read_workspace_config,
    task_list::print_tasks_for_current_unit,
};

use super::{
    run_executor::{execute_plan, RunOutputMode, TaskEvent},
    run_plan::RunPlan,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchDepth {
    NonRecursive,
    Recursive,
}

impl WatchDepth {
    fn notify_mode(self) -> RecursiveMode {
        match self {
            Self::NonRecursive => RecursiveMode::NonRecursive,
            Self::Recursive => RecursiveMode::Recursive,
        }
    }
}

struct RunOnceResult {
    watch_paths: HashMap<PathBuf, WatchDepth>,
    execution_error: Option<anyhow::Error>,
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

fn collect_watch_paths(graph: &TaskGraph, git_root: &Path) -> HashMap<PathBuf, WatchDepth> {
    let mut paths = HashMap::new();

    for node in &graph.scripts {
        if node.task.watch.is_some() {
            paths.insert(node.unit_path.clone(), WatchDepth::Recursive);
        }
    }

    if !paths.is_empty() {
        paths
            .entry(git_root.to_path_buf())
            .or_insert(WatchDepth::NonRecursive);
    }

    paths
}

fn apply_cache_events(cache: &TaskCache, events: &[TaskEvent]) -> Result<()> {
    for event in events {
        match event {
            TaskEvent::Succeeded {
                cache_key,
                cache_hash: Some(cache_hash),
            } => cache.store(cache_key, cache_hash)?,
            TaskEvent::Succeeded {
                cache_key,
                cache_hash: None,
            } => cache.remove(cache_key)?,
            TaskEvent::Failed { cache_key } => cache.remove(cache_key)?,
        }
    }

    Ok(())
}

fn run_once(
    target: &str,
    force: bool,
    output_mode: RunOutputMode,
    jobs: usize,
    append_cmd: &Option<String>,
) -> Result<RunOnceResult> {
    let cwd = std::env::current_dir()?;
    let (graph, git_root) = match build_target_graph(target, &cwd) {
        Ok(result) => result,
        Err(error) => {
            print_tasks_for_current_unit();
            return Err(error);
        }
    };
    let cache = TaskCache::open(&git_root)?;
    let workspace_config = read_workspace_config(&git_root)?;

    let plan = RunPlan::build(
        &graph,
        &git_root,
        &cache,
        force,
        append_cmd.as_ref(),
        workspace_config.as_ref(),
    )?;
    let outcome = execute_plan(&plan, output_mode, jobs);

    apply_cache_events(&cache, &outcome.events)?;

    Ok(RunOnceResult {
        watch_paths: collect_watch_paths(&graph, &git_root),
        execution_error: outcome.into_result().err(),
    })
}

fn event_is_relevant(
    path: &Path,
    git_root: &Path,
    watched_paths: &HashMap<PathBuf, WatchDepth>,
) -> bool {
    if path.starts_with(git_root.join(".scripts_cache")) || path.starts_with(git_root.join(".git"))
    {
        return false;
    }

    if path.parent() == Some(git_root)
        && watched_paths.get(git_root) == Some(&WatchDepth::NonRecursive)
    {
        return path
            .file_name()
            .is_some_and(|name| name == "SCRIPTS_WORKSPACE.toml");
    }

    true
}

fn reconcile_watch_paths<W: Watcher + ?Sized>(
    watcher: &mut W,
    current: &mut HashMap<PathBuf, WatchDepth>,
    desired: HashMap<PathBuf, WatchDepth>,
) -> Result<()> {
    for (path, depth) in current.iter() {
        if desired.get(path) != Some(depth) {
            watcher.unwatch(path)?;
        }
    }
    for (path, depth) in &desired {
        if current.get(path) != Some(depth) {
            watcher.watch(path, depth.notify_mode())?;
        }
    }
    *current = desired;
    Ok(())
}

fn watch_target_graph(
    target: &str,
    output_mode: RunOutputMode,
    jobs: usize,
    append_cmd: &Option<String>,
    watch_paths: HashMap<PathBuf, WatchDepth>,
) -> Result<()> {
    if watch_paths.is_empty() {
        eprintln!("watch mode requested, but no watched tasks were found in the target graph");
        return Ok(());
    }

    eprintln!("watching for changes... (Ctrl+C to exit)");

    let cwd = std::env::current_dir()?;
    let (_, git_root) = build_target_graph(target, &cwd)?;

    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(
        Duration::from_millis(500),
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )?;

    let mut watched_paths = HashMap::new();
    reconcile_watch_paths(debouncer.watcher(), &mut watched_paths, watch_paths)?;

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let saw_relevant_change = events
                    .iter()
                    .any(|event| event_is_relevant(&event.path, &git_root, &watched_paths));
                if !saw_relevant_change {
                    continue;
                }

                eprintln!("change detected; re-running target graph");
                match run_once(target, false, output_mode, jobs, append_cmd) {
                    Ok(result) => {
                        reconcile_watch_paths(
                            debouncer.watcher(),
                            &mut watched_paths,
                            result.watch_paths,
                        )?;
                        if let Some(error) = result.execution_error {
                            eprintln!("watch re-run failed: {error}");
                        }
                        if watched_paths.is_empty() {
                            eprintln!(
                                "watch mode stopped because no watched tasks remain in the target graph"
                            );
                            return Ok(());
                        }
                    }
                    Err(error) => eprintln!("watch re-run failed: {error}"),
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
    jobs: Option<NonZeroUsize>,
    append_cmd: Option<String>,
) -> Result<()> {
    let output_mode = output_mode(quiet, verbose);
    let jobs = jobs.map(NonZeroUsize::get).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1)
    });

    let initial = run_once(target, force, output_mode, jobs, &append_cmd)?;
    if let Some(error) = initial.execution_error {
        return Err(error);
    }
    if !watch {
        return Ok(());
    }

    watch_target_graph(target, output_mode, jobs, &append_cmd, initial.watch_paths)
}
