scripts(1) "scripts" "Scripts Manual"

# NAME

scripts - pragmatic monorepo task runner with content-aware caching

# SYNOPSIS

```
scripts
scripts run [--force] [--quiet | --verbose] [--watch] [--jobs n] [--interactive] target [-- args...]
scripts env target
scripts print-tree [--json] [--flat] target
scripts clean [path]
scripts completions shell
```

# DESCRIPTION

*scripts* executes unit-scoped tasks described in *SCRIPTS*(5) files.

It is designed for polyglot repositories which need lightweight orchestration,
dependency graphs, caching, and development workflows without adopting a
heavier hermetic build system.

Tasks are executed through *sh*(1). The current implementation targets Unix-like
environments, including macOS and Linux.

Invoking *scripts* with no arguments prints command help and the available tasks
from the nearest enclosing unit, if one can be found.

# COMMANDS

## run

Run a task and its dependencies.

```
scripts run [--force] [--quiet | --verbose] [--watch] [--jobs n] [--interactive] target [-- args...]
```

If arguments are provided after *--*, they are appended to the root task's
command text and become part of that task's cache key.

Use *--force* to ignore cached results and execute the task graph again.

Use *--quiet* to suppress routine task status lines. Use *--verbose* to print
each task's working directory and shell command before execution.

Independent tasks run concurrently. Use *--jobs n* to limit concurrent tasks.
The default is the number of logical CPUs. A failed task skips its dependents;
independent branches continue.

Use *--watch* to run the graph once, then monitor watched units and re-run the
target graph when changes are detected. Watch mode updates its monitored units
when the dependency graph changes. It starts after the graph finishes and does
not supervise long-running processes.

Output streams by default. Use *--interactive* to open a task dashboard when
stdin, stdout, and stderr are terminals and *TERM* is supported. The flag falls
back to streaming when any stream is redirected or *TERM* is missing, empty,
*dumb*, or *unknown*.

The dashboard keeps tasks in dependency order, with states and elapsed times.
Up, Down, *j*, *k*, and Tab select tasks. Page Up and Page Down scroll logs.
End resumes following output. Left and Right pan long lines. Ctrl-C stops the
run and its task process groups and restores the terminal.

Each task log retains the last 256 KiB of stdout and stderr, with terminal
escape sequences stripped for display. Truncation is marked. *--verbose* adds
commands and working directories to the log pane. Task stdin is closed in TUI
mode; omit *--interactive* for child commands that need input.

The dashboard closes at completion and prints a plain-text summary to stderr,
including captured logs for failed or interrupted tasks. Successful task logs
are available only while the dashboard is open. In watch mode, each run opens
a new dashboard; watch notifications stream between runs.

## env

Start a shell with *PATH* prepared for a task.

The shell inherits any *bin* paths declared by the target unit and its
dependencies, plus workspace-wide paths from *SCRIPTS_WORKSPACE.toml*(5).

```
scripts env target
```

The current working directory is not changed.

## print-tree

Print a task's dependency graph.

```
scripts print-tree [--json] [--flat] target
```

Use *--json* for structured output or *--flat* for a deduplicated list.

*tree* is available as an alias for *print-tree*.

## clean

Remove the repository cache directory.

```
scripts clean [path]
```

Any path inside the repository may be used. It is only used to locate the git
repository root.

The cache is stored at *.scripts_cache* in the git repository root.

## completions

Generate a shell completion script on standard output.

```
scripts completions shell
```

Supported shells include *bash*, *elvish*, *fish*, *powershell*, and *zsh*.

# TARGETS

Use one of these forms:

- *<unit>:<task>* for a task in another unit
- *<task>* for a task in the nearest enclosing unit
- *:<task>* also for a task in the nearest enclosing unit

The unit portion names a directory containing a *SCRIPTS*(5) file. The task
portion names one top-level task table in that file.

Examples:

- *app:build*
- *test*
- *:test*

Target parsing does not inspect the filesystem. A plain name always identifies
a task; use the colon form to identify another unit.

# CACHING

When a task declares *watch*, *scripts* hashes the task command text and the
contents of watched files.

Caching behaves as follows:

- omitted *watch*: always run
- empty *watch* list: hash only the command text
- non-empty *watch* list: hash the command text plus watched file contents

A task is considered cached only when its own hash matches the stored value and
none of its dependencies had to rerun. Workspace *bin_append* configuration is
part of the hash. Cache entries are stored and replaced independently so
concurrent *scripts* processes cannot overwrite another task's entry.

# WATCH MODE

*--watch* runs the requested task graph once, then monitors the units
participating in that graph and re-runs the graph when changes are detected.
After each run, it updates registrations for units added to or removed from the
graph. Watch mode starts after the graph finishes; long-running process
supervision is left to the task command or dedicated tools.

# EXAMPLES

Run a local task from inside a unit:

```
scripts run build
```

Run a task in another unit:

```
scripts run app:build
```

Append an inline command fragment to the root task:

```
scripts run :dev -- echo done
```

Limit execution to four concurrent tasks:

```
scripts run --jobs 4 app:build
```

Force a rerun even when inputs are unchanged:

```
scripts run --force tools/pkg:build
```

Re-run the graph when watched files change:

```
scripts run :build --watch
```

Suppress routine status lines:

```
scripts run --quiet app:build
```

Print each task's working directory and shell command:

```
scripts run --verbose app:build
```

Inspect the dependency graph as JSON:

```
scripts print-tree app:test --json
```

Inspect the dependency graph with the alias:

```
scripts tree app:build --flat
```

Open a shell with dependency-provided binaries on *PATH*:

```
scripts env dev
```

Generate a Bash completion script:

```
scripts completions bash > ~/.local/share/bash-completion/completions/scripts
```

# ENVIRONMENT

*PATH*
	Inherited and extended with task *bin* entries, dependency-provided *bin*
	entries, and any workspace *bin_append* entries from
	*SCRIPTS_WORKSPACE.toml*(5).

*SHELL*
	Used by *scripts env* to choose which shell to start. If unset,
	*/bin/sh* is used.

*TERM*
	Controls terminal support for *scripts run --interactive*. Missing, empty, *dumb*,
	and *unknown* values select streaming output.

# FILES

*SCRIPTS*
	Task definitions for a unit. See *SCRIPTS*(5).

*SCRIPTS_WORKSPACE.toml*
	Optional workspace configuration. See *SCRIPTS_WORKSPACE.toml*(5).

*.scripts_cache/*
	Directory of per-task cache entries stored at the git repository root.

# EXIT STATUS

*0* on success.

*1* if command-line parsing fails, a target cannot be resolved, a task command
fails, or a task or workspace configuration file is invalid.

*130* if a TUI run is interrupted.

# NOTES

*scripts* writes task status lines such as *RUN*, *OK*, *CACHED*, and *FAIL* to
standard error so standard output remains usable for task output.

*scripts* is intentionally non-hermetic. It orchestrates and caches task
boundaries, but it does not sandbox toolchains or the host environment.

# SEE ALSO

*SCRIPTS*(5), *SCRIPTS_WORKSPACE.toml*(5), *scdoc*(1), *sh*(1), *git*(1)
