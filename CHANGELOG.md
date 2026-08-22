# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

- Run independent tasks concurrently with a bounded in-process DAG executor.
- Add `--jobs N`, defaulting to the host's logical CPU count.
- Continue independent branches after failures while skipping transitive dependents.
- Store atomic per-task cache entries so concurrent invocations cannot overwrite each other.
- Include workspace PATH configuration in task fingerprints.
- Reject malformed configuration, unknown fields, and dependency units outside the repository.
- Resolve plain task names from the nearest enclosing unit without filesystem-dependent parsing.
- Update watch registrations when the dependency graph changes.
- Add an authoritative agent skill under `skills/scripts-runner/`.

## [0.1.0] - 2026-04-11

Initial public release.
