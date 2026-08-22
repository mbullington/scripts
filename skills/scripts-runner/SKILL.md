---
name: scripts-runner
description: Use when working with the `scripts` monorepo task runner: writing SCRIPTS files, configuring SCRIPTS_WORKSPACE.toml, running parallel task graphs, debugging caching or dependency resolution, or using watch mode.
---

# scripts — monorepo task runner

A local monorepo task runner with bounded parallel DAG execution, content-aware caching, and watch mode.

- TOML configuration in `SCRIPTS` files
- dependency graphs across units and languages
- independent tasks run concurrently
- per-task atomic cache entries
- no daemon, remote execution, hermetic sandbox, or process supervision
- Unix-like systems only; commands execute through `sh -c`

## Core concepts

A **unit** is a directory containing a `SCRIPTS` file. Commands invoked below a unit use the nearest enclosing unit.

A **task** is a top-level TOML table inside a `SCRIPTS` file.

A **target** uses one of these forms:

- `<unit>:<task>` — task in another unit, such as `app:build`
- `<task>` — task in the nearest enclosing unit
- `:<task>` — also a task in the nearest enclosing unit

Parsing is lexical. A plain name always identifies a task, even when a file or directory has the same name. Use a colon to identify another unit.

## Commands

### `scripts run [OPTIONS] <TARGET> [-- ARGS...]`

Run a task and its dependencies.

- Independent ready tasks run concurrently.
- `--jobs N` sets the concurrency limit. The default is the logical CPU count.
- A failed task skips its transitive dependents; independent branches continue.
- `--force` ignores cached results and executes the graph.
- `--quiet` suppresses routine status lines but preserves task output and failures.
- `--verbose` prints each task's working directory and shell command.
- `--watch` reruns the graph when watched inputs change and updates registrations when the graph changes.
- Text after `--` is appended to the root task's shell command and included in its cache fingerprint.

Status lines go to stderr. Task stdout remains available to pipelines.

Examples:

```sh
scripts run build
scripts run app:build
scripts run --jobs 4 app:test
scripts run --force :build
scripts run --watch :test
scripts run dev -- echo done
```

### `scripts env <TARGET>`

Start `$SHELL` with `PATH` prepared from the target, its dependencies, and workspace `bin_append` entries. The working directory does not change.

### `scripts print-tree [--json] [--flat] <TARGET>`

Print a dependency graph. `tree` is an alias.

### `scripts clean [PATH]`

Remove `.scripts_cache/` from the Git root found from `PATH`, or from the current directory when omitted.

### `scripts completions <SHELL>`

Generate completions for Bash, Elvish, Fish, PowerShell, or Zsh.

## `SCRIPTS` format

Each top-level table defines a task. Unknown task keys are errors.

```toml
[build]
deps = ["tools/pkg:build", ":lint"]
command = "cargo build --release"
bin = ["target/release"]
watch = ["src/**", "Cargo.toml", "Cargo.lock"]

[test]
deps = [":build"]
command = "cargo test"
watch = ["src/**", "tests/**"]
```

Task keys:

- `deps` — optional dependency references
- `command` — optional shell command; omit it for a grouping task
- `bin` — optional unit-relative directories prepended to `PATH` for the task and its dependents
- `watch` — optional unit-relative files or glob patterns used for caching

Caching semantics:

- omitted `watch`: always run
- `watch = []`: hash declarations and command text without file contents
- non-empty `watch`: also hash matching file paths and contents

A dependent reruns whenever one of its dependencies reruns.

## Workspace configuration

An optional `SCRIPTS_WORKSPACE.toml` at the Git root defines paths appended to every task's `PATH`:

```toml
bin_append = [
  "tools/bin",
  { path = "node_modules/.bin", relative_to = "unit" },
]
```

String entries resolve from the Git root. Object entries set `relative_to` to `git_root` or `unit`. Missing directories are ignored. Malformed values and unknown fields are errors.

Workspace configuration is part of cacheable task fingerprints. Changing `bin_append` invalidates cached tasks.

## Dependency resolution

For a dependency such as `tools/pkg:build`, `scripts` checks the depending unit, then each ancestor through the Git root. The first candidate containing `SCRIPTS` wins. A resolved unit may not escape the repository.

## Cache behavior

`.scripts_cache/` contains one opaque, atomically replaced entry per task. Repository-relative unit/task identity determines the entry name. Separate entries allow concurrent `scripts` processes to update different tasks safely.

The task fingerprint includes:

- cache format version
- effective root command
- dependency, `bin`, and `watch` declarations
- workspace `bin_append` configuration
- watched paths and file contents

`.scripts_cache/` and `.git/` are excluded from watched content.

## Watch mode

Watch mode starts after the initial graph finishes. It watches units containing cacheable tasks plus workspace configuration. Each successful rerun rebuilds the graph and reconciles watched units, so newly added dependencies take effect without restarting.

Watch mode reruns completed commands. It does not supervise long-running services. Do not background a server and expect `scripts` to manage its lifecycle; use a dedicated process supervisor.

## Conventions

- Keep `watch` patterns narrow enough to make cache hits useful and complete enough to prevent stale results.
- Use grouping tasks to name workflows.
- Model generated tools as dependency tasks with `bin` outputs.
- Use `--jobs 1` when a graph intentionally serializes access not represented by dependencies.
- Treat configuration errors as errors; do not rely on unknown keys being ignored.
