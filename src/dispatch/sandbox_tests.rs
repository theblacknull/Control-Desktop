use agent_desktop_core::{AppError, ErrorCode};
use clap::Parser;

use super::reject_outer_options;

#[test]
fn outer_global_options_are_rejected_before_sandbox_exec() {
    let plain =
        crate::Cli::try_parse_from(["agent-desktop", "sandbox", "exec", "dev", "--", "version"])
            .unwrap();
    assert!(reject_outer_options(&plain).is_ok());

    let outer = crate::Cli::try_parse_from([
        "agent-desktop",
        "--session",
        "abc",
        "sandbox",
        "exec",
        "dev",
        "--",
        "version",
    ])
    .unwrap();
    let AppError::Adapter(error) = reject_outer_options(&outer).unwrap_err() else {
        panic!("outer options must be an adapter error");
    };

    assert_eq!(error.code, ErrorCode::InvalidArgs);
    assert!(
        error.suggestion.is_some(),
        "the refusal must say where the options go"
    );
}
