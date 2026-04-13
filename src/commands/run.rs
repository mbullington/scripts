use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use colored::*;
use dagrs::{log as daglog, Action, Dag, DefaultTask, EnvVar, Input, LogLevel, Output, Task};
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};
use sha2::{Digest, Sha256};

use crate::helpers::{
    git::get_git_root,
    graph::{build_task_graph, TaskGraph},
    path::{build_path_var, collect_task_bins, resolve_workspace_bins},
    readiness::wait_for_readiness,
    resolve::{parse_target, read_workspace_config},
    scripts_def::{Readiness, RestartPolicy},
    task_list::print_tasks_for_current_unit,
};

#[derive(Clone, Copy, Debug)]
enum RunOutputMode {
    Normal,
    Quiet,
    Verbose,
}

impl RunOutputMode {
    fn shows_status(self, label: &str) -> bool {
        !matches!(self, Self::Quiet) || label == "FAIL"
    }

    fn is_verbose(self) -> bool {
        matches!(self, Self::Verbose)
    }
}

struct MaybeCommandAction {
    command: Option<String>,
    run: bool,
    name: String,
    dir: PathBuf,
    idx: usize,
    path_var: std::ffi::OsString,
    output_mode: RunOutputMode,
    results: Arc<Mutex<HashMap<usize, bool>>>,
    cache_key: String,
    cache_hash: Option<String>,
    cache: Arc<Mutex<HashMap<String, String>>>,
    cache_path: PathBuf,
    readiness: Option<Readiness>,
}

fn status_label(label: &str) -> colored::ColoredString {
    match label {
        "RUN" => label.bold().blue(),
        "OK" => label.bold().green(),
        "CACHED" => label.bold().bright_black(),
        "SKIP" => label.bold().yellow(),
        "FAIL" => label.bold().red(),
        _ => label.normal(),
    }
}

fn print_status(output_mode: RunOutputMode, label: &str, name: &str, detail: Option<&str>) {
    if !output_mode.shows_status(label) {
        return;
    }

    let label = status_label(label);
    match detail {
        Some(detail) => eprintln!("{label} {name} {detail}"),
        None => eprintln!("{label} {name}"),
    }
}

fn print_verbose_command(output_mode: RunOutputMode, dir: &Path, command: &str) {
    if !output_mode.is_verbose() {
        return;
    }

    eprintln!("    cwd: {}", dir.display());
    eprintln!("    cmd:");
    for line in command.lines() {
        eprintln!("      {line}");
    }
}

impl MaybeCommandAction {
    fn on_success(&self) -> Result<Output, dagrs::RunningError> {
        if let Some(readiness) = &self.readiness {
            if let Err(error) = wait_for_readiness(readiness, &self.name) {
                persist_cache_change(&self.cache, &self.cache_path, |cache| {
                    cache.remove(&self.cache_key);
                })
                .unwrap_or_else(|save_error| {
                    eprintln!("warning: failed to save cache: {save_error}")
                });
                self.results.lock().unwrap().insert(self.idx, false);
                let detail = format!("({error})");
                print_status(self.output_mode, "FAIL", &self.name, Some(&detail));
                return Err(dagrs::RunningError::new(error.to_string()));
            }
        }

        if let Some(hash) = &self.cache_hash {
            persist_cache_change(&self.cache, &self.cache_path, |cache| {
                cache.insert(self.cache_key.clone(), hash.clone());
            })
            .unwrap_or_else(|error| eprintln!("warning: failed to save cache: {error}"));
        }
        self.results.lock().unwrap().insert(self.idx, true);
        print_status(self.output_mode, "OK", &self.name, None);
        Ok(Output::empty())
    }
}

impl Action for MaybeCommandAction {
    fn run(&self, input: Input, _env: Arc<EnvVar>) -> Result<Output, dagrs::RunningError> {
        if !self.run {
            print_status(self.output_mode, "CACHED", &self.name, None);
            return Ok(Output::empty());
        }

        let command = match &self.command {
            Some(command) => command,
            None => {
                print_status(self.output_mode, "RUN", &self.name, Some("(no command)"));
                return self.on_success();
            }
        };

        print_status(self.output_mode, "RUN", &self.name, None);
        print_verbose_command(self.output_mode, &self.dir, command);

        let mut args = Vec::from(["-c", command.as_str()]);
        input.get_iter().for_each(|input| {
            if let Some(value) = input.get::<String>() {
                args.push(value);
            }
        });

        use std::process::Stdio;

        let status = std::process::Command::new("sh")
            .args(args)
            .current_dir(&self.dir)
            .env("PATH", &self.path_var)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(dagrs::RunningError::from_err)?;
        let success = status.success();
        let exit_code = status.code();

        if success {
            self.on_success()
        } else {
            self.results.lock().unwrap().insert(self.idx, false);
            persist_cache_change(&self.cache, &self.cache_path, |cache| {
                cache.remove(&self.cache_key);
            })
            .unwrap_or_else(|error| eprintln!("warning: failed to save cache: {error}"));
            let detail = format!("(exit code: {exit_code:?})");
            print_status(self.output_mode, "FAIL", &self.name, Some(&detail));
            Err(dagrs::RunningError::new(format!(
                "task {} failed with exit code {exit_code:?}",
                self.name
            )))
        }
    }
}

fn persist_cache_change(
    cache: &Arc<Mutex<HashMap<String, String>>>,
    path: &Path,
    change: impl FnOnce(&mut HashMap<String, String>),
) -> Result<()> {
    let mut cache = cache.lock().unwrap();
    change(&mut cache);
    save_cache(path, &cache)
}

fn hash_file(path: &Path) -> Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().into())
}

fn compute_watch_hash(root: &Path, pattern: &str) -> Result<String> {
    let walker = if pattern == "." {
        WalkBuilder::new(root).hidden(false).build()
    } else {
        let mut overrides = OverrideBuilder::new(root);
        let adjusted_pattern =
            if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
                let candidate = root.join(pattern);
                if candidate.is_dir() {
                    if pattern.ends_with('/') {
                        format!("{pattern}**")
                    } else {
                        format!("{pattern}/**")
                    }
                } else {
                    pattern.to_string()
                }
            } else {
                pattern.to_string()
            };

        overrides.add(&adjusted_pattern)?;
        WalkBuilder::new(root).overrides(overrides.build()?).build()
    };

    let mut entries = Vec::new();
    for result in walker {
        let entry = result?;
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let relative_path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        let digest = hash_file(entry.path())?;
        entries.push((relative_path, digest));
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (path, digest) in entries {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(digest);
        hasher.update([0]);
    }

    Ok(hex::encode(hasher.finalize()))
}

fn load_cache(path: &Path) -> Result<HashMap<String, String>> {
    if path.exists() {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    } else {
        Ok(HashMap::new())
    }
}

fn save_cache(path: &Path, cache: &HashMap<String, String>) -> Result<()> {
    std::fs::write(path, serde_json::to_string(cache)?)?;
    Ok(())
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

fn task_command(
    graph: &TaskGraph,
    idx: usize,
    root_index: usize,
    append_cmd: &Option<String>,
) -> Option<String> {
    let node = &graph.scripts[idx];
    if idx == root_index {
        combine_command(&node.task.command, append_cmd.as_ref())
    } else {
        node.task.command.clone()
    }
}

fn task_display_name(node: &crate::helpers::graph::TaskGraphNode, git_root: &Path) -> String {
    let relative_path = node
        .unit_path
        .strip_prefix(git_root)
        .unwrap_or(&node.unit_path);
    format!("{}:{}", relative_path.display(), node.task_name)
}

fn should_skip_for_watch_rerun(watch_triggered: bool, restart_policy: &RestartPolicy) -> bool {
    watch_triggered && matches!(restart_policy, RestartPolicy::Never)
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

struct PlannedTask {
    key: String,
    hash: Option<String>,
    run: bool,
}

struct RunOnceResult {
    watch_roots: Vec<PathBuf>,
}

fn compute_task_hash(
    node: &crate::helpers::graph::TaskGraphNode,
    command: &Option<String>,
) -> Result<Option<String>> {
    let Some(patterns) = &node.task.watch else {
        return Ok(None);
    };

    let mut hasher = Sha256::new();
    let mut needs_hash = false;

    if let Some(command) = command {
        hasher.update(command.as_bytes());
        needs_hash = true;
    }

    for pattern in patterns {
        let watch_hash = compute_watch_hash(&node.unit_path, pattern)?;
        hasher.update(watch_hash.as_bytes());
        needs_hash = true;
    }

    Ok(needs_hash.then(|| hex::encode(hasher.finalize())))
}

fn should_run_task(
    node: &crate::helpers::graph::TaskGraphNode,
    key: &str,
    hash: &Option<String>,
    force: bool,
    cache: &HashMap<String, String>,
    watch_triggered: bool,
) -> bool {
    if should_skip_for_watch_rerun(watch_triggered, &node.task.restart_policy) {
        return false;
    }

    if node.task.watch.is_none() {
        return true;
    }

    let Some(hash) = hash else {
        return false;
    };

    if force {
        return true;
    }

    !cache.get(key).is_some_and(|previous| previous == hash)
}

fn build_run_plan(
    graph: &TaskGraph,
    root_index: usize,
    append_cmd: &Option<String>,
    force: bool,
    cache: &Arc<Mutex<HashMap<String, String>>>,
    watch_triggered: bool,
) -> Result<Vec<PlannedTask>> {
    let cache_snapshot = cache.lock().unwrap().clone();
    let mut planned = Vec::with_capacity(graph.scripts.len());

    for (idx, node) in graph.scripts.iter().enumerate() {
        let key = format!("{}:{}", node.unit_path.display(), node.task_name);
        let command = task_command(graph, idx, root_index, append_cmd);
        let hash = compute_task_hash(node, &command)?;
        let run = should_run_task(node, &key, &hash, force, &cache_snapshot, watch_triggered);

        planned.push(PlannedTask { key, hash, run });
    }

    for idx in 0..planned.len() {
        let dependency_ran = graph.scripts[idx]
            .dependencies
            .iter()
            .any(|dep| planned[*dep].run);

        if !planned[idx].run
            && dependency_ran
            && !should_skip_for_watch_rerun(
                watch_triggered,
                &graph.scripts[idx].task.restart_policy,
            )
        {
            planned[idx].run = true;
        }
    }

    Ok(planned)
}

fn run_once(
    target: &str,
    force: bool,
    output_mode: RunOutputMode,
    append_cmd: &Option<String>,
    watch_triggered: bool,
) -> Result<RunOnceResult> {
    let (unit, task_name) = parse_target(target)?;
    let unit_path = Path::new(&unit);
    let git_root = get_git_root(unit_path)?;
    let cache_path = git_root.join(".scripts_cache");
    let cache = Arc::new(Mutex::new(load_cache(&cache_path)?));
    let workspace_config = read_workspace_config(&git_root);

    let graph: TaskGraph = match build_task_graph(unit_path, &task_name) {
        Ok(graph) => graph,
        Err(error) => {
            print_tasks_for_current_unit();
            return Err(error.into());
        }
    };
    let root_index = graph.scripts.len().saturating_sub(1);

    let planned = build_run_plan(
        &graph,
        root_index,
        append_cmd,
        force,
        &cache,
        watch_triggered,
    )?;

    let mut dag_scripts = Vec::with_capacity(graph.scripts.len());
    let results: Arc<Mutex<HashMap<usize, bool>>> = Arc::new(Mutex::new(HashMap::new()));

    for (idx, node) in graph.scripts.iter().enumerate() {
        let name = task_display_name(node, &git_root);

        let mut bins = collect_task_bins(&graph, idx);
        bins.extend(resolve_workspace_bins(
            &git_root,
            &node.unit_path,
            workspace_config.as_ref(),
        ));

        let command = task_command(&graph, idx, root_index, append_cmd);

        let action = MaybeCommandAction {
            command,
            run: planned[idx].run,
            name: name.clone(),
            dir: node.unit_path.clone(),
            idx,
            path_var: build_path_var(&bins)?,
            output_mode,
            results: results.clone(),
            cache_key: planned[idx].key.clone(),
            cache_hash: planned[idx].hash.clone(),
            cache: cache.clone(),
            cache_path: cache_path.clone(),
            readiness: node.task.readiness.clone(),
        };
        dag_scripts.push(DefaultTask::new(action, &name));
    }

    for (idx, node) in graph.scripts.iter().enumerate() {
        let dep_ids: Vec<usize> = node
            .dependencies
            .iter()
            .map(|&handle| dag_scripts[handle].id())
            .collect();
        dag_scripts[idx].set_predecessors_by_id(&dep_ids);
    }

    let mut dag = Dag::with_tasks(dag_scripts);
    let _ = dag.start()?;

    let results_map = results.lock().unwrap();
    for (idx, node) in graph.scripts.iter().enumerate() {
        if planned[idx].run && !results_map.contains_key(&idx) {
            let name = task_display_name(node, &git_root);
            print_status(output_mode, "SKIP", &name, Some("(dependency failed)"));
        }
    }
    drop(results_map);

    Ok(RunOnceResult {
        watch_roots: collect_watch_roots(&graph),
    })
}

fn event_is_relevant(path: &Path, git_root: &Path) -> bool {
    path != git_root.join(".scripts_cache") && !path.starts_with(git_root.join(".git"))
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
                if let Err(error) = run_once(target, false, output_mode, append_cmd, true) {
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
    daglog::init_logger(LogLevel::Off, None);

    let output_mode = output_mode(quiet, verbose);

    let initial = run_once(target, force, output_mode, &append_cmd, false)?;
    if !watch {
        return Ok(());
    }

    watch_target_graph(target, output_mode, &append_cmd, initial.watch_roots)
}
