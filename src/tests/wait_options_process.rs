#![cfg(target_os = "macos")]

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_home() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "agent-desktop-wait-options-{}-{nonce}",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    let home = isolated_home();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-desktop"))
        .args(args)
        .env("AGENT_DESKTOP_HOME", &home)
        .env_remove("AGENT_DESKTOP_SESSION")
        .output()
        .expect("agent-desktop starts");
    let _ = std::fs::remove_dir_all(home);
    output
}

fn error_envelope(output: Output) -> Value {
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not one JSON envelope: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(output.status.code(), Some(1), "{envelope}");
    assert_eq!(envelope["ok"], false, "{envelope}");
    assert_eq!(envelope["command"], "wait", "{envelope}");
    assert_eq!(envelope["error"]["code"], "INVALID_ARGS", "{envelope}");
    assert!(
        envelope["error"]["suggestion"]
            .as_str()
            .is_some_and(|suggestion| !suggestion.is_empty()),
        "{envelope}"
    );
    envelope
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires Accessibility permissions"]
fn wait_orphan_options_return_invalid_args() {
    error_envelope(run(&["wait", "0", "--action", "type"]));
    error_envelope(run(&["wait", "0", "--window-id", "w123"]));

    let output = run(&["wait", "0"]);
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("wait returns JSON");
    assert!(output.status.success(), "{envelope}");
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(envelope["command"], "wait", "{envelope}");
    assert_eq!(envelope["data"]["waited_ms"], 0, "{envelope}");
}
