use std::{
    fs::read_to_string,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result as AnyhowResult};

use crate::helpers::scripts_def::{ScriptsDef, WorkspaceConfig};

use super::git::{get_git_root, GitError};

#[derive(Debug)]
#[allow(dead_code)]
pub enum ResolveScriptsError {
    DoesNotExist(&'static str),
    /// DoesNotExist with a dynamic message (e.g., includes task name)
    DoesNotExistMsg(String),
    InvalidTarget(String),
    DependencyCycle(Vec<String>),
    Toml(toml::de::Error),
    IO(std::io::Error),
    GitError(GitError),
}

/// Units that list other units as dependencies have the following "search path":
/// - `(unit root)/..`
/// - `(unit root)/../..`
/// - So on and so forth until `(git root)`
pub fn resolve_scripts_path(
    path: &str,
    working_dir: &Path,
) -> Result<PathBuf, ResolveScriptsError> {
    let git_root = get_git_root(working_dir).map_err(ResolveScriptsError::GitError)?;

    let mut current_dir = PathBuf::from(working_dir)
        .canonicalize()
        .map_err(ResolveScriptsError::IO)?;
    loop {
        let candidate = current_dir.join(path);
        if candidate.join("SCRIPTS").is_file() {
            return candidate.canonicalize().map_err(ResolveScriptsError::IO);
        }

        if current_dir == git_root {
            break;
        }

        current_dir = current_dir
            .join("..")
            .canonicalize()
            .map_err(ResolveScriptsError::IO)?;
    }

    Err(ResolveScriptsError::DoesNotExist("Unit not found"))
}

pub fn read_scripts(path: &Path) -> Result<ScriptsDef, ResolveScriptsError> {
    if !path.exists() {
        return Err(ResolveScriptsError::DoesNotExist(
            "Directory for unit not found",
        ));
    }
    if !path.is_dir() {
        return Err(ResolveScriptsError::DoesNotExist(
            "Unit path is not a directory",
        ));
    }

    let scripts_path = path.join("SCRIPTS");
    if !scripts_path.exists() {
        return Err(ResolveScriptsError::DoesNotExist("SCRIPTS file not found"));
    }

    let contents = match read_to_string(scripts_path) {
        Ok(contents) => contents,
        Err(e) => return Err(ResolveScriptsError::IO(e)),
    };

    match toml::from_str(&contents) {
        Ok(def) => Ok(def),
        Err(e) => Err(ResolveScriptsError::Toml(e)),
    }
}

fn split_explicit_target(target: &str) -> std::result::Result<Option<(&str, &str)>, ()> {
    if let Some(pos) = target.rfind(':') {
        let (path, task) = target.split_at(pos);
        let task = &task[1..];
        if task.is_empty() {
            return Err(());
        }
        return Ok(Some((path, task)));
    }

    Ok(None)
}

fn looks_like_unit_path(target: &str) -> bool {
    target.contains('/')
        || target == "."
        || target == ".."
        || target.starts_with("./")
        || target.starts_with("../")
        || Path::new(target).exists()
}

/// Split a CLI task target into the unit path and task name.
pub fn parse_target(target: &str) -> AnyhowResult<(String, String)> {
    if let Some((path, task)) = split_explicit_target(target).map_err(|()| {
        anyhow::anyhow!(
            "invalid target '{target}'. Missing task name after ':'. Use 'build' for the current unit or '<unit>:build' for another unit"
        )
    })? {
        return Ok((
            if path.is_empty() { "." } else { path }.to_string(),
            task.to_string(),
        ));
    }

    if looks_like_unit_path(target) {
        bail!(
            "invalid target '{target}'. Units must include a task name. Use '<unit>:<task>' for another unit or '<task>' for the current unit"
        );
    }

    Ok((".".to_string(), target.to_string()))
}

/// Split a dependency reference into an optional unit path and task name.
pub fn parse_dependency(dep: &str) -> Result<(String, String), ResolveScriptsError> {
    if let Some((path, task)) = split_explicit_target(dep).map_err(|()| {
        ResolveScriptsError::InvalidTarget(format!(
            "invalid dependency '{dep}'. Missing task name after ':'"
        ))
    })? {
        return Ok((path.to_string(), task.to_string()));
    }

    if looks_like_unit_path(dep) {
        return Err(ResolveScriptsError::InvalidTarget(format!(
            "invalid dependency '{dep}'. Use '<unit>:<task>' for another unit or '<task>' for the current unit"
        )));
    }

    Ok((String::new(), dep.to_string()))
}

pub fn read_workspace_config(git_root: &Path) -> Option<WorkspaceConfig> {
    let workspace_path = git_root.join("SCRIPTS_WORKSPACE.toml");
    if !workspace_path.is_file() {
        return None;
    }

    let contents = read_to_string(workspace_path).ok()?;
    toml::from_str(&contents).ok()
}

#[cfg(test)]
mod tests {
    use super::parse_target;

    #[test]
    fn parse_target_handles_explicit_unit_and_task() {
        assert_eq!(
            parse_target("tools/pkg:build").unwrap(),
            ("tools/pkg".into(), "build".into())
        );
        assert_eq!(parse_target(":dev").unwrap(), (".".into(), "dev".into()));
    }

    #[test]
    fn parse_target_treats_plain_name_as_current_unit_task() {
        assert_eq!(parse_target("build").unwrap(), (".".into(), "build".into()));
    }

    #[test]
    fn parse_target_rejects_path_like_targets_without_task_names() {
        assert!(parse_target("./tools/pkg").is_err());
        assert!(parse_target("..").is_err());
    }
}
