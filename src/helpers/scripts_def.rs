use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScriptsDef {
    /// Tasks declared directly at the top level of the SCRIPTS file.
    #[serde(flatten)]
    pub scripts: HashMap<String, Task>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Task {
    pub deps: Option<Vec<String>>,
    pub command: Option<String>,
    /// Paths that should be added to PATH when running this task or entering an
    /// env shell for it.
    pub bin: Option<Vec<String>>,
    /// Paths or glob patterns to watch for changes. If not specified, the task
    /// will always run. An empty list means only the command hash is used for
    /// caching.
    pub watch: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelativeTo {
    Unit,
    GitRoot,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BinAppendObject {
    pub path: String,
    pub relative_to: RelativeTo,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum BinAppendEntry {
    String(String),
    Object(BinAppendObject),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkspaceConfig {
    /// Paths that should be appended to PATH for all tasks.
    pub bin_append: Option<Vec<BinAppendEntry>>,
}
