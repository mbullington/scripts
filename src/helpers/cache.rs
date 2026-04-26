use std::{collections::HashMap, fs::File, io::Read, path::Path};

use anyhow::Result;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::helpers::graph::TaskGraphNode;

const CACHE_FORMAT_VERSION: &str = "scripts-cache-v2";

#[derive(Serialize)]
struct TaskFingerprint<'a> {
    version: &'static str,
    command: Option<&'a str>,
    deps: Option<&'a [String]>,
    bin: Option<&'a [String]>,
    watch: &'a [String],
    watch_hashes: Vec<String>,
}

pub fn load_cache(path: &Path) -> Result<HashMap<String, String>> {
    if path.exists() {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    } else {
        Ok(HashMap::new())
    }
}

pub fn save_cache(path: &Path, cache: &HashMap<String, String>) -> Result<()> {
    std::fs::write(path, serde_json::to_string(cache)?)?;
    Ok(())
}

pub fn compute_task_hash(node: &TaskGraphNode, command: Option<&str>) -> Result<Option<String>> {
    let Some(patterns) = &node.task.watch else {
        return Ok(None);
    };

    let watch_hashes = patterns
        .iter()
        .map(|pattern| compute_watch_hash(&node.unit_path, pattern))
        .collect::<Result<Vec<_>>>()?;
    let fingerprint = TaskFingerprint {
        version: CACHE_FORMAT_VERSION,
        command,
        deps: node.task.deps.as_deref(),
        bin: node.task.bin.as_deref(),
        watch: patterns,
        watch_hashes,
    };

    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&fingerprint)?);
    Ok(Some(hex::encode(hasher.finalize())))
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

fn should_skip_watch_entry(relative_path: &str) -> bool {
    relative_path == ".scripts_cache" || relative_path.starts_with(".git/")
}

fn compute_watch_hash(root: &Path, pattern: &str) -> Result<String> {
    let mut builder = WalkBuilder::new(root);
    if pattern == "." {
        builder.hidden(false);
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
        builder.overrides(overrides.build()?);
    }

    let walker = builder
        .filter_entry(|entry| {
            let file_name = entry.file_name().to_string_lossy();
            file_name != ".git" && file_name != ".scripts_cache"
        })
        .build();
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
        if should_skip_watch_entry(&relative_path) {
            continue;
        }

        let digest = hash_file(entry.path())?;
        entries.push((relative_path, digest));
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (path, digest) in entries {
        hasher.update(path.as_bytes());
        hasher.update([0_u8]);
        hasher.update(digest);
        hasher.update([0_u8]);
    }

    Ok(hex::encode(hasher.finalize()))
}
