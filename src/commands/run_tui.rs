use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Once,
    },
    time::{Duration, Instant},
};

use crossterm::{
    cursor::Show,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Terminal,
};

use super::{
    run_executor::{RunOutputMode, TaskState},
    run_plan::RunPlan,
    run_reporter::{state_label, ReportEvent},
};

const LOG_LIMIT: usize = 256 * 1024;
static TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn mark_interrupted(_signal: i32) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

fn restore_terminal() {
    if TERMINAL_ACTIVE.swap(false, Ordering::SeqCst) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stderr(),
            ResetColor,
            SetAttribute(Attribute::Reset),
            LeaveAlternateScreen,
            Show
        );
    }
}

struct TerminalGuard {
    signals: Vec<(Signal, SigAction)>,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        PANIC_HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                restore_terminal();
                previous(info);
            }));
        });
        let mut guard = Self {
            signals: Vec::new(),
        };
        INTERRUPTED.store(false, Ordering::Relaxed);
        let action = SigAction::new(
            SigHandler::Handler(mark_interrupted),
            SaFlags::SA_RESTART,
            SigSet::empty(),
        );
        for signal in [Signal::SIGINT, Signal::SIGTERM] {
            // The handler only sets a lock-free atomic flag. Restore prior handlers on exit,
            // including between watch runs, rather than leaving termination signals ignored.
            let previous = unsafe { sigaction(signal, &action) }?;
            guard.signals.push((signal, previous));
        }
        // Arm cleanup before any terminal mutation, including partial setup failures.
        TERMINAL_ACTIVE.store(true, Ordering::SeqCst);
        enable_raw_mode()?;
        execute!(io::stderr(), EnterAlternateScreen)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
        for (signal, previous) in self.signals.drain(..) {
            // Restore the exact action saved before entering this single-thread-owned TUI.
            let _ = unsafe { sigaction(signal, &previous) };
        }
    }
}

#[derive(Default)]
struct TaskLog {
    bytes: VecDeque<u8>,
    truncated: bool,
}

impl TaskLog {
    fn push(&mut self, bytes: &[u8]) {
        let overflow = (self.bytes.len() + bytes.len()).saturating_sub(LOG_LIMIT);
        if overflow > 0 {
            self.truncated = true;
            self.bytes.drain(..overflow.min(self.bytes.len()));
        }
        self.bytes
            .extend(&bytes[bytes.len().saturating_sub(LOG_LIMIT)..]);
    }

    fn text(&self) -> String {
        plain_text(&self.bytes.iter().copied().collect::<Vec<_>>())
    }
}

fn plain_text(bytes: &[u8]) -> String {
    let stripped = strip_ansi_escapes::strip(bytes);
    String::from_utf8_lossy(&stripped)
        .replace("\r\n", "\n")
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
        .map(|ch| if ch == '\r' { '\n' } else { ch })
        .collect()
}

enum Timing {
    NotStarted,
    Running(Instant),
    Finished(Duration),
}

impl Timing {
    fn elapsed(&self) -> String {
        match self {
            Self::NotStarted => "-".into(),
            Self::Running(start) => format!("{:.1}s", start.elapsed().as_secs_f64()),
            Self::Finished(duration) => format!("{:.1}s", duration.as_secs_f64()),
        }
    }
}

struct TaskView {
    name: String,
    state: TaskState,
    timing: Timing,
    detail: String,
    log: TaskLog,
}

pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
    guard: Option<TerminalGuard>,
    tasks: Vec<TaskView>,
    selection: TableState,
    log_offset: usize,
    horizontal_offset: u16,
    dirty: bool,
    last_draw: Instant,
    started: Instant,
}

impl Tui {
    pub fn new(plan: &RunPlan, mode: RunOutputMode) -> io::Result<Self> {
        let guard = TerminalGuard::enter()?;
        let terminal = Terminal::new(CrosstermBackend::new(io::stderr()))?;
        let tasks = plan
            .entries
            .iter()
            .map(|entry| {
                let mut log = TaskLog::default();
                if matches!(mode, RunOutputMode::Verbose) {
                    log.push(
                        format!(
                            "cwd: {}\ncmd: {}\n",
                            entry.dir.display(),
                            entry.command.as_deref().unwrap_or("(no command)")
                        )
                        .as_bytes(),
                    );
                }
                TaskView {
                    name: plain_text(entry.name.as_bytes()),
                    state: TaskState::Pending,
                    timing: Timing::NotStarted,
                    detail: String::new(),
                    log,
                }
            })
            .collect();
        Ok(Self {
            terminal,
            guard: Some(guard),
            tasks,
            selection: TableState::default().with_selected(0),
            log_offset: 0,
            horizontal_offset: 0,
            dirty: true,
            last_draw: Instant::now(),
            started: Instant::now(),
        })
    }

    pub fn event(&mut self, event: ReportEvent<'_>) {
        match event {
            ReportEvent::State {
                index,
                state,
                detail,
            } => {
                let task = &mut self.tasks[index];
                if state == TaskState::Running {
                    task.timing = Timing::Running(Instant::now());
                } else if let Timing::Running(start) = task.timing {
                    task.timing = Timing::Finished(start.elapsed());
                }
                task.state = state;
                task.detail = detail
                    .map(|text| plain_text(text.as_bytes()))
                    .unwrap_or_default();
            }
            ReportEvent::Output { index, bytes } => self.tasks[index].log.push(bytes),
        }
        self.dirty = true;
    }

    pub fn tick(&mut self, wait: Duration) -> io::Result<bool> {
        if event::poll(wait)? {
            for _ in 0..32 {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            INTERRUPTED.store(true, Ordering::Relaxed);
                        }
                        let selected = self.selection.selected().unwrap_or(0);
                        let next = match key.code {
                            KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
                            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                                (selected + 1).min(self.tasks.len().saturating_sub(1))
                            }
                            KeyCode::PageUp => {
                                self.log_offset = self.log_offset.saturating_add(10);
                                selected
                            }
                            KeyCode::PageDown => {
                                self.log_offset = self.log_offset.saturating_sub(10);
                                selected
                            }
                            KeyCode::End => {
                                self.log_offset = 0;
                                selected
                            }
                            KeyCode::Left => {
                                self.horizontal_offset = self.horizontal_offset.saturating_sub(8);
                                selected
                            }
                            KeyCode::Right => {
                                self.horizontal_offset = self.horizontal_offset.saturating_add(8);
                                selected
                            }
                            _ => selected,
                        };
                        if next != selected {
                            self.selection.select(Some(next));
                            self.log_offset = 0;
                            self.horizontal_offset = 0;
                        }
                        self.dirty = true;
                    }
                    Event::Resize(_, _) => self.dirty = true,
                    _ => {}
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if self.dirty || self.last_draw.elapsed() >= Duration::from_millis(250) {
            self.draw()?;
            self.dirty = false;
            self.last_draw = Instant::now();
        }
        Ok(INTERRUPTED.load(Ordering::Relaxed))
    }

    fn draw(&mut self) -> io::Result<()> {
        let selected = self.selection.selected().unwrap_or(0);
        let task = self.tasks.get(selected);
        let log = task.map(|task| task.log.text()).unwrap_or_default();
        self.terminal.draw(|frame| {
            let areas = Layout::vertical([Constraint::Length(2), Constraint::Min(0), Constraint::Length(2)]).split(frame.area());
            let count = |state| self.tasks.iter().filter(|task| task.state == state).count();
            let heading = format!("scripts   {} tasks   {} running   {} cached   {} succeeded   {} failed   {:.1}s{}",
                self.tasks.len(), count(TaskState::Running), count(TaskState::Cached), count(TaskState::Succeeded),
                count(TaskState::Failed), self.started.elapsed().as_secs_f64(),
                if INTERRUPTED.load(Ordering::Relaxed) { "   stopping..." } else { "" });
            frame.render_widget(Paragraph::new(heading).style(Style::default().add_modifier(Modifier::BOLD)), areas[0]);
            let panes = Layout::default().direction(if frame.area().width < 90 { Direction::Vertical } else { Direction::Horizontal })
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)]).split(areas[1]);
            let rows = self.tasks.iter().map(|task| {
                let (label, color) = match task.state {
                    TaskState::Pending => ("pending", Color::DarkGray), TaskState::Running => ("running", Color::Cyan),
                    TaskState::Cached => ("cached", Color::Blue), TaskState::Succeeded => ("succeeded", Color::Green),
                    TaskState::Failed => ("failed", Color::Red), TaskState::Skipped => ("skipped", Color::Yellow),
                };
                Row::new(vec![Cell::from(task.name.clone()), Cell::from(label).style(Style::default().fg(color)), Cell::from(task.timing.elapsed())])
            });
            let table = Table::new(rows, [Constraint::Min(1), Constraint::Length(9), Constraint::Length(7)])
                .header(Row::new(["Task", "State", "Elapsed"]).style(Style::default().add_modifier(Modifier::BOLD)))
                .block(Block::default().borders(Borders::ALL).title(" Tasks "))
                .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)).highlight_symbol("> ");
            frame.render_stateful_widget(table, panes[0], &mut self.selection);
            if let Some(task) = task {
                let height = panes[1].height.saturating_sub(2) as usize;
                let lines: Vec<&str> = log.lines().collect();
                let max_offset = lines.len().saturating_sub(height);
                self.log_offset = self.log_offset.min(max_offset);
                let start = max_offset.saturating_sub(self.log_offset);
                let visible = if log.is_empty() {
                    match task.state { TaskState::Cached => "Cached; no output from this run.", TaskState::Pending => "Waiting for dependencies or a job slot.", _ => "No output." }.to_string()
                } else { lines.iter().skip(start).take(height).copied().collect::<Vec<_>>().join("\n") };
                let title = format!(" {} | {}{}{} ", task.name,
                    if self.log_offset == 0 { "following" } else { "scrolled" },
                    if task.log.truncated { " | older output discarded" } else { "" },
                    if task.detail.is_empty() { String::new() } else { format!(" | {}", task.detail) });
                frame.render_widget(Paragraph::new(visible).scroll((0, self.horizontal_offset))
                    .block(Block::default().borders(Borders::ALL).title(title)), panes[1]);
            }
            frame.render_widget(Paragraph::new("Up/Down j/k Tab: task   PgUp/PgDn: logs   End: follow\nLeft/Right: pan logs   Ctrl-C: stop   Exits when all tasks finish"), areas[2]);
        })?;
        Ok(())
    }

    pub fn finish(&mut self) {
        if self.guard.take().is_none() {
            return;
        }
        let mut stderr = io::stderr().lock();
        let _ = writeln!(
            stderr,
            "scripts summary ({:.1}s)",
            self.started.elapsed().as_secs_f64()
        );
        for task in &self.tasks {
            let _ = writeln!(
                stderr,
                "{:<7} {}  {} {}",
                state_label(task.state),
                task.name,
                task.timing.elapsed(),
                task.detail
            );
            if task.state == TaskState::Failed {
                if task.log.truncated {
                    let _ = writeln!(stderr, "    [older output discarded; last 256 KiB follows]");
                }
                for line in task.log.text().lines() {
                    let _ = writeln!(stderr, "    {line}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_are_bounded_and_strip_split_terminal_escape_sequences() {
        let mut log = TaskLog::default();
        log.push(&vec![b'x'; LOG_LIMIT + 20]);
        assert_eq!(log.bytes.len(), LOG_LIMIT);
        assert!(log.truncated);
        log.push(b"\x1b[31");
        log.push(b"mred\x1b[0m\r\n\x1b]0;unsafe title\x07safe\0");
        let text = log.text();
        assert!(text.ends_with("red\nsafe"));
        assert!(!text.contains('\x1b'));
        assert_eq!(log.bytes.len(), LOG_LIMIT);
    }
}
