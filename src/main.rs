#[cfg(not(unix))]
compile_error!("scripts currently supports Unix-like environments (macOS and Linux) only.");

use std::io;

use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};
use commands::{cmd_clean_command, cmd_env_command, cmd_print_tree_command, cmd_run_command};

use crate::helpers::task_list::print_tasks_for_current_unit;

mod commands;
mod helpers;

const ROOT_AFTER_HELP: &str = "Examples:\n  scripts run app:build\n  scripts run build\n  scripts run :dev --watch\n  scripts print-tree app:test --json\n  scripts env dev\n  scripts completions bash > ~/.local/share/bash-completion/completions/scripts\n\nTarget syntax:\n  <unit>:<task>   Run a specific task in another unit\n  <task>          Run a task in the current unit\n  :<task>         Also run a task in the current unit";

const RUN_AFTER_HELP: &str = "Examples:\n  scripts run app:build\n  scripts run build\n  scripts run :dev --watch\n  scripts run dev -- echo done\n  scripts run --force tools/pkg:build\n  scripts run --quiet app:build\n  scripts run --verbose app:build";

const ENV_AFTER_HELP: &str = "Examples:\n  scripts env app:dev\n  scripts env dev";

const PRINT_TREE_AFTER_HELP: &str = "Examples:\n  scripts print-tree app:build\n  scripts tree app:build --flat\n  scripts print-tree app:test --json";

const CLEAN_AFTER_HELP: &str = "Examples:\n  scripts clean\n  scripts clean app";

const COMPLETIONS_AFTER_HELP: &str = "Examples:\n  scripts completions bash > ~/.local/share/bash-completion/completions/scripts\n  scripts completions zsh > ~/.zfunc/_scripts\n  scripts completions fish > ~/.config/fish/completions/scripts.fish";

#[derive(Debug, Parser)]
#[command(name = "scripts")]
#[command(version)]
#[command(about = "A pragmatic monorepo task runner with content-aware caching.")]
#[command(after_help = ROOT_AFTER_HELP)]
enum Cli {
    /// Run a task and its dependencies.
    Run(RunArgs),
    /// Remove the repository cache file.
    Clean(CleanArgs),
    /// Start a shell with PATH prepared for a task.
    Env(EnvArgs),
    /// Print a task's dependency graph.
    #[command(visible_alias = "tree")]
    PrintTree(PrintTreeArgs),
    /// Generate shell completion scripts.
    Completions(CompletionsArgs),
}

#[derive(Debug, clap::Args)]
#[command(about = "Run a task and its dependencies.")]
#[command(after_help = RUN_AFTER_HELP)]
struct RunArgs {
    #[arg(value_name = "TARGET")]
    /// Task target. Use <unit>:<task> for another unit or <task> for the current unit.
    target: String,
    /// Ignore cached results and run even if inputs are unchanged.
    #[arg(long)]
    force: bool,
    /// Suppress routine task status lines. Task output still passes through.
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,
    /// Show task status lines plus the working directory and shell command.
    #[arg(short, long, conflicts_with = "quiet")]
    verbose: bool,
    /// Watch for changes and re-run the target graph.
    #[arg(long)]
    watch: bool,
    /// Append an inline shell fragment to the root task after `--`.
    #[arg(trailing_var_arg = true, value_name = "ARGS")]
    args: Vec<String>,
}

#[derive(Debug, clap::Args)]
#[command(about = "Remove the repository cache file.")]
#[command(after_help = CLEAN_AFTER_HELP)]
struct CleanArgs {
    #[arg(default_value = ".", value_name = "PATH")]
    /// Any path inside the repository. Used only to locate the git root.
    target: String,
}

#[derive(Debug, clap::Args)]
#[command(about = "Start a shell with PATH prepared for a task.")]
#[command(after_help = ENV_AFTER_HELP)]
struct EnvArgs {
    #[arg(value_name = "TARGET")]
    /// Task target. Use <unit>:<task> for another unit or <task> for the current unit.
    target: String,
}

#[derive(Debug, clap::Args)]
#[command(about = "Print a task's dependency graph.")]
#[command(after_help = PRINT_TREE_AFTER_HELP)]
struct PrintTreeArgs {
    #[arg(value_name = "TARGET")]
    /// Task target. Use <unit>:<task> for another unit or <task> for the current unit.
    target: String,
    /// Emit JSON instead of human-readable text.
    #[arg(long)]
    json: bool,
    /// Print a deduplicated list instead of a tree.
    #[arg(long)]
    flat: bool,
}

#[derive(Debug, clap::Args)]
#[command(about = "Generate shell completion scripts.")]
#[command(after_help = COMPLETIONS_AFTER_HELP)]
struct CompletionsArgs {
    #[arg(value_name = "SHELL")]
    /// Shell name. Supported: bash, elvish, fish, powershell, zsh.
    shell: Shell,
}

fn main() {
    // If invoked without subcommands/args, print help and any local unit tasks.
    if std::env::args_os().len() <= 1 {
        let _ = Cli::command().print_help();
        println!();
        print_tasks_for_current_unit();
        return;
    }

    let cli = Cli::parse();
    let result = match cli {
        Cli::Run(args) => {
            let appended = if args.args.is_empty() {
                None
            } else {
                Some(args.args.join(" "))
            };
            cmd_run_command(
                &args.target,
                args.force,
                args.quiet,
                args.verbose,
                args.watch,
                appended,
            )
        }
        Cli::Clean(args) => cmd_clean_command(&args.target),
        Cli::Env(args) => cmd_env_command(&args.target),
        Cli::PrintTree(args) => cmd_print_tree_command(&args.target, args.json, args.flat),
        Cli::Completions(args) => {
            let mut command = Cli::command();
            generate(args.shell, &mut command, "scripts", &mut io::stdout());
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
