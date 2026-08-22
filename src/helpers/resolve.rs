use std::{
    fs::read_to_string,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result as AnyhowResult};

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
    let git_root = get_git_root(working_dir)
        .map_err(ResolveScriptsError::GitError)?
        .canonicalize()
        .map_err(ResolveScriptsError::IO)?;

    let mut current_dir = PathBuf::from(working_dir)
        .canonicalize()
        .map_err(ResolveScriptsError::IO)?;
    loop {
        let candidate = current_dir.join(path);
        if candidate.join("SCRIPTS").is_file() {
            let candidate = candidate.canonicalize().map_err(ResolveScriptsError::IO)?;
            if !candidate.starts_with(&git_root) {
                return Err(ResolveScriptsError::InvalidTarget(format!(
                    "unit path '{path}' resolves outside the git repository"
                )));
            }
            return Ok(candidate);
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

pub fn find_enclosing_unit(
    working_dir: &Path,
    git_root: &Path,
) -> Result<PathBuf, ResolveScriptsError> {
    let git_root = git_root.canonicalize().map_err(ResolveScriptsError::IO)?;
    let mut current = working_dir
        .canonicalize()
        .map_err(ResolveScriptsError::IO)?;

    while current.starts_with(&git_root) {
        if current.join("SCRIPTS").is_file() {
            return Ok(current);
        }
        if current == git_root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }

    Err(ResolveScriptsError::DoesNotExist("SCRIPTS file not found"))
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

/// Split a CLI task target into the unit path and task name.
pub fn parse_target(target: &str) -> AnyhowResult<(String, String)> {
    if let Some((path, task)) = split_explicit_target(target).map_err(|()| {
        anyhow::anyhow!(
            "invalid target '{target}'. Missing task name after ':'. Use 'build' for the nearest enclosing unit or '<unit>:build' for another unit"
        )
    })? {
        return Ok((
            if path.is_empty() { "." } else { path }.to_string(),
            task.to_string(),
        ));
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

    Ok((String::new(), dep.to_string()))
}

pub fn read_workspace_config(git_root: &Path) -> AnyhowResult<Option<WorkspaceConfig>> {
    let workspace_path = git_root.join("SCRIPTS_WORKSPACE.toml");
    if !workspace_path
        .try_exists()
        .with_context(|| format!("failed to inspect {}", workspace_path.display()))?
    {
        return Ok(None);
    }

    let contents = read_to_string(&workspace_path)
        .with_context(|| format!("failed to read {}", workspace_path.display()))?;
    let config = toml::from_str(&contents).with_context(|| {
        format!(
            "invalid workspace configuration in {}",
            workspace_path.display()
        )
    })?;
    Ok(Some(config))
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
    fn parse_target_treats_path_like_names_as_current_unit_tasks() {
        assert_eq!(
            parse_target("./tools/pkg").unwrap(),
            (".".into(), "./tools/pkg".into())
        );
        assert_eq!(parse_target("..").unwrap(), (".".into(), "..".into()));
    }
}
