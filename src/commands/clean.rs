use std::path::Path;

use anyhow::Result;

use crate::helpers::git::get_git_root;

pub fn cmd_clean_command(target: &str) -> Result<()> {
    let git_root = get_git_root(Path::new(target))?;
    let cache_path = git_root.join(".scripts_cache");

    if cache_path.exists() {
        std::fs::remove_file(&cache_path)?;
        println!("removed {}", cache_path.display());
    } else {
        println!("cache already clean ({})", cache_path.display());
    }

    Ok(())
}
