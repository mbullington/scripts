use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    resolve::{read_scripts, resolve_scripts_path, ResolveScriptsError},
    scripts_def::Task,
};

pub type TaskGraphHandle = usize;

#[derive(Debug, Clone)]
pub struct TaskGraphNode {
    pub unit_path: PathBuf,
    pub task_name: String,
    pub task: Task,
    pub dependencies: Vec<TaskGraphHandle>,
}

#[derive(Debug)]
pub struct TaskGraph {
    pub scripts: Vec<TaskGraphNode>,
    pub handle_map: HashMap<(PathBuf, String), TaskGraphHandle>,
}

#[derive(Error, Debug)]
pub struct TaggedResolveScriptsError {
    pub path: PathBuf,
    pub error: ResolveScriptsError,
}

impl Display for TaggedResolveScriptsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use super::resolve::ResolveScriptsError as E;
        match &self.error {
            E::DoesNotExist("Unit not found") => write!(
                f,
                "{}: unit not found. Expected a directory containing a SCRIPTS file.",
                self.path.display()
            ),
            E::DoesNotExist("SCRIPTS file not found") => write!(
                f,
                "{}: SCRIPTS file not found. Add a SCRIPTS file to this unit or pick a different target.",
                self.path.display()
            ),
            E::DoesNotExist(msg) => write!(f, "{}: {msg}", self.path.display()),
            E::DoesNotExistMsg(msg) if msg.starts_with("Task '") => write!(
                f,
                "{}: {msg}. Run `scripts` in that unit to list available tasks.",
                self.path.display()
            ),
            E::DoesNotExistMsg(msg) => write!(f, "{}: {msg}", self.path.display()),
            E::InvalidTarget(msg) => write!(f, "{}: {msg}", self.path.display()),
            E::DependencyCycle(cycle) => {
                write!(
                    f,
                    "{}: dependency cycle detected: {}. Remove or restructure one of these dependencies.",
                    self.path.display(),
                    cycle.join(" -> ")
                )
            }
            E::Toml(e) => write!(
                f,
                "{}: invalid SCRIPTS file: {}. Define tasks as top-level tables like [build], and check values like watch = [\"src/**\"] and deps = [\":build\"].",
                self.path.display(),
                e
            ),
            E::IO(e) => write!(f, "{}: I/O error: {e}", self.path.display()),
            E::GitError(e) => write!(f, "{}: {e}", self.path.display()),
        }
    }
}

fn parse_dep(dep: &str) -> Result<(String, String), ResolveScriptsError> {
    if let Some(pos) = dep.rfind(':') {
        let (path, task) = dep.split_at(pos);
        let task = &task[1..];
        if task.is_empty() {
            return Err(ResolveScriptsError::InvalidTarget(format!(
                "invalid dependency '{dep}'. Missing task name after ':'"
            )));
        }

        return Ok((path.to_string(), task.to_string()));
    }

    let looks_like_path = dep.contains('/')
        || dep == "."
        || dep == ".."
        || dep.starts_with("./")
        || dep.starts_with("../");

    if looks_like_path {
        return Err(ResolveScriptsError::InvalidTarget(format!(
            "invalid dependency '{dep}'. Use '<unit>:<task>' for another unit or '<task>' for the current unit"
        )));
    }

    Ok((String::new(), dep.to_string()))
}

fn format_task_ref(unit_path: &Path, task_name: &str) -> String {
    format!("{}:{}", unit_path.display(), task_name)
}

pub fn build_task_graph(
    initial_path: &Path,
    task: &str,
) -> Result<TaskGraph, TaggedResolveScriptsError> {
    let mut scripts = Vec::new();
    let mut handle_map = HashMap::<(PathBuf, String), usize>::new();
    let mut visiting = Vec::<(PathBuf, String)>::new();

    fn add_task(
        unit_path: PathBuf,
        task_name: String,
        scripts: &mut Vec<TaskGraphNode>,
        map: &mut HashMap<(PathBuf, String), usize>,
        visiting: &mut Vec<(PathBuf, String)>,
    ) -> Result<usize, TaggedResolveScriptsError> {
        if let Some(handle) = map.get(&(unit_path.clone(), task_name.clone())) {
            return Ok(*handle);
        }

        if let Some(cycle_start) = visiting
            .iter()
            .position(|entry| entry == &(unit_path.clone(), task_name.clone()))
        {
            let mut cycle: Vec<String> = visiting[cycle_start..]
                .iter()
                .map(|(path, task)| format_task_ref(path, task))
                .collect();
            cycle.push(format_task_ref(&unit_path, &task_name));
            return Err(TaggedResolveScriptsError {
                path: unit_path,
                error: ResolveScriptsError::DependencyCycle(cycle),
            });
        }

        visiting.push((unit_path.clone(), task_name.clone()));

        let result = (|| {
            let def = read_scripts(&unit_path).map_err(|error| TaggedResolveScriptsError {
                path: unit_path.clone(),
                error,
            })?;
            let task_def = def
                .scripts
                .get(&task_name)
                .ok_or(TaggedResolveScriptsError {
                    path: unit_path.clone(),
                    error: ResolveScriptsError::DoesNotExistMsg(format!(
                        "Task '{task_name}' not found"
                    )),
                })?
                .clone();

            let mut dependencies = Vec::new();
            if let Some(deps) = &task_def.deps {
                for dep in deps {
                    let (dep_unit, dep_task) =
                        parse_dep(dep).map_err(|error| TaggedResolveScriptsError {
                            path: unit_path.clone(),
                            error,
                        })?;
                    let dep_path = if dep_unit.is_empty() {
                        unit_path.clone()
                    } else {
                        resolve_scripts_path(&dep_unit, &unit_path).map_err(|error| {
                            TaggedResolveScriptsError {
                                path: unit_path.clone(),
                                error,
                            }
                        })?
                    };
                    let dep_handle = add_task(dep_path, dep_task, scripts, map, visiting)?;
                    dependencies.push(dep_handle);
                }
            }

            let handle = scripts.len();
            map.insert((unit_path.clone(), task_name.clone()), handle);
            scripts.push(TaskGraphNode {
                unit_path,
                task_name,
                task: task_def,
                dependencies,
            });
            Ok(handle)
        })();

        visiting.pop();
        result
    }

    let initial_path = initial_path
        .canonicalize()
        .map_err(|error| TaggedResolveScriptsError {
            path: initial_path.to_path_buf(),
            error: ResolveScriptsError::IO(error),
        })?;

    add_task(
        initial_path,
        task.to_string(),
        &mut scripts,
        &mut handle_map,
        &mut visiting,
    )?;

    Ok(TaskGraph {
        scripts,
        handle_map,
    })
}
