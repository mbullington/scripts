scripts(1) "scripts" "Scripts Manual"

# NAME

scripts - pragmatic monorepo task runner with content-aware caching

# SYNOPSIS

```
scripts
scripts run [--force] [--quiet | --verbose] [target] [-- args...]
scripts env [target]
scripts print-tree [--json] [--flat] [target]
scripts clean [path]
scripts completions shell
```

# DESCRIPTION

*scripts* executes unit-scoped tasks described in *SCRIPTS*(5) files.

It is designed for polyglot repositories which need lightweight orchestration,
dependency graphs, and content-aware caching without adopting a heavyweight,
hermetic build system.

Tasks are executed through *sh*(1). The current implementation targets Unix-like
environments, including macOS and Linux.

Invoking *scripts* with no arguments prints command help and the available tasks
from the nearest enclosing unit, if one can be found.

# COMMANDS

## run

Run a task and its dependencies.

```
scripts run [--force] [--quiet | --verbose] [target] [-- args...]
```

If arguments are provided after *--*, they are appended to the root task's
command text and become part of that task's cache key.

Use *--force* to ignore cached results and execute the task graph again.

Use *--quiet* to suppress routine task status lines. Use *--verbose* to print
each task's working directory and shell command before execution.

## env

Start a shell with *PATH* prepared for a task.

The shell inherits any *bin* paths declared by the target unit and its
dependencies.

```
scripts env [target]
```

The current working directory is not changed.

## print-tree

Print a task's dependency graph.

```
scripts print-tree [--json] [--flat] [target]
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

Targets use the form <unit>:<task>.

The unit portion names a directory containing a *SCRIPTS*(5) file. The task
portion names one top-level task table in that file.

If _task_ is omitted in dependency declarations, *build* is assumed.

When the CLI target omits the colon entirely, *scripts* applies a convenience
rule:

- if the argument looks like a path or names a directory containing *SCRIPTS*,
  it is treated as a unit and the task defaults to *build*
- otherwise, it is treated as a task name in the current unit

Examples:

- *app:build*
- *:test*
- *tools/pkg* meaning *tools/pkg:build*
- *build* meaning *.:build* when run from inside a unit

# CACHING

When a task declares *watch*, *scripts* hashes the task command text and the
contents of watched files.

Caching behaves as follows:

- omitted *watch*: always run
- empty *watch* list: hash only the command text
- non-empty *watch* list: hash the command text plus watched file contents

A task is considered cached only when its own hash matches the stored value and
none of its dependencies had to rerun.

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
scripts env :dev
```

Generate a Bash completion script:

```
scripts completions bash > ~/.local/share/bash-completion/completions/scripts
```

# ENVIRONMENT

*PATH*
	Inherited and extended with any *bin* paths declared in *SCRIPTS*(5), plus
	*node_modules/.bin* when present in the active unit.

*SHELL*
	Used by *scripts env* to choose which shell to start. If unset,
	*/bin/sh* is used.

# FILES

*SCRIPTS*
	Task definitions for a unit. See *SCRIPTS*(5).

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

*SCRIPTS*(5), *scdoc*(1), *sh*(1), *git*(1)
