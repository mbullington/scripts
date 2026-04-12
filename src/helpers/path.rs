use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::Result;

pub fn build_path_var(unit_path: &Path, bins: &[PathBuf]) -> Result<OsString> {
    let mut paths = Vec::new();

    let node_modules_bin = unit_path.join("node_modules").join(".bin");
    if node_modules_bin.is_dir() {
        paths.push(node_modules_bin);
    }

    for path in bins.iter().rev() {
        paths.push(path.clone());
    }

    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }

    Ok(std::env::join_paths(paths)?)
}
