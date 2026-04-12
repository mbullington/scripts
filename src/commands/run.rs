use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::Result;
use colored::*;
use dagrs::{log as daglog, Action, Dag, DefaultTask, EnvVar, Input, LogLevel, Output, Task};
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use sha2::{Digest, Sha256};

use crate::helpers::{
    git::get_git_root,
    graph::{build_task_graph, TaskGraph},
    path::build_path_var,
    resolve::parse_target,
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

impl Action for MaybeCommandAction {
    fn run(&self, input: Input, _env: Arc<EnvVar>) -> Result<Output, dagrs::RunningError> {
        if !self.run {
            print_status(self.output_mode, "CACHED", &self.name, None);
            return Ok(Output::empty());
        }

        let command = match &self.command {
            Some(command) => command,
            None => {
                print_status(self.output_mode, "OK", &self.name, Some("(no command)"));
                self.results.lock().unwrap().insert(self.idx, true);
                if let Some(hash) = &self.cache_hash {
                    persist_cache_change(&self.cache, &self.cache_path, |cache| {
                        cache.insert(self.cache_key.clone(), hash.clone());
                    })
                    .unwrap_or_else(|error| eprintln!("warning: failed to save cache: {error}"));
                }
                return Ok(Output::empty());
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

        self.results.lock().unwrap().insert(self.idx, success);

        if success {
            if let Some(hash) = &self.cache_hash {
                persist_cache_change(&self.cache, &self.cache_path, |cache| {
                    cache.insert(self.cache_key.clone(), hash.clone());
                })
                .unwrap_or_else(|error| eprintln!("warning: failed to save cache: {error}"));
            }
            print_status(self.output_mode, "OK", &self.name, None);
            Ok(Output::empty())
        } else {
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

pub fn cmd_run_command(
    target: &str,
    force: bool,
    quiet: bool,
    verbose: bool,
    append_cmd: Option<String>,
) -> Result<()> {
    daglog::init_logger(LogLevel::Off, None);

    let output_mode = if verbose {
        RunOutputMode::Verbose
    } else if quiet {
        RunOutputMode::Quiet
    } else {
        RunOutputMode::Normal
    };

    let (unit, task_name) = parse_target(target);
    let unit_path = Path::new(&unit);
    let git_root = get_git_root(unit_path)?;
    let cache_path = git_root.join(".scripts_cache");
    let cache = Arc::new(Mutex::new(load_cache(&cache_path)?));

    struct Planned {
        key: String,
        hash: Option<String>,
        run: bool,
    }

    let graph: TaskGraph = build_task_graph(unit_path, &task_name)?;
    let root_index = graph.scripts.len().saturating_sub(1);

    let mut bins = Vec::new();
    for node in &graph.scripts {
        if let Some(bin) = &node.task.bin {
            for path in bin {
                bins.push(node.unit_path.join(path));
            }
        }
    }

    let mut planned = Vec::with_capacity(graph.scripts.len());
    for (idx, node) in graph.scripts.iter().enumerate() {
        let key = format!("{}:{}", node.unit_path.display(), node.task_name);
        let mut hasher = Sha256::new();
        let mut needs_hash = false;

        if let Some(patterns) = &node.task.watch {
            let command = if idx == root_index {
                combine_command(&node.task.command, append_cmd.as_ref())
            } else {
                node.task.command.clone()
            };

            if let Some(command) = &command {
                hasher.update(command.as_bytes());
                needs_hash = true;
            }

            for pattern in patterns {
                let watch_hash = compute_watch_hash(&node.unit_path, pattern)?;
                hasher.update(watch_hash.as_bytes());
                needs_hash = true;
            }
        }

        let hash = needs_hash.then(|| hex::encode(hasher.finalize()));

        let mut run = true;
        if let Some(hash) = &hash {
            if !force {
                let cache_lock = cache.lock().unwrap();
                if cache_lock
                    .get(&key)
                    .is_some_and(|previous| previous == hash)
                {
                    run = false;
                }
            }
        }

        if !needs_hash && node.task.watch.is_some() {
            run = false;
        }

        planned.push(Planned { key, hash, run });
    }

    for idx in 0..planned.len() {
        if !planned[idx].run
            && graph.scripts[idx]
                .dependencies
                .iter()
                .any(|dep| planned[*dep].run)
        {
            planned[idx].run = true;
        }
    }

    let mut dag_scripts = Vec::with_capacity(graph.scripts.len());
    let results: Arc<Mutex<HashMap<usize, bool>>> = Arc::new(Mutex::new(HashMap::new()));

    for (idx, node) in graph.scripts.iter().enumerate() {
        let relative_path = node
            .unit_path
            .strip_prefix(&git_root)
            .unwrap_or(&node.unit_path);
        let name = format!("{}:{}", relative_path.display(), node.task_name);
        let path_var = build_path_var(&node.unit_path, &bins)?;
        let command = if idx == root_index {
            combine_command(&node.task.command, append_cmd.as_ref())
        } else {
            node.task.command.clone()
        };

        let action = MaybeCommandAction {
            command,
            run: planned[idx].run,
            name: name.clone(),
            dir: node.unit_path.clone(),
            idx,
            path_var,
            output_mode,
            results: results.clone(),
            cache_key: planned[idx].key.clone(),
            cache_hash: planned[idx].hash.clone(),
            cache: cache.clone(),
            cache_path: cache_path.clone(),
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
            let relative_path = node
                .unit_path
                .strip_prefix(&git_root)
                .unwrap_or(&node.unit_path);
            let name = format!("{}:{}", relative_path.display(), node.task_name);
            print_status(output_mode, "SKIP", &name, Some("(dependency failed)"));
        }
    }

    Ok(())
}
