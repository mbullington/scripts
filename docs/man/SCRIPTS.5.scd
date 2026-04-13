SCRIPTS(5) "scripts" "File Formats Manual"

# NAME

SCRIPTS - task definition file for scripts

# DESCRIPTION

*SCRIPTS* is a TOML file used by *scripts*(1) to define unit-scoped tasks,
dependencies, watched inputs, PATH entries, watch-mode restart behavior, and
optional readiness checks.

A unit is any directory containing a *SCRIPTS* file.

# FORMAT

A *SCRIPTS* file is UTF-8 TOML.

Each top-level table defines one task directly. For example:

```
[build]
```

# TASK KEYS

## deps

Optional array of dependency references.

Each dependency uses one of these forms:

- *<unit>:<task>* for another unit
- *<task>* for the current unit
- *:<task>* also for the current unit

Examples:

- *tools/pkg:build*
- *lint*
- *:lint*

Dependency units are resolved by searching upward from the depending unit
toward the git repository root. The first matching directory which contains a
*SCRIPTS* file is selected.

## command

Optional shell command string.

The command is executed with *sh -c* from the owning unit directory.

A task without *command* may still be useful as a grouping node when it only
depends on other tasks.

## bin

Optional array of relative paths to prepend to *PATH* for that task.

These paths are made available when the task runs, when a dependent task runs,
and when opening a shell with *scripts env*.

## watch

Optional array of file or glob patterns to hash for caching.

Caching semantics are intentionally simple:

- omitted *watch*: always run
- *watch = []*: hash only the command text
- non-empty *watch*: hash the command text plus watched file contents

Patterns are evaluated relative to the unit directory.

## restart_policy

Optional watch-mode restart behavior.

Supported values:

- *"always"* - restart on watch-triggered reruns
- *"never"* - skip this task on watch-triggered reruns

If omitted, the default is *"always"*.

## readiness

Optional post-command readiness check.

This is useful for commands that launch a background service and then exit.
When present, the task is not considered successful until the readiness probe
passes.

Supported subkeys:

- *port* - TCP port to probe
- *host* - host name for *port* probes, default *127.0.0.1*
- *exec* - shell command that exits *0* when ready
- *timeout_ms* - timeout in milliseconds, default *30000*

Exactly one of *port* or *exec* should be used.

# EXAMPLE

```
[build]
bin = ["target/debug"]
deps = ["tools/pkg:build", ":lint"]
command = """
pnpm -C app build
"""
watch = ["app/**", "Cargo.lock"]

[lint]
command = """
pnpm -C app lint
"""
watch = ["app/**", "package.json", "pnpm-lock.yaml"]

[dev]
command = "./scripts/dev-server &"
restart_policy = "never"
watch = ["src/**"]

[dev.readiness]
exec = "curl -fsS http://127.0.0.1:3000/health"
timeout_ms = 5000

[release]
deps = [":build", "packages/api:test"]
watch = []
```

# RESOLUTION

Given a dependency path declared from some unit directory, *scripts*(1)
checks:

- unit/path
- unit/../path
- unit/../../path
- and so on until the git repository root

The first candidate directory containing *SCRIPTS* is treated as the dependency
unit.

# CACHING

The cache key for a task is derived from:

- the canonical unit path
- the task name
- the command text
- watched file contents, when *watch* is present

If a watched file is renamed, added, removed, or modified, the task hash
changes.

A task is rerun when:

- its current hash does not match the stored cache entry
- *watch* is omitted
- *--force* is passed to *scripts run*
- any dependency reruns

# CONVENTIONS

Good *SCRIPTS* files keep *watch* narrow and explicit. Under-declaring watched
inputs can cause false cache hits, while over-declaring them reduces cache
usefulness.

Because commands are run through the host shell and environment, *scripts*(1)
is not a hermetic build system.

# SEE ALSO

*scripts*(1), *SCRIPTS_WORKSPACE.toml*(5), *scdoc*(5), *sh*(1), *toml*(5)
