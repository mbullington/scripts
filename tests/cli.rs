use std::{fs, path::Path, process::Command};

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
fn clean_reports_when_the_cache_file_is_removed() {
    let repo = init_repo();
    write_file(repo.path(), ".scripts_cache", "{}\n");

    scripts_command(&repo)
        .args(["clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
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
fn path_like_dependency_targets_require_explicit_task_names() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
deps = ["tools/pkg"]
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["print-tree", "app:build"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid dependency 'tools/pkg'"));
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
fn existing_unit_paths_require_explicit_task_names() {
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
        .args(["run", "app"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Units must include a task name"));
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
fn readiness_exec_runs_before_dependents() {
    let repo = init_repo();

    write_file(
        repo.path(),
        "svc/SCRIPTS",
        r#"
[build]
command = "printf 'svc\n'"
watch = []

[build.readiness]
exec = "true"
"#,
    );
    write_file(
        repo.path(),
        "app/SCRIPTS",
        r#"
[build]
deps = ["svc:build"]
command = "printf 'app\n'"
watch = []
"#,
    );

    scripts_command(&repo)
        .args(["run", "app:build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("svc").and(predicate::str::contains("app")));
}
