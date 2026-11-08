use super::execute;
use crate::commands::sandbox_action::SandboxAction;
use crate::refs::refs_test_support::HomeGuard;
use crate::sandbox::provider_test_support::ScriptedAdapter;
use crate::sandbox::sandbox_store;
use std::path::PathBuf;

const CREATED: &str = r#"{"ok":true,"state":"stopped","message":"Sandbox dev-1 is created.","next":"Run `agent-desktop sandbox start dev-1`."}"#;
const STOPPED: &str = r#"{"ok":true,"state":"stopped","message":"Sandbox dev-1 is stopped."}"#;
const RUNNING: &str = r#"{"ok":true,"state":"running","message":"Sandbox dev-1 is running."}"#;
const REMOVED: &str = r#"{"ok":true,"state":"stopped","message":"Sandbox dev-1 is removed."}"#;
const UNAVAILABLE: &str =
    r#"{"ok":true,"state":"unavailable","message":"This Mac cannot run virtual machines."}"#;

fn create_action() -> SandboxAction {
    SandboxAction::Create {
        name: "dev-1".to_string(),
        provider: "stub".to_string(),
        set: Vec::new(),
    }
}

fn sandboxes_root(home: &HomeGuard) -> PathBuf {
    home.path().join(".agent-desktop").join("sandboxes")
}

#[test]
fn a_failed_create_leaves_no_record_unless_the_provider_left_files() {
    {
        let home = HomeGuard::new();
        let adapter = ScriptedAdapter::new();

        execute(create_action(), &adapter).expect_err("the provider create fails");

        assert!(
            !sandboxes_root(&home).join("dev-1").exists(),
            "a failed create leaves no sandbox behind"
        );
    }

    let home = HomeGuard::new();
    let sandbox_dir = sandboxes_root(&home).join("dev-1");
    crate::storage::private_file_parent::ensure_private(&sandbox_dir)
        .expect("private sandbox directory");
    std::fs::write(sandbox_dir.join("id_ed25519"), b"key").expect("provider file");
    let adapter = ScriptedAdapter::new();

    execute(create_action(), &adapter).expect_err("the provider create fails");

    assert!(
        sandbox_store::load(&sandboxes_root(&home), "dev-1").is_ok(),
        "the record stays so status can report what the provider left"
    );
}

#[test]
fn create_on_a_provider_that_cannot_run_here_is_refused_and_leaves_no_record() {
    let home = HomeGuard::new();
    let adapter = ScriptedAdapter::new().reply("create", UNAVAILABLE, 0);

    let error = execute(create_action(), &adapter).expect_err("the provider is unavailable");

    assert_eq!(error.code(), "PLATFORM_NOT_SUPPORTED");
    assert!(
        !sandboxes_root(&home).join("dev-1").exists(),
        "an unavailable provider leaves no sandbox behind"
    );
}

#[test]
fn removing_a_running_sandbox_is_refused_and_keeps_the_record() {
    let home = HomeGuard::new();
    let adapter = ScriptedAdapter::new()
        .reply("create", CREATED, 0)
        .reply("status", RUNNING, 0);
    execute(create_action(), &adapter).expect("create");

    let error = execute(
        SandboxAction::Rm {
            name: "dev-1".to_string(),
        },
        &adapter,
    )
    .expect_err("a running sandbox is refused");

    assert_eq!(error.code(), "INVALID_ARGS");
    assert_eq!(adapter.requests().len(), 2, "the rm verb never runs");
    assert!(
        sandbox_store::load(&sandboxes_root(&home), "dev-1").is_ok(),
        "the record stays"
    );
}

#[test]
fn removing_a_stopped_sandbox_removes_the_record() {
    let home = HomeGuard::new();
    let adapter = ScriptedAdapter::new()
        .reply("create", CREATED, 0)
        .reply("status", STOPPED, 0)
        .reply("rm", REMOVED, 0);
    execute(create_action(), &adapter).expect("create");

    execute(
        SandboxAction::Rm {
            name: "dev-1".to_string(),
        },
        &adapter,
    )
    .expect("a stopped sandbox is removed");

    assert_eq!(
        sandbox_store::load(&sandboxes_root(&home), "dev-1")
            .expect_err("the record is gone")
            .code(),
        "SANDBOX_NOT_FOUND"
    );
}
