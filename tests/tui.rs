use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::process::{CommandExt, ExitStatusExt},
    },
    path::Path,
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    libc,
    pty::{openpty, Winsize},
    sys::termios::{tcgetattr, Termios},
    unistd::setsid,
};

struct TerminalRun {
    child: Child,
    master: File,
    slave: File,
    original: Termios,
    output: String,
}

impl TerminalRun {
    fn start(root: &Path, args: &[&str]) -> Self {
        let pty = openpty(
            Some(&Winsize {
                ws_row: 24,
                ws_col: 100,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }),
            None,
        )
        .unwrap();
        let master = File::from(pty.master);
        let slave = File::from(pty.slave);
        let original = tcgetattr(&slave).unwrap();
        fcntl(master.as_raw_fd(), FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_scripts"));
        command
            .current_dir(root)
            .args(args)
            .env("TERM", "xterm-256color")
            .env_remove("CI")
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave.try_clone().unwrap());
        // Give the child its own controlling terminal so it cannot modify the test runner's TTY.
        // Only async-signal-safe operations run between fork and exec.
        unsafe {
            command.pre_exec(|| {
                setsid()?;
                if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self {
            child: command.spawn().unwrap(),
            master,
            slave,
            original,
            output: String::new(),
        }
    }

    fn read_output(&mut self) {
        let mut buffer = [0; 8192];
        loop {
            match self.master.read(&mut buffer) {
                Ok(0) => break,
                Ok(len) => self
                    .output
                    .push_str(&String::from_utf8_lossy(&buffer[..len])),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("read terminal: {error}"),
            }
        }
    }

    fn wait_for(&mut self, text: &str, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            self.read_output();
            if self.output.matches(text).count() >= count {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("did not see {text:?} {count} times: {}", self.output);
    }

    fn interrupt(&mut self) {
        self.master.write_all(b"\x03").unwrap();
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            self.read_output();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.read_output();
                return status;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("terminal command did not exit: {}", self.output);
    }

    fn assert_restored(&self) {
        assert_eq!(tcgetattr(&self.slave).unwrap(), self.original);
        assert!(
            self.output.contains("\x1b[?1049l"),
            "did not leave alternate screen"
        );
    }
}

impl Drop for TerminalRun {
    fn drop(&mut self) {
        let _ = self.master.write_all(b"\x03");
        for _ in 0..100 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn repo(scripts: &str) -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(repo.path())
        .status()
        .unwrap()
        .success());
    fs::write(repo.path().join("SCRIPTS"), scripts).unwrap();
    repo
}

#[test]
fn watch_restores_signals_and_terminal_between_tui_runs() {
    let repo =
        repo("[build]\ncommand = 'cat input; cp input output; sleep 0.1'\nwatch = ['input']\n");
    fs::write(repo.path().join("input"), "first build\n").unwrap();
    let mut run = TerminalRun::start(repo.path(), &["run", "--interactive", "--watch", "build"]);
    run.wait_for("watching for changes", 1);
    run.assert_restored();
    fs::write(repo.path().join("input"), "second build\n").unwrap();
    run.wait_for("watching for changes", 2);
    assert_eq!(
        fs::read_to_string(repo.path().join("output")).unwrap(),
        "second build\n"
    );
    run.assert_restored();
    run.interrupt();
    assert_eq!(run.wait().signal(), Some(libc::SIGINT));
    run.assert_restored();
}

#[test]
fn interrupted_tui_preserves_partial_output_and_skips_dependents() {
    let repo = repo("[slow]\ncommand = \"printf 'partial diagnostic'; touch started; sleep 2\"\nwatch = []\n[after]\ndeps = [':slow']\ncommand = 'touch must-not-run'\n");
    let mut run = TerminalRun::start(repo.path(), &["run", "--interactive", "after"]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !repo.path().join("started").exists() && Instant::now() < deadline {
        run.read_output();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(repo.path().join("started").exists(), "task did not start");
    run.interrupt();
    assert_eq!(run.wait().code(), Some(130));
    run.assert_restored();
    let summary = run
        .output
        .split("scripts summary")
        .nth(1)
        .expect("durable summary");
    assert!(summary.contains("FAIL    :slow"));
    assert!(summary.contains("partial diagnostic"));
    assert!(summary.contains("SKIP    :after"));
    assert!(!repo.path().join("must-not-run").exists());
    assert!(!fs::read_dir(repo.path().join(".scripts_cache"))
        .is_ok_and(|mut entries| entries.next().is_some()));
}

#[test]
fn terminal_runs_stream_unless_interactive_is_requested() {
    let repo = repo("[build]\ncommand = 'echo task-output'\n");
    let mut run = TerminalRun::start(repo.path(), &["run", "build"]);
    assert!(run.wait().success());
    assert!(run.output.contains("RUN"));
    assert!(run.output.contains("task-output"));
    assert!(run.output.contains("OK"));
    assert!(
        !run.output.contains("\x1b[?1049h"),
        "entered alternate screen without --interactive"
    );
    assert_eq!(tcgetattr(&run.slave).unwrap(), run.original);
}
