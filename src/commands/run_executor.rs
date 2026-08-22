use std::{collections::VecDeque, sync::mpsc, thread};

use anyhow::Result;
use colored::*;

use super::run_plan::{PlanEntry, RunPlan};

#[derive(Clone, Copy, Debug)]
pub enum RunOutputMode {
    Normal,
    Quiet,
    Verbose,
}

impl RunOutputMode {
    fn shows_status(self, label: &str) -> bool {
        !matches!(self, Self::Quiet) || label == "FAIL"
    }

    fn is_verbose(self) -> bool {
        matches!(self, Self::Verbose)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Pending,
    Running,
    Cached,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Clone, Debug)]
pub enum TaskEvent {
    Succeeded {
        cache_key: String,
        cache_hash: Option<String>,
    },
    Failed {
        cache_key: String,
    },
}

#[derive(Debug)]
pub struct ExecutionOutcome {
    pub events: Vec<TaskEvent>,
    pub states: Vec<TaskState>,
}

impl ExecutionOutcome {
    pub fn into_result(self) -> Result<()> {
        if self
            .states
            .iter()
            .any(|state| !matches!(state, TaskState::Cached | TaskState::Succeeded))
        {
            anyhow::bail!("one or more tasks failed");
        }

        Ok(())
    }
}

struct CommandCompletion {
    index: usize,
    result: std::io::Result<std::process::ExitStatus>,
}

struct Scheduler<'a> {
    plan: &'a RunPlan,
    output_mode: RunOutputMode,
    states: Vec<TaskState>,
    events: Vec<TaskEvent>,
    remaining_dependencies: Vec<usize>,
    dependents: Vec<Vec<usize>>,
    ready: VecDeque<usize>,
}

impl<'a> Scheduler<'a> {
    fn new(plan: &'a RunPlan, output_mode: RunOutputMode) -> Self {
        let remaining_dependencies: Vec<usize> = plan
            .entries
            .iter()
            .map(|entry| entry.dependencies.len())
            .collect();
        let mut dependents = vec![Vec::new(); plan.entries.len()];
        for entry in &plan.entries {
            for dependency in &entry.dependencies {
                dependents[*dependency].push(entry.index);
            }
        }
        let ready = remaining_dependencies
            .iter()
            .enumerate()
            .filter_map(|(index, remaining)| (*remaining == 0).then_some(index))
            .collect();

        Self {
            plan,
            output_mode,
            states: vec![TaskState::Pending; plan.entries.len()],
            events: Vec::new(),
            remaining_dependencies,
            dependents,
            ready,
        }
    }

    fn next_ready(&mut self) -> Option<usize> {
        while let Some(index) = self.ready.pop_front() {
            if self.states[index] == TaskState::Pending {
                return Some(index);
            }
        }
        None
    }

    fn complete_cached(&mut self, index: usize) {
        self.states[index] = TaskState::Cached;
        print_status(
            self.output_mode,
            "CACHED",
            &self.plan.entries[index].name,
            None,
        );
        self.release_dependents(index);
    }

    fn complete_success(&mut self, index: usize) {
        let entry = &self.plan.entries[index];
        self.states[index] = TaskState::Succeeded;
        self.events.push(TaskEvent::Succeeded {
            cache_key: entry.cache_key.clone(),
            cache_hash: entry.cache_hash.clone(),
        });
        print_status(self.output_mode, "OK", &entry.name, None);
        self.release_dependents(index);
    }

    fn complete_failure(&mut self, index: usize, detail: &str) {
        let entry = &self.plan.entries[index];
        self.states[index] = TaskState::Failed;
        self.events.push(TaskEvent::Failed {
            cache_key: entry.cache_key.clone(),
        });
        print_status(self.output_mode, "FAIL", &entry.name, Some(detail));
        self.skip_dependents(index);
    }

    fn release_dependents(&mut self, index: usize) {
        for dependent in &self.dependents[index] {
            if self.states[*dependent] != TaskState::Pending {
                continue;
            }
            self.remaining_dependencies[*dependent] -= 1;
            if self.remaining_dependencies[*dependent] == 0 {
                self.ready.push_back(*dependent);
            }
        }
    }

    fn skip_dependents(&mut self, index: usize) {
        for dependent in self.dependents[index].clone() {
            if self.states[dependent] != TaskState::Pending {
                continue;
            }
            self.states[dependent] = TaskState::Skipped;
            self.ready.retain(|queued| *queued != dependent);
            print_status(
                self.output_mode,
                "SKIP",
                &self.plan.entries[dependent].name,
                Some("(dependency failed)"),
            );
            self.skip_dependents(dependent);
        }
    }

    fn into_outcome(self) -> ExecutionOutcome {
        ExecutionOutcome {
            events: self.events,
            states: self.states,
        }
    }
}

pub fn execute_plan(plan: &RunPlan, output_mode: RunOutputMode, jobs: usize) -> ExecutionOutcome {
    assert!(jobs > 0, "executor requires at least one job");

    let mut scheduler = Scheduler::new(plan, output_mode);
    let (completion_tx, completion_rx) = mpsc::channel();
    let mut active = 0;

    thread::scope(|scope| loop {
        while active < jobs {
            let Some(index) = scheduler.next_ready() else {
                break;
            };
            let entry = &plan.entries[index];

            if !entry.should_run {
                scheduler.complete_cached(index);
                continue;
            }

            let Some(command) = entry.command.clone() else {
                print_status(output_mode, "RUN", &entry.name, Some("(no command)"));
                scheduler.complete_success(index);
                continue;
            };

            print_status(output_mode, "RUN", &entry.name, None);
            print_verbose_command(output_mode, entry, &command);
            scheduler.states[index] = TaskState::Running;

            let dir = entry.dir.clone();
            let path_var = entry.path_var.clone();
            let completion_tx = completion_tx.clone();
            let spawn_result = thread::Builder::new().spawn_scoped(scope, move || {
                use std::process::Stdio;

                let result = std::process::Command::new("sh")
                    .args(["-c", &command])
                    .current_dir(dir)
                    .env("PATH", path_var)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status();
                let _ = completion_tx.send(CommandCompletion { index, result });
            });

            match spawn_result {
                Ok(_) => active += 1,
                Err(error) => {
                    scheduler.complete_failure(index, &format!("(failed to start worker: {error})"))
                }
            }
        }

        if active == 0 {
            if scheduler.ready.is_empty() {
                break;
            }
            continue;
        }

        let completion = completion_rx
            .recv()
            .expect("worker completion channel closed unexpectedly");
        active -= 1;
        match completion.result {
            Ok(status) if status.success() => scheduler.complete_success(completion.index),
            Ok(status) => {
                let detail = match status.code() {
                    Some(code) => format!("(exit code: {code})"),
                    None => "(terminated by signal)".to_string(),
                };
                scheduler.complete_failure(completion.index, &detail);
            }
            Err(error) => scheduler.complete_failure(
                completion.index,
                &format!("(failed to start command: {error})"),
            ),
        }
    });

    scheduler.into_outcome()
}

fn status_label(label: &str) -> colored::ColoredString {
    match label {
        "RUN" => label.bold().blue(),
        "OK" => label.bold().green(),
        "CACHED" => label.bold().bright_black(),
        "SKIP" => label.bold().yellow(),
        "FAIL" => label.bold().red(),
        _ => label.normal(),
    }
}

fn print_status(output_mode: RunOutputMode, label: &str, name: &str, detail: Option<&str>) {
    if !output_mode.shows_status(label) {
        return;
    }

    let label = status_label(label);
    match detail {
        Some(detail) => eprintln!("{label} {name} {detail}"),
        None => eprintln!("{label} {name}"),
    }
}

fn print_verbose_command(output_mode: RunOutputMode, entry: &PlanEntry, command: &str) {
    if !output_mode.is_verbose() {
        return;
    }

    eprintln!("    cwd: {}", entry.dir.display());
    eprintln!("    cmd:");
    for line in command.lines() {
        eprintln!("      {line}");
    }
}
