# Scripts

A pragmatic monorepo task runner with content-aware caching.

- simple TOML configuration
- dependency graphs across units and languages
- content-aware caching
- no daemon, no server, no remote service

Website: https://mbullington.github.io/scripts/

Repository docs also include scdoc man page sources for *scripts*(1) and
*SCRIPTS*(5) under `docs/man/`.

## Installation

```sh
cargo install scripts_runner
```

This installs the `scripts` binary.

> `scripts` currently targets Unix-like environments (macOS and Linux). Tasks are executed through `sh`, so Windows is not supported yet.

## Goals

- Track dependencies across units and build systems like Cargo, CMake, Make, and GN.
- Speed up reruns with content-aware caching.
- Stay lightweight and easy to adopt in brownfield repos.

## Non-goals

- **Hermeticity.** `scripts` does not isolate builds from the host environment or require every dependency to be modeled inside `scripts`.
- **Remote execution.** This is a local orchestration tool, not a full distributed build system.

## Example

`SCRIPTS` files are TOML.

```toml
[build]
bin = ["target/debug"]
deps = ["tools/pkg:build"]
command = """
pnpm -C app build
"""
watch = ["app/**", "Cargo.lock"]

[test]
deps = [":build"]
command = """
cargo test
"""
watch = ["src/**", "tests/**", "Cargo.lock"]
```

### Task fields

- `deps`: optional list of `<unit>:<task>` dependencies. Omitting `:<task>` defaults to `build`.
- `command`: optional shell command. Tasks without a command can still exist to group dependencies.
- `watch`: optional list of files or glob patterns to hash.
  - omitted: always run
  - `[]`: hash only the command text
  - non-empty list: hash command text plus watched file contents
- `bin`: optional list of paths added to `PATH` for that task when using `scripts run` or `scripts env`

## Target syntax

`scripts` accepts task targets in these forms:

- `<unit>:<task>` — run a specific task in another unit
- `<task>` — run a task in the current unit
- `:<task>` — also run a task in the current unit

Examples:

- `app:build`
- `test`
- `:test`

## Commands

### `scripts run [OPTIONS] <TARGET> [-- ARGS...]`

Run a task and its dependencies.

```sh
scripts run app:build
scripts run build
scripts run dev -- echo done
scripts run --force tools/pkg:build
scripts run --quiet app:build
scripts run --verbose app:build
```

Notes:
- use `app:build` for another unit, or `build` / `:build` for the current unit
- anything after `--` is appended to the root task command and becomes part of the cache key
- `--quiet` suppresses routine task status lines but still streams task output
- `--verbose` shows the working directory and shell command for each task
- task status lines are written to stderr so stdout stays usable for task output

### `scripts env <TARGET>`

Start a shell with PATH prepared for a task.

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

## Manual pages

This repo includes scdoc sources for:

- `docs/man/scripts.1.scd`
- `docs/man/SCRIPTS.5.scd`

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
```

Preview them with `man ./target/man/scripts.1` and
`man ./target/man/SCRIPTS.5`.

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

## Distribution note

If you plan to publish publicly:

- publish the Rust crate on crates.io
- publish installable binaries on GitHub Releases
- host docs/marketing on GitHub Pages

GitHub Packages does not provide a first-class Cargo registry, so crates.io is the right distribution channel for the crate itself.

## Roadmap

- better TUI/status output
- optional local workers and scheduling controls
- structured event output for richer UIs
- optional remote cache later, if it proves worthwhile
