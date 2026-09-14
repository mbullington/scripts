use std::{
    io::{self, IsTerminal},
    time::Duration,
};

use anyhow::Result;
use colored::*;

use super::{
    run_executor::{RunOutputMode, TaskState},
    run_plan::{PlanEntry, RunPlan},
    run_tui::Tui,
};

#[derive(Debug, thiserror::Error)]
#[error("run interrupted")]
pub struct RunInterrupted;

pub enum ReportEvent<'a> {
    State {
        index: usize,
        state: TaskState,
        detail: Option<&'a str>,
    },
    Output {
        index: usize,
        bytes: &'a [u8],
    },
}

pub enum Reporter {
    Stream(RunOutputMode),
    Tui(Box<Tui>),
}

impl Reporter {
    pub fn new(plan: &RunPlan, mode: RunOutputMode, interactive: bool) -> Result<Self> {
        let terminals =
            io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal();
        let supported = std::env::var("TERM")
            .is_ok_and(|term| !term.is_empty() && term != "dumb" && term != "unknown");
        if interactive && terminals && supported {
            return Ok(Self::Tui(Box::new(Tui::new(plan, mode)?)));
        }
        if !io::stderr().is_terminal() || !supported {
            colored::control::set_override(false);
        }
        Ok(Self::Stream(mode))
    }

    pub fn captures_output(&self) -> bool {
        matches!(self, Self::Tui(_))
    }

    pub fn event(&mut self, plan: &RunPlan, event: ReportEvent<'_>) {
        match self {
            Self::Tui(tui) => tui.event(event),
            Self::Stream(mode) => {
                if let ReportEvent::State {
                    index,
                    state,
                    detail,
                } = event
                {
                    let entry = &plan.entries[index];
                    print_status(*mode, state, &entry.name, detail);
                    if state == TaskState::Running {
                        print_verbose_command(*mode, entry);
                    }
                }
            }
        }
    }

    pub fn tick(&mut self, wait: Duration) -> io::Result<bool> {
        match self {
            Self::Tui(tui) => tui.tick(wait),
            Self::Stream(_) => {
                std::thread::sleep(wait);
                Ok(false)
            }
        }
    }

    pub fn finish(&mut self) {
        if let Self::Tui(tui) = self {
            tui.finish();
        }
    }
}

pub fn state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Pending => "PENDING",
        TaskState::Running => "RUN",
        TaskState::Cached => "CACHED",
        TaskState::Succeeded => "OK",
        TaskState::Failed => "FAIL",
        TaskState::Skipped => "SKIP",
    }
}

fn print_status(mode: RunOutputMode, state: TaskState, name: &str, detail: Option<&str>) {
    if matches!(mode, RunOutputMode::Quiet) && state != TaskState::Failed {
        return;
    }
    let label = state_label(state);
    let label = match state {
        TaskState::Running => label.bold().blue(),
        TaskState::Succeeded => label.bold().green(),
        TaskState::Cached => label.bold().bright_black(),
        TaskState::Skipped => label.bold().yellow(),
        TaskState::Failed => label.bold().red(),
        TaskState::Pending => label.normal(),
    };
    match detail {
        Some(detail) => eprintln!("{label} {name} {detail}"),
        None => eprintln!("{label} {name}"),
    }
}

fn print_verbose_command(mode: RunOutputMode, entry: &PlanEntry) {
    if !matches!(mode, RunOutputMode::Verbose) {
        return;
    }
    eprintln!("    cwd: {}", entry.dir.display());
    if let Some(command) = &entry.command {
        eprintln!("    cmd:");
        for line in command.lines() {
            eprintln!("      {line}");
        }
    }
}
