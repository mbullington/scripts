use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};
use predicates::prelude::*;
use tempfile::TempDir;

fn init_repo() -> TempDir {
    let repo = tempfile::tempdir().expect("create temp repo");
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(repo.path())
        .assert()
        .success();
    repo
}

fn write_file(root: &Path, relative_path: &str, contents: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, contents).expect("write file");
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(path).expect("stat executable").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod executable");
    }
}

fn scripts_command(repo: &TempDir) -> Command {
    let mut command = Command::cargo_bin("scripts").expect("find scripts binary");
    command.current_dir(repo.path());
    command
}

fn wait_until(description: &str, timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {description}");
}

fn line_count(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|contents| contents.lines().count())
        .unwrap_or(0)
}

#[test]
fn run_caches_tasks_and_reruns_when_watched_paths_change() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
deps = ["dep:build"]
command = "printf 'ran-app\n'"
watch = ["src/**"]
"#,
    );
    write_file(repo.path(), "app/src/input.txt", "hello\n");
    write_file(
        repo.path(),
        "dep/SCRIPTS",
        r#"
[build]
command = "printf 'ran-dep\n'"
watch = ["file.txt"]
"#,
    );
    write_file(repo.path(), "dep/file.txt", "dep\n");

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ran-dep").and(predicate::str::contains("ran-app")));

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stderr(predicate::str::contains("CACHED"));

    fs::rename(
        repo.path().join("app/src/input.txt"),
        repo.path().join("app/src/renamed.txt"),
    )
    .expect("rename watched file");

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ran-app").and(predicate::str::contains("ran-dep").not()));
}

#[test]
fn cache_hash_includes_dependency_declarations() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "dep/SCRIPTS",
        r#"
[build]
command = "printf 'dep\n'"
watch = []
"#,
    );
    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
command = "printf 'app\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "dep:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dep"));
    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("app"));

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
deps = ["dep:build"]
command = "printf 'app\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("app"));
}

#[test]
fn cache_hash_includes_bin_declarations() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "tool/bin1/helper",
        "#!/bin/sh\nprintf 'one\n'\n",
    );
    make_executable(&repo.path().join("tool/bin1/helper"));
    write_file(
        repo.path(),
        "tool/bin2/helper",
        "#!/bin/sh\nprintf 'two\n'\n",
    );
    make_executable(&repo.path().join("tool/bin2/helper"));
    write_file(
        repo.path(),
        "tool/SCRIPTS",
        r#"
[build]
bin = ["bin1"]
command = "helper"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "tool:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("one"));

    write_file(
        repo.path(),
        "tool/SCRIPTS",
        r#"
[build]
bin = ["bin2"]
command = "helper"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "tool:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("two"));
}

#[test]
fn watch_dot_ignores_the_cache_file_it_writes() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[build]
command = "printf 'run\n'"
watch = ["."]
"#,
    );
    write_file(repo.path(), "input.txt", "hello\n");

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("run"));

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("run").not())
        .stderr(predicate::str::contains("CACHED"));
}

#[test]
fn plain_task_name_targets_the_current_unit() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[build]
command = "printf 'root-build\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("root-build"));
}

#[test]
fn dependency_resolution_skips_matching_directories_without_scripts_files() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
deps = ["shared/tool:build"]
command = "printf 'app\n'"
watch = []
"#,
    );
    fs::create_dir_all(repo.path().join("app/shared/tool")).expect("create shadow directory");
    write_file(
        repo.path(),
        "shared/tool/SCRIPTS",
        r#"
[build]
command = "printf 'shared-tool\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["print-tree", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shared/tool:build"));
}

#[test]
fn dependency_cycles_fail_with_a_clear_error() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[a]
deps = [":b"]
watch = []

[b]
deps = [":a"]
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["print-tree", ":a"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("dependency cycle detected"));
}

#[test]
fn missing_task_errors_suggest_listing_available_tasks() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
watch = []
"#,
    );

    scripts_command(&repo)
        .current_dir(repo.path().join("app"))
        .args(["print-tree", ":test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Run `scripts` in that unit to list available tasks",
        ));
}

#[test]
fn clean_removes_legacy_cache_file() {
    let repo = init_repo();
    write_file(repo.path(), ".scripts_cache", "{}\n");

    scripts_command(&repo)
        .args(["clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
}

#[test]
fn clean_removes_cache_directory() {
    let repo = init_repo();
    write_file(repo.path(), "SCRIPTS", "[build]\nwatch = []\n");
    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .success();
    assert!(repo.path().join(".scripts_cache").is_dir());

    scripts_command(&repo)
        .args(["clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
    assert!(!repo.path().join(".scripts_cache").exists());
}

#[test]
fn failed_dependencies_report_skipped_dependents() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[dep]
command = "exit 7"
watch = []

[build]
deps = [":dep"]
command = "printf 'build\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("FAIL").and(predicate::str::contains("SKIP")));
}

#[test]
fn quiet_run_hides_routine_status_lines() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
command = "printf 'hello\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "--quiet", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"))
        .stderr(predicate::str::contains("RUN").not());
}

#[test]
fn verbose_run_shows_working_directory_and_command() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
command = "printf 'hello\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "--verbose", "app:build"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("RUN app:build")
                .and(predicate::str::contains("cwd:"))
                .and(predicate::str::contains("cmd:")),
        );
}

#[test]
fn completions_command_generates_shell_script() {
    let repo = init_repo();

    scripts_command(&repo)
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("scripts")
                .and(predicate::str::contains("print-tree"))
                .and(predicate::str::contains("completions")),
        );
}

#[test]
fn path_like_dependency_names_are_current_unit_tasks() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
["tools/pkg"]
command = "printf 'path-like-task\n'"
watch = []

[build]
deps = ["tools/pkg"]
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("path-like-task"));
}

#[test]
fn per_task_bin_is_added_to_path() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "tool/SCRIPTS",
        r#"
[build]
bin = ["bin"]
command = "printf 'tool\n'"
watch = []
"#,
    );
    write_file(
        repo.path(),
        "tool/bin/helper",
        "#!/bin/sh\nprintf 'helper\n'\n",
    );
    make_executable(&repo.path().join("tool/bin/helper"));

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
deps = ["tool:build"]
command = "helper"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));
}

#[test]
fn task_names_do_not_change_meaning_when_a_matching_directory_exists() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[app]
command = "printf 'task-app\n'"
watch = []
"#,
    );
    fs::create_dir(repo.path().join("app")).expect("create matching directory");

    scripts_command(&repo)
        .args(["run", "app"])
        .assert()
        .success()
        .stdout(predicate::str::contains("task-app"));
}

#[test]
fn workspace_bin_append_is_added_to_path() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "SCRIPTS_WORKSPACE.toml",
        r#"bin_append = ["tools/bin"]
"#,
    );
    write_file(
        repo.path(),
        "tools/bin/workspace-helper",
        "#!/bin/sh\nprintf 'workspace-helper\n'\n",
    );
    let helper = repo.path().join("tools/bin/workspace-helper");
    let mut perms = fs::metadata(&helper)
        .expect("stat workspace helper")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).expect("chmod workspace helper");
    }

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
command = "workspace-helper"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("workspace-helper"));
}

#[test]
fn tasks_run_from_descendants_of_their_unit() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "unit/SCRIPTS",
        r#"
[build]
command = "printf 'nested-unit\n'"
watch = []
"#,
    );
    fs::create_dir_all(repo.path().join("unit/src/nested")).expect("create nested directory");

    scripts_command(&repo)
        .current_dir(repo.path().join("unit/src/nested"))
        .args(["run", "build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nested-unit"));
}

#[test]
fn malformed_workspace_configuration_is_an_error() {
    let repo = init_repo();
    write_file(repo.path(), "SCRIPTS", "[build]\nwatch = []\n");
    write_file(
        repo.path(),
        "SCRIPTS_WORKSPACE.toml",
        "bin_append = \"not-an-array\"\n",
    );

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid workspace configuration"));
}

#[test]
fn unknown_workspace_fields_are_an_error() {
    let repo = init_repo();
    write_file(repo.path(), "SCRIPTS", "[build]\nwatch = []\n");
    write_file(
        repo.path(),
        "SCRIPTS_WORKSPACE.toml",
        "bin_apend = [\"tools/bin\"]\n",
    );

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown field `bin_apend`"));
}

#[test]
fn unknown_task_fields_are_an_error() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[build]
command = "printf should-not-run"
wath = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown field `wath`"));
}

#[test]
fn workspace_configuration_changes_invalidate_cached_tasks() {
    let repo = init_repo();
    for (directory, output) in [("one", "one"), ("two", "two")] {
        write_file(
            repo.path(),
            &format!("tools/{directory}/helper"),
            &format!("#!/bin/sh\nprintf '{output}\\n'\n"),
        );
        make_executable(&repo.path().join(format!("tools/{directory}/helper")));
    }
    write_file(
        repo.path(),
        "SCRIPTS",
        "[build]\ncommand = \"helper\"\nwatch = []\n",
    );
    write_file(
        repo.path(),
        "SCRIPTS_WORKSPACE.toml",
        "bin_append = [\"tools/one\"]\n",
    );

    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("one"));

    write_file(
        repo.path(),
        "SCRIPTS_WORKSPACE.toml",
        "bin_append = [\"tools/two\"]\n",
    );
    scripts_command(&repo)
        .args(["run", "build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("two"));
}

#[test]
fn dependency_resolution_stays_inside_the_git_repository() {
    let repo = init_repo();
    let parent = repo.path().parent().expect("repo parent");
    let outside = tempfile::tempdir_in(parent).expect("create sibling unit");
    write_file(
        outside.path(),
        "SCRIPTS",
        "[build]\ncommand = \"printf escaped\"\n",
    );
    let outside_name = outside
        .path()
        .file_name()
        .expect("outside unit name")
        .to_string_lossy();
    write_file(
        repo.path(),
        "app/SCRIPTS",
        &format!("[build]\ndeps = [\"../{outside_name}:build\"]\n"),
    );

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "resolves outside the git repository",
        ));
}

#[test]
fn jobs_two_runs_independent_tasks_concurrently() {
    let repo = init_repo();
    fs::create_dir(repo.path().join("state")).expect("create state directory");
    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[a]
command = """
touch state/a
attempt=0
while [ ! -e state/b ] && [ "$attempt" -lt 100 ]; do
  sleep 0.01
  attempt=$((attempt + 1))
done
test -e state/b
"""

[b]
command = """
touch state/b
attempt=0
while [ ! -e state/a ] && [ "$attempt" -lt 100 ]; do
  sleep 0.01
  attempt=$((attempt + 1))
done
test -e state/a
"""

[build]
deps = [":a", ":b"]
"#,
    );

    scripts_command(&repo)
        .args(["run", "--jobs", "2", "build"])
        .assert()
        .success();
}

#[test]
fn jobs_one_never_overlaps_tasks() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[a]
command = "mkdir lock; sleep 0.1; rmdir lock"

[b]
command = "mkdir lock; sleep 0.1; rmdir lock"

[build]
deps = [":a", ":b"]
"#,
    );

    scripts_command(&repo)
        .args(["run", "--jobs", "1", "build"])
        .assert()
        .success();
}

#[test]
fn zero_jobs_is_rejected() {
    let repo = init_repo();
    write_file(repo.path(), "SCRIPTS", "[build]\n");

    scripts_command(&repo)
        .args(["run", "--jobs", "0", "build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value '0'"));
}

#[test]
fn independent_branches_continue_after_a_failure() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[fail]
command = "exit 7"

[independent]
command = "printf ran > independent-ran"

[build]
deps = [":fail", ":independent"]
"#,
    );

    scripts_command(&repo)
        .args(["run", "--jobs", "1", "build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("FAIL").and(predicate::str::contains("SKIP")));
    assert!(repo.path().join("independent-ran").is_file());
}

#[test]
fn concurrent_runs_keep_each_tasks_cache_entry() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[a]
command = "sleep 0.1; printf 'a\n'"
watch = []

[b]
command = "sleep 0.2; printf 'b\n'"
watch = []
"#,
    );

    let mut a = scripts_command(&repo)
        .args(["run", "a"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start task a");
    let mut b = scripts_command(&repo)
        .args(["run", "b"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start task b");
    assert!(a.wait().expect("wait for task a").success());
    assert!(b.wait().expect("wait for task b").success());

    for task in ["a", "b"] {
        scripts_command(&repo)
            .args(["run", task])
            .assert()
            .success()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("CACHED"));
    }
}

#[test]
fn concurrent_success_cannot_restore_a_failed_tasks_cache_entry() {
    let repo = init_repo();
    write_file(
        repo.path(),
        "SCRIPTS",
        r#"
[a]
command = "test ! -e fail"
watch = []

[b]
command = "sleep 0.4"
watch = []
"#,
    );
    scripts_command(&repo).args(["run", "a"]).assert().success();
    write_file(repo.path(), "fail", "");

    let mut b = scripts_command(&repo)
        .args(["run", "--force", "b"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start task b");
    thread::sleep(Duration::from_millis(50));
    scripts_command(&repo)
        .args(["run", "--force", "a"])
        .assert()
        .failure();
    assert!(b.wait().expect("wait for task b").success());

    scripts_command(&repo)
        .args(["run", "a"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("FAIL"));
}

#[test]
fn watch_mode_adds_new_dependency_roots() {
    let repo = init_repo();
    write_file(repo.path(), "app/input", "first\n");
    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
command = "printf 'app\n' >> ../app-runs"
watch = ["input", "SCRIPTS"]
"#,
    );
    write_file(repo.path(), "dep/input", "first\n");
    write_file(repo.path(), "block-dependency", "");
    write_file(
        repo.path(),
        "dep/SCRIPTS",
        r#"
[build]
command = "printf 'attempt\n' >> ../dep-attempts; test ! -e ../block-dependency; printf 'dep\n' >> ../dep-runs"
watch = ["input"]
"#,
    );

    let mut child = scripts_command(&repo)
        .args(["run", "--watch", "app:build"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start watch mode");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_until("initial app run", Duration::from_secs(5), || {
            line_count(&repo.path().join("app-runs")) == 1
        });
        thread::sleep(Duration::from_millis(750));

        write_file(
            repo.path(),
            "app/SCRIPTS",
            r#"
[build]
deps = ["dep:build"]
command = "printf 'app\n' >> ../app-runs"
watch = ["input", "SCRIPTS"]
"#,
        );
        wait_until("failed new dependency run", Duration::from_secs(5), || {
            line_count(&repo.path().join("dep-attempts")) >= 1
        });
        thread::sleep(Duration::from_millis(750));

        fs::remove_file(repo.path().join("block-dependency")).expect("unblock dependency");
        write_file(repo.path(), "dep/input", "second\n");
        wait_until("new dependency root change", Duration::from_secs(5), || {
            line_count(&repo.path().join("dep-runs")) >= 1
                && line_count(&repo.path().join("app-runs")) >= 2
        });
    }));

    child.kill().expect("stop watch mode");
    child.wait().expect("wait for watch mode");
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}
