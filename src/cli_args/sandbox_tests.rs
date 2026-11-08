use clap::Parser;

use super::SandboxActionCli;
use crate::cli::{Cli, Commands};

fn exec_action(argv: &[&str]) -> super::SandboxExecArgs {
    let mut full = vec!["agent-desktop", "sandbox"];
    full.extend_from_slice(argv);
    let cli = Cli::try_parse_from(full).expect("the sandbox command parses");
    let Commands::Sandbox(args) = cli.command.expect("the sandbox command is present") else {
        panic!("expected the sandbox command");
    };
    let SandboxActionCli::Exec(exec) = args.action else {
        panic!("expected the exec action");
    };
    exec
}

#[test]
fn sandbox_exec_hands_every_argument_after_the_separator_to_the_guest() {
    let exec = exec_action(&[
        "exec",
        "dev",
        "--",
        "snapshot",
        "--app",
        "Finder",
        "--verbose",
    ]);

    assert_eq!(exec.name, "dev");
    assert_eq!(exec.args, ["snapshot", "--app", "Finder", "--verbose"]);
}
