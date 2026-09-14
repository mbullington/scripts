use std::{
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    sys::signal::{killpg, Signal},
    unistd::Pid,
};

use super::{
    run_plan::{PlanEntry, RunPlan},
    run_reporter::{ReportEvent, Reporter},
};

pub struct RunningCommand {
    pub index: usize,
    child: Child,
    captured: bool,
    reaped: bool,
    cancelled_at: Option<Instant>,
}

impl RunningCommand {
    pub fn spawn(entry: &PlanEntry, capture: bool) -> io::Result<Self> {
        let mut command = Command::new("sh");
        command
            .args(["-c", entry.command.as_deref().expect("command task")])
            .current_dir(&entry.dir)
            .env("PATH", &entry.path_var);
        if capture {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0);
        }
        let child = command.spawn()?;
        let running = Self {
            index: entry.index,
            child,
            captured: capture,
            reaped: false,
            cancelled_at: None,
        };
        if capture {
            set_nonblocking(running.child.stdout.as_ref().expect("piped stdout"))?;
            set_nonblocking(running.child.stderr.as_ref().expect("piped stderr"))?;
        }
        Ok(running)
    }

    pub fn cancel(&mut self) {
        if self.cancelled_at.is_none() {
            self.cancelled_at = Some(Instant::now());
            self.signal(Signal::SIGTERM);
        }
    }

    fn signal(&mut self, signal: Signal) {
        if self.captured {
            let _ = killpg(Pid::from_raw(self.child.id() as i32), signal);
        } else {
            let _ = self.child.kill();
        }
    }

    pub fn poll(
        &mut self,
        plan: &RunPlan,
        reporter: &mut Reporter,
    ) -> io::Result<Option<ExitStatus>> {
        if self
            .cancelled_at
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(500))
        {
            self.signal(Signal::SIGKILL);
        }
        self.read_output(plan, reporter, 64 * 1024)?;
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
            // Read the remaining pipe contents, without waiting for inherited writers.
            self.read_output(plan, reporter, 1024 * 1024)?;
        }
        Ok(status)
    }

    fn read_output(
        &mut self,
        plan: &RunPlan,
        reporter: &mut Reporter,
        limit: usize,
    ) -> io::Result<()> {
        if let Some(stdout) = &mut self.child.stdout {
            drain(stdout, self.index, plan, reporter, limit)?;
        }
        if let Some(stderr) = &mut self.child.stderr {
            drain(stderr, self.index, plan, reporter, limit)?;
        }
        Ok(())
    }
}

impl Drop for RunningCommand {
    fn drop(&mut self) {
        if self.captured || !self.reaped {
            self.signal(Signal::SIGKILL);
        }
        if !self.reaped {
            let _ = self.child.wait();
        }
    }
}

fn set_nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    let flags = fcntl(pipe.as_raw_fd(), FcntlArg::F_GETFL)?;
    fcntl(
        pipe.as_raw_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )?;
    Ok(())
}

fn drain(
    pipe: &mut impl Read,
    index: usize,
    plan: &RunPlan,
    reporter: &mut Reporter,
    limit: usize,
) -> io::Result<()> {
    let mut buffer = [0; 8192];
    let mut read = 0;
    while read < limit {
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(len) => {
                reporter.event(
                    plan,
                    ReportEvent::Output {
                        index,
                        bytes: &buffer[..len],
                    },
                );
                read += len;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
