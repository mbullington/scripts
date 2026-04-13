# scripts

A pragmatic monorepo task runner with content-aware caching, watch mode, and
simple service orchestration.

- simple TOML configuration
- dependency graphs across units and languages
- content-aware caching
- watch mode for development workflows
- no daemon, no remote service, intentionally non-hermetic

Website: https://mbullington.github.io/scripts/

Repository docs also include scdoc man page sources under `docs/man/`.

## Installation

```sh
cargo install scripts_runner
```

This installs the `scripts` binary.

> `scripts` currently targets Unix-like environments (macOS and Linux). Tasks are executed through `sh`, so Windows is not supported yet.

## Example

`SCRIPTS` files are plain TOML with one task per top-level table.

```toml
[build]
command = "cargo build --release"
watch = ["src/**", "Cargo.toml", "Cargo.lock"]
bin = ["target/release"]

[test]
deps = [":build"]
command = "cargo test"
watch = ["src/**", "tests/**"]
```

Run tasks:

```sh
scripts run :build
scripts run :test
scripts run :build --force
scripts run :dev --watch
```

## Task fields

- `deps`: optional list of dependencies
  - `"task"` or `":task"` for the current unit
  - `"path/to/unit:task"` for another unit
- `command`: optional shell command
- `watch`: optional list of files or glob patterns to hash
  - omitted: always run
  - `[]`: hash only the command text
  - non-empty list: hash command text plus watched file contents
- `bin`: optional list of paths added to `PATH` for the task and its dependents
- `restart_policy`: optional watch-mode restart behavior
  - `"always"` (default)
  - `"never"`
- `readiness`: optional post-command readiness check
  - `port`
  - `host` (default `127.0.0.1`)
  - `exec`
  - `timeout_ms` (default `30000`)

## Workspace configuration

At the git root you can add an optional `SCRIPTS_WORKSPACE.toml` file:

```toml
bin_append = ["tools/bin", "target/release"]
```

Each entry is added to `PATH` for every task. Entries can also be objects for
explicit path resolution:

```toml
bin_append = [
  { path = "tools/bin", relative_to = "git_root" },
  { path = "node_modules/.bin", relative_to = "unit" },
]
```

## Commands

### `scripts run [OPTIONS] <TARGET> [-- ARGS...]`

Run a task and its dependencies.

```sh
scripts run app:build
scripts run build
scripts run :dev --watch
scripts run dev -- echo done
scripts run --force tools/pkg:build
scripts run --quiet app:build
scripts run --verbose app:build
```

Notes:
- use `app:build` for another unit, or `build` / `:build` for the current unit
- anything after `--` is appended to the root task command and becomes part of the cache key
- `--watch` re-runs the target graph when changes are detected
- `--quiet` suppresses routine task status lines but still streams task output
- `--verbose` shows the working directory and shell command for each task
- task status lines are written to stderr so stdout stays usable for task output

### `scripts env <TARGET>`

Start a shell with `PATH` prepared for a task.

```sh
scripts env app:dev
scripts env dev
```

### `scripts print-tree <TARGET>`

Print a task's dependency graph.

```sh
scripts print-tree app:build
scripts tree app:build --flat
scripts print-tree app:test --json
```

`tree` is available as an alias for `print-tree`.

### `scripts clean [PATH]`

Remove the repository cache file.

```sh
scripts clean
scripts clean app
```

Any path inside the repository can be used; it is only used to locate the git root.

### `scripts completions <SHELL>`

Generate a shell completion script.

```sh
scripts completions bash > ~/.local/share/bash-completion/completions/scripts
scripts completions zsh > ~/.zfunc/_scripts
scripts completions fish > ~/.config/fish/completions/scripts.fish
```

Supported shells: `bash`, `elvish`, `fish`, `powershell`, `zsh`.

## Target syntax

- `<unit>:<task>` — run a specific task in another unit
- `<task>` — run a task in the current unit
- `:<task>` — also run a task in the current unit

If you provide a path-like target without a task name, `scripts` will ask for
`<unit>:<task>` explicitly.

## Manual pages

This repo includes scdoc sources for:

- `docs/man/scripts.1.scd`
- `docs/man/SCRIPTS.5.scd`
- `docs/man/SCRIPTS_WORKSPACE.toml.5.scd`

Build them from the repo root with `scripts` itself:

```sh
scripts run man
```

Clean generated manpages with:

```sh
scripts run clean-man
```

Or build files directly with scdoc:

```sh
mkdir -p target/man
scdoc < docs/man/scripts.1.scd > target/man/scripts.1
scdoc < docs/man/SCRIPTS.5.scd > target/man/SCRIPTS.5
scdoc < docs/man/SCRIPTS_WORKSPACE.toml.5.scd > target/man/SCRIPTS_WORKSPACE.toml.5
```

Preview them with `man ./target/man/scripts.1`,
`man ./target/man/SCRIPTS.5`, and
`man ./target/man/SCRIPTS_WORKSPACE.toml.5`.

## Resolution model

Units are directories containing a `SCRIPTS` file.

Dependencies resolve by searching upward from the depending unit toward the git root:

- `(unit root)/..`
- `(unit root)/../..`
- and so on until `(git root)`

The first matching path that contains a `SCRIPTS` file wins.

## Cache behavior

For each task, `scripts` hashes:

- the task command text
- the contents of any watched files

A task is cached only when its own hash matches and none of its dependencies had to rerun.

## Non-goals

- **Hermeticity.** `scripts` does not isolate builds from the host environment or require every dependency to be modeled inside `scripts`.
- **Remote execution.** This is a local orchestration tool, not a distributed build system.

## Roadmap

- better TUI/status output
- richer watch-mode behavior for long-running workflows
- structured event output for richer UIs
- optional remote cache later, if it proves worthwhile
