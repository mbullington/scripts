SCRIPTS(5) "scripts" "File Formats Manual"

# NAME

SCRIPTS - task definition file for scripts

# DESCRIPTION

*SCRIPTS* is a TOML file used by *scripts*(1) to define unit-scoped tasks,
dependencies, watched inputs, and task-specific *PATH* entries.

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

Optional array of relative paths to prepend to *PATH* for that task when used
with *scripts run* or *scripts env*.

## watch

Optional array of file or glob patterns to hash for caching.

Caching semantics are intentionally simple:

- omitted *watch*: always run
- *watch = []*: hash only the command text
- non-empty *watch*: hash the command text plus watched file contents

Patterns are evaluated relative to the unit directory.

# EXAMPLE

```
[build]
bin = ["target/debug", "node_modules/.bin"]
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

*scripts*(1), *scdoc*(5), *sh*(1), *toml*(5)
