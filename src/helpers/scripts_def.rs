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
