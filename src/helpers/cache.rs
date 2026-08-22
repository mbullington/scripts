use std::{
    fs::{File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::helpers::{graph::TaskGraphNode, scripts_def::WorkspaceConfig};

const CACHE_FORMAT_VERSION: &str = "scripts-cache-v3";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct TaskCache {
    root: PathBuf,
}

#[derive(Serialize)]
struct TaskFingerprint<'a> {
    version: &'static str,
    command: Option<&'a str>,
    deps: Option<&'a [String]>,
    bin: Option<&'a [String]>,
    watch: &'a [String],
    watch_hashes: Vec<String>,
    workspace: Option<&'a WorkspaceConfig>,
}

impl TaskCache {
    pub fn open(git_root: &Path) -> Result<Self> {
        let root = git_root.join(".scripts_cache");
        if root.is_file() {
            match std::fs::remove_file(&root) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(Self { root })
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let path = self.entry_path(key);
        match std::fs::read_to_string(path) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn store(&self, key: &str, value: &str) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;

        let path = self.entry_path(key);
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_path = self.root.join(format!(
            ".{}.{}.{}.tmp",
            cache_file_name(key),
            std::process::id(),
            sequence
        ));

        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            file.write_all(value.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temp_path, path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(temp_path);
        }
        result
    }

    pub fn remove(&self, key: &str) -> Result<()> {
        match std::fs::remove_file(self.entry_path(key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.root.join(cache_file_name(key))
    }
}

fn cache_file_name(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn compute_task_hash(
    node: &TaskGraphNode,
    command: Option<&str>,
    workspace_config: Option<&WorkspaceConfig>,
) -> Result<Option<String>> {
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
        workspace: workspace_config,
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
    relative_path == ".scripts_cache"
        || relative_path.starts_with(".scripts_cache/")
        || relative_path.starts_with(".git/")
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
