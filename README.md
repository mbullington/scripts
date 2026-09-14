
# scripts

A parallel monorepo task runner with content-aware caching and watch mode.

- simple TOML configuration
- bounded parallel execution of dependency graphs
- interactive task dashboard with per-task logs
- content-aware caching
- watch mode for development workflows
- no daemon, no remote service, intentionally non-hermetic

Repository docs include scdoc man page sources under `docs/man/`.

## Installation

### npm, pnpm, or Yarn

Install the project-local CLI with one of these package managers:

```sh
npm install --save-dev --save-exact @mbullington/scripts
npx scripts --version

pnpm add -D -E @mbullington/scripts
pnpm exec scripts --version

yarn add --dev --exact @mbullington/scripts
yarn exec scripts --version
```

The npm launcher requires Node.js 22.15+ or 23.11+. Prebuilt binaries support
macOS ARM64, macOS x64, Linux x64, and Linux ARM64. The Linux binaries use static
musl linking and work on both glibc and musl distributions. No Rust toolchain or
postinstall download is needed. Keep optional dependencies enabled so the package
manager can install the binary for your operating system and architecture.

Commit `package.json` and your lockfile. In CI, use `npm ci`,
`pnpm install --frozen-lockfile`, or `yarn install --immutable`, then invoke the
project-local command. Do not copy `node_modules` between platforms.

Package scripts can call `scripts` directly:

```json
{
  "scripts": {
    "build": "scripts run :build"
  }
}
```

### Cargo

```sh
cargo install scripts_runner
```

This installs the `scripts` binary. Use Cargo to build from source on Unix targets
without a prebuilt npm package.

For npm release setup, see [Publishing npm packages](docs/npm-releases.md).

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
scripts run :build --watch
scripts run --jobs 4 :test
```

## Task fields

- `deps`: optional list of dependencies. Use `<unit>:<task>` for another unit, or `<task>` / `:<task>` for the current unit.
- `command`: optional shell command. Tasks without a command can still exist to group dependencies.
- `watch`: optional list of files or glob patterns to hash.
  - omitted: always run
  - `[]`: hash only the command text
  - non-empty list: hash command text plus watched file contents
- `bin`: optional list of paths added to `PATH` for the task and its dependents

Unknown task fields are errors, so misspelled keys cannot silently change task behavior.

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

Malformed workspace configuration and unknown fields are errors.

## Commands

### `scripts run [OPTIONS] <TARGET> [-- ARGS...]`

Run a task and its dependencies.

```sh
scripts run app:build
scripts run build
scripts run :build --watch
scripts run dev -- echo done
scripts run --jobs 4 app:build
scripts run --force tools/pkg:build
scripts run --quiet app:build
scripts run --verbose app:build
```

Notes:
- use `app:build` for another unit, or `build` / `:build` for the nearest enclosing unit
- independent tasks run concurrently; `--jobs N` sets the limit, which defaults to the logical CPU count
- a failed task skips its dependents, while independent branches continue
- anything after `--` is appended to the root task command and becomes part of the cache key
- `--watch` starts after the graph finishes, then re-runs the target graph when watched inputs change
- watch mode updates its watched units when the dependency graph changes
- `--interactive` opens a task dashboard on supported terminals; output streams by default
- `--quiet` suppresses routine task status lines in streaming output
- `--verbose` shows the working directory and shell command for each task
- task status lines are written to stderr so stdout stays usable for task output

### Interactive output

Output streams by default. `--interactive` opens a dashboard when stdin,
stdout, and stderr are terminals and `TERM` is supported. It falls back to
streaming when a stream is redirected or `TERM` is missing, empty, `dumb`, or
`unknown`.

```sh
scripts run --interactive --jobs 4 app:build
scripts run app:build > build.log
```

The dashboard keeps tasks in dependency order as their states change. It shows
pending, running, cached, succeeded, failed, and skipped tasks, with elapsed
time for executed tasks. Ratatui updates changed terminal cells rather than
clearing the screen each frame.

- Up, Down, `j`, `k`, or Tab selects a task.
- Page Up and Page Down scroll its logs. End resumes following new output.
- Left and Right pan long log lines.
- Ctrl-C stops the run and its task process groups, then restores the terminal.

The dashboard captures stdout and stderr per task, retaining the last 256 KiB
and marking truncated logs. Terminal escape sequences are stripped for display.
`--verbose` includes each command and working directory in its log pane.
Task stdin is closed in TUI mode. Omit `--interactive` for child commands that need input.

The dashboard closes when the run finishes. A plain-text summary remains on
stderr, including captured output from failed or interrupted tasks. Successful
task logs are available only while the dashboard is open. Watch mode opens a
new dashboard for each run and streams watch notifications between runs.

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

Remove the repository cache directory.

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
- `<task>` — run a task in the nearest enclosing unit
- `:<task>` — also run a task in the nearest enclosing unit

Target parsing does not inspect the filesystem. A plain name is always a task;
use the colon form to name another unit.

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

- `(unit root)/<dependency path>`
- `(unit root)/../<dependency path>`
- and so on through `(git root)/<dependency path>`

The first matching path that contains a `SCRIPTS` file wins. Resolved units must
remain inside the git repository.

## Cache behavior

For each task with `watch` present, `scripts` hashes:

- a cache format version
- the task command text
- dependency, `bin`, and `watch` declarations
- workspace `bin_append` configuration
- the contents of any watched files

The repository `.scripts_cache/` directory stores one atomic entry per task and
is ignored when hashing watched files. Separate entries let concurrent
invocations update different tasks without overwriting each other.

A task is cached only when its own hash matches and none of its dependencies had to rerun.

## Agent skill

The repository owns an agent reference at
[`skills/scripts-runner/SKILL.md`](skills/scripts-runner/SKILL.md). Keep it in
sync with CLI and configuration changes.

## Non-goals

- **Hermeticity.** `scripts` does not isolate builds from the host environment or require every dependency to be modeled inside `scripts`.
- **Remote execution.** This is a local orchestration tool, not a distributed build system.
- **Process supervision.** `scripts run --watch` reruns completed task graphs; it does not manage long-running service lifecycles.
