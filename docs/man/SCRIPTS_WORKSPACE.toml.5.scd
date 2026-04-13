SCRIPTS_WORKSPACE.toml(5) "scripts" "File Formats Manual"

# NAME

SCRIPTS_WORKSPACE.toml - workspace-wide configuration for scripts

# DESCRIPTION

*SCRIPTS_WORKSPACE.toml* is an optional TOML file placed at the git repository
root. It defines workspace-wide settings shared by all *scripts*(1) tasks.

The current configuration surface is intentionally small: it allows directories
to be appended to *PATH* for every task through *bin_append*.

# FORMAT

A workspace file is regular UTF-8 TOML.

# KEYS

## bin_append

Optional array of path entries added to *PATH* for every task.

Each entry may be either:

- a string, resolved relative to the git repository root
- an object with *path* and *relative_to*

String example:

```
bin_append = ["tools/bin", "target/release"]
```

Object example:

```
bin_append = [
  { path = "tools/bin", relative_to = "git_root" },
  { path = "node_modules/.bin", relative_to = "unit" },
]
```

### path

Relative path to a directory that should be prepended to *PATH*.

### relative_to

How the entry should be resolved.

Supported values:

- *git_root* - resolve relative to the repository root
- *unit* - resolve relative to the active task's unit directory

If an entry resolves to a path which does not exist or is not a directory, it
is ignored.

# EXAMPLES

Use shared repo-level tools:

```
bin_append = ["tools/bin", "target/release"]
```

Use each unit's own local Node.js binaries:

```
bin_append = [
  { path = "node_modules/.bin", relative_to = "unit" },
]
```

Mix repo-wide and unit-relative entries:

```
bin_append = [
  { path = "tools/bin", relative_to = "git_root" },
  { path = "bin", relative_to = "unit" },
]
```

# NOTES

Workspace bins are appended after task-local and dependency-provided *bin*
entries, so local task configuration still wins.

# SEE ALSO

*scripts*(1), *SCRIPTS*(5), *toml*(5)
