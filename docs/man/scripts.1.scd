scripts(1) "scripts" "Scripts Manual"

# NAME

scripts - pragmatic monorepo task runner with content-aware caching

# SYNOPSIS

```
scripts
scripts run [--force] [--quiet | --verbose] [--watch] target [-- args...]
scripts env target
scripts wait [--port port | --exec command]
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
scripts run [--force] [--quiet | --verbose] [--watch] target [-- args...]
```

If arguments are provided after *--*, they are appended to the root task's
command text and become part of that task's cache key.

Use *--force* to ignore cached results and execute the task graph again.

Use *--quiet* to suppress routine task status lines. Use *--verbose* to print
each task's working directory and shell command before execution.

Use *--watch* to run the graph once, then monitor watched units and re-run the
target graph when changes are detected. Watch mode starts after the graph
finishes; it does not supervise long-running processes.

## env

Start a shell with *PATH* prepared for a task.

The shell inherits any *bin* paths declared by the target unit and its
dependencies, plus workspace-wide paths from *SCRIPTS_WORKSPACE.toml*(5).

```
scripts env target
```

The current working directory is not changed.

## wait

Wait for a TCP port or shell command to become ready.

```
scripts wait [--port port | --exec command] [--host host] [--timeout-ms ms]
```

Use *--port* to probe a TCP port. Use *--host* to choose the host for port
probes; the default is *127.0.0.1*.

Use *--exec* to run a shell command until it exits successfully.

Use *--timeout-ms* to set the maximum wait time in milliseconds. The default is
*30000*.

This is a composable helper intended for use inside task commands. *scripts*
does not attach readiness or process lifecycle policy to the *SCRIPTS*(5)
schema.

## print-tree

Print a task's dependency graph.

```
scripts print-tree [--json] [--flat] target
```

Use *--json* for structured output or *--flat* for a deduplicated list.

*tree* is available as an alias for *print-tree*.

## clean

Remove the repository cache file.

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
- *<task>* for a task in the current unit
- *:<task>* also for a task in the current unit

The unit portion names a directory containing a *SCRIPTS*(5) file. The task
portion names one top-level task table in that file.

Examples:

- *app:build*
- *test*
- *:test*

If a path-like target is provided without *:<task>*, *scripts* reports an error
and asks for the explicit *<unit>:<task>* form.

# CACHING

When a task declares *watch*, *scripts* hashes the task command text and the
contents of watched files.

Caching behaves as follows:

- omitted *watch*: always run
- empty *watch* list: hash only the command text
- non-empty *watch* list: hash the command text plus watched file contents

A task is considered cached only when its own hash matches the stored value and
none of its dependencies had to rerun.

# WATCH MODE

*--watch* runs the requested task graph once, then monitors the units
participating in that graph and re-runs the graph when changes are detected.
Watch mode starts after the graph finishes; long-running process supervision is
left to the task command or dedicated tools.

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

Force a rerun even when inputs are unchanged:

```
scripts run --force tools/pkg:build
```

Re-run the graph when watched files change:

```
scripts run :build --watch
```

Wait for a local service in a task command or shell script:

```
scripts wait --port 3000
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

# FILES

*SCRIPTS*
	Task definitions for a unit. See *SCRIPTS*(5).

*SCRIPTS_WORKSPACE.toml*
	Optional workspace configuration. See *SCRIPTS_WORKSPACE.toml*(5).

*.scripts_cache*
	JSON cache file stored at the git repository root.

# EXIT STATUS

*0* on success.

*1* if command-line parsing fails, a target cannot be resolved, a task command
fails, or a *SCRIPTS*(5) file is invalid.

# NOTES

*scripts* writes task status lines such as *RUN*, *OK*, *CACHED*, and *FAIL* to
standard error so standard output remains usable for task output.

*scripts* is intentionally non-hermetic. It orchestrates and caches task
boundaries, but it does not sandbox toolchains or the host environment.

# SEE ALSO

*SCRIPTS*(5), *SCRIPTS_WORKSPACE.toml*(5), *scdoc*(1), *sh*(1), *git*(1)
