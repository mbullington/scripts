use std::sync::{Arc, Mutex};

use anyhow::Result;
use colored::*;
use dagrs::{log as daglog, Action, Dag, DefaultTask, EnvVar, Input, LogLevel, Output, Task};

use crate::helpers::readiness::wait_for_readiness;

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
    dag_failed: bool,
}

impl ExecutionOutcome {
    pub fn into_result(self) -> Result<()> {
        if self.dag_failed
            || self
                .states
                .iter()
                .any(|state| matches!(state, TaskState::Failed | TaskState::Skipped))
        {
            anyhow::bail!("one or more tasks failed");
        }

        Ok(())
    }
}

struct RunAction {
    entry: PlanEntry,
    output_mode: RunOutputMode,
    states: Arc<Mutex<Vec<TaskState>>>,
    events: Arc<Mutex<Vec<TaskEvent>>>,
}

pub fn execute_plan(plan: &RunPlan, output_mode: RunOutputMode) -> ExecutionOutcome {
    daglog::init_logger(LogLevel::Off, None);

    let states = Arc::new(Mutex::new(vec![TaskState::Pending; plan.entries.len()]));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut dag_tasks = Vec::with_capacity(plan.entries.len());

    for entry in &plan.entries {
        let action = RunAction {
            entry: entry.clone(),
            output_mode,
            states: states.clone(),
            events: events.clone(),
        };
        dag_tasks.push(DefaultTask::new(action, &entry.name));
    }

    for entry in &plan.entries {
        let dep_ids: Vec<usize> = entry
            .dependencies
            .iter()
            .map(|&handle| dag_tasks[handle].id())
            .collect();
        dag_tasks[entry.index].set_predecessors_by_id(&dep_ids);
    }

    let mut dag = Dag::with_tasks(dag_tasks);
    let dag_failed = dag.start().is_err();

    let mut states = states.lock().unwrap().clone();
    for entry in &plan.entries {
        if entry.should_run && states[entry.index] == TaskState::Pending {
            states[entry.index] = TaskState::Skipped;
            print_status(
                output_mode,
                "SKIP",
                &entry.name,
                Some("(dependency failed)"),
            );
        }
    }

    let events = events.lock().unwrap().clone();
    ExecutionOutcome {
        events,
        states,
        dag_failed,
    }
}

impl Action for RunAction {
    fn run(&self, _input: Input, _env: Arc<EnvVar>) -> Result<Output, dagrs::RunningError> {
        if !self.entry.should_run {
            self.set_state(TaskState::Cached);
            print_status(self.output_mode, "CACHED", &self.entry.name, None);
            return Ok(Output::empty());
        }

        let command = match &self.entry.command {
            Some(command) => command,
            None => {
                print_status(
                    self.output_mode,
                    "RUN",
                    &self.entry.name,
                    Some("(no command)"),
                );
                return self.finish_success();
            }
        };

        print_status(self.output_mode, "RUN", &self.entry.name, None);
        print_verbose_command(self.output_mode, &self.entry, command);

        use std::process::Stdio;

        let status = std::process::Command::new("sh")
            .args(["-c", command])
            .current_dir(&self.entry.dir)
            .env("PATH", &self.entry.path_var)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(dagrs::RunningError::from_err)?;

        if status.success() {
            self.finish_success()
        } else {
            self.finish_failure();
            let exit_code = status.code();
            let detail = format!("(exit code: {exit_code:?})");
            print_status(self.output_mode, "FAIL", &self.entry.name, Some(&detail));
            Err(dagrs::RunningError::new(format!(
                "task {} failed with exit code {exit_code:?}",
                self.entry.name
            )))
        }
    }
}

impl RunAction {
    fn set_state(&self, state: TaskState) {
        self.states.lock().unwrap()[self.entry.index] = state;
    }

    fn finish_success(&self) -> Result<Output, dagrs::RunningError> {
        if let Some(readiness) = &self.entry.readiness {
            if let Err(error) = wait_for_readiness(readiness, &self.entry.name) {
                self.finish_failure();
                let detail = format!("({error})");
                print_status(self.output_mode, "FAIL", &self.entry.name, Some(&detail));
                return Err(dagrs::RunningError::new(error.to_string()));
            }
        }

        self.set_state(TaskState::Succeeded);
        self.events.lock().unwrap().push(TaskEvent::Succeeded {
            cache_key: self.entry.cache_key.clone(),
            cache_hash: self.entry.cache_hash.clone(),
        });
        print_status(self.output_mode, "OK", &self.entry.name, None);
        Ok(Output::empty())
    }

    fn finish_failure(&self) {
        self.set_state(TaskState::Failed);
        self.events.lock().unwrap().push(TaskEvent::Failed {
            cache_key: self.entry.cache_key.clone(),
        });
    }
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
