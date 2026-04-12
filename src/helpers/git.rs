use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};

use thiserror::Error;

// Instead of using a git library, we'll just wrap the git command found on the system.
// This keeps the dependency surface small while still giving useful user-facing errors.

#[derive(Debug, Error)]
pub enum GitError {
    #[error("not inside a git repository. Run `scripts` from inside a git checkout")]
    NotInRepository,
    #[error("`git` was not found in PATH. Install git or update PATH")]
    GitNotFound,
    #[error("failed to run `git`: {0}")]
    IO(std::io::Error),
    #[error("failed to read git output: {0}")]
    Other(anyhow::Error),
}

pub fn get_git_root(working_dir: &Path) -> Result<PathBuf, GitError> {
    let output = match Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(working_dir)
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == ErrorKind::NotFound => return Err(GitError::GitNotFound),
        Err(error) => return Err(GitError::IO(error)),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a git repository") {
            return Err(GitError::NotInRepository);
        }
        return Err(GitError::IO(std::io::Error::other(
            stderr.trim().to_string(),
        )));
    }

    match String::from_utf8(output.stdout) {
        Ok(root) => Ok(PathBuf::from(root.trim())),
        Err(error) => Err(GitError::Other(anyhow::Error::new(error))),
    }
}
