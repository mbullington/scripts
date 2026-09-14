use std::{collections::VecDeque, time::Duration};

use anyhow::Result;

use super::{
    run_plan::RunPlan,
    run_process::RunningCommand,
    run_reporter::{ReportEvent, Reporter, RunInterrupted},
};

#[derive(Clone, Copy, Debug)]
pub enum RunOutputMode {
    Normal,
    Quiet,
    Verbose,
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
    error: Option<anyhow::Error>,
}

impl ExecutionOutcome {
    pub fn into_result(self) -> Result<()> {
        if let Some(error) = self.error {
            return Err(error);
        }
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

struct Scheduler<'a> {
    plan: &'a RunPlan,
    reporter: Reporter,
    states: Vec<TaskState>,
    events: Vec<TaskEvent>,
    remaining_dependencies: Vec<usize>,
    dependents: Vec<Vec<usize>>,
    ready: VecDeque<usize>,
}

impl<'a> Scheduler<'a> {
    fn new(plan: &'a RunPlan, reporter: Reporter) -> Self {
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
            reporter,
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

    fn set_state(&mut self, index: usize, state: TaskState, detail: Option<&str>) {
        self.states[index] = state;
        self.reporter.event(
            self.plan,
            ReportEvent::State {
                index,
                state,
                detail,
            },
        );
    }

    fn complete_cached(&mut self, index: usize) {
        self.set_state(index, TaskState::Cached, None);
        self.release_dependents(index);
    }

    fn complete_success(&mut self, index: usize) {
        let entry = &self.plan.entries[index];
        self.events.push(TaskEvent::Succeeded {
            cache_key: entry.cache_key.clone(),
            cache_hash: entry.cache_hash.clone(),
        });
        self.set_state(index, TaskState::Succeeded, None);
        self.release_dependents(index);
    }

    fn complete_failure(&mut self, index: usize, detail: &str) {
        self.events.push(TaskEvent::Failed {
            cache_key: self.plan.entries[index].cache_key.clone(),
        });
        self.set_state(index, TaskState::Failed, Some(detail));
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
            self.ready.retain(|queued| *queued != dependent);
            self.set_state(dependent, TaskState::Skipped, Some("(dependency failed)"));
            self.skip_dependents(dependent);
        }
    }

    fn cancel_pending(&mut self) {
        self.ready.clear();
        for index in 0..self.states.len() {
            if self.states[index] == TaskState::Pending {
                self.set_state(index, TaskState::Skipped, Some("(run interrupted)"));
            }
        }
    }

    fn into_outcome(mut self, error: Option<anyhow::Error>) -> ExecutionOutcome {
        self.reporter.finish();
        ExecutionOutcome {
            events: self.events,
            states: self.states,
            error,
        }
    }
}

pub fn execute_plan(
    plan: &RunPlan,
    output_mode: RunOutputMode,
    jobs: usize,
    interactive: bool,
) -> Result<ExecutionOutcome> {
    assert!(jobs > 0, "executor requires at least one job");
    let reporter = Reporter::new(plan, output_mode, interactive)?;
    let capture = reporter.captures_output();
    let mut scheduler = Scheduler::new(plan, reporter);
    let mut active: Vec<RunningCommand> = Vec::new();
    let mut error = None;

    loop {
        match scheduler.reporter.tick(Duration::ZERO) {
            Ok(true) if error.is_none() => error = Some(RunInterrupted.into()),
            Err(io_error) if error.is_none() => error = Some(io_error.into()),
            _ => {}
        }
        if error.is_some() {
            scheduler.cancel_pending();
            for command in &mut active {
                command.cancel();
            }
        }
        while error.is_none() && active.len() < jobs {
            let Some(index) = scheduler.next_ready() else {
                break;
            };
            let entry = &plan.entries[index];
            if !entry.should_run {
                scheduler.complete_cached(index);
                continue;
            }
            scheduler.set_state(
                index,
                TaskState::Running,
                entry.command.is_none().then_some("(no command)"),
            );
            if entry.command.is_none() {
                scheduler.complete_success(index);
                continue;
            }
            match RunningCommand::spawn(entry, capture) {
                Ok(command) => active.push(command),
                Err(error) => scheduler
                    .complete_failure(index, &format!("(failed to start command: {error})")),
            }
        }

        let mut position = 0;
        while position < active.len() {
            let index = active[position].index;
            let result = active[position].poll(plan, &mut scheduler.reporter);
            let detail = match result {
                Ok(None) => {
                    position += 1;
                    continue;
                }
                Ok(_) if error.is_some() => Some("(run interrupted)".to_string()),
                Ok(Some(status)) if status.success() => None,
                Ok(Some(status)) => Some(match status.code() {
                    Some(code) => format!("(exit code: {code})"),
                    None => "(terminated by signal)".to_string(),
                }),
                Err(error) => Some(format!("(command I/O failed: {error})")),
            };
            active.swap_remove(position);
            if let Some(detail) = detail {
                scheduler.complete_failure(index, &detail);
            } else {
                scheduler.complete_success(index);
            }
        }
        if active.is_empty() && scheduler.ready.is_empty() {
            break;
        }
        // Poll child pipes without reader threads or an unbounded output queue.
        match scheduler.reporter.tick(Duration::from_millis(20)) {
            Ok(true) if error.is_none() => error = Some(RunInterrupted.into()),
            Err(io_error) if error.is_none() => error = Some(io_error.into()),
            _ => {}
        }
    }
    Ok(scheduler.into_outcome(error))
}
