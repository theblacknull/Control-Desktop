use super::*;

#[cfg(target_os = "windows")]
#[test]
fn process_image_name_finds_the_current_process() {
    let name = process_image_name(ProcessId::from(std::process::id()));

    assert!(
        name.is_some_and(|name| !name.is_empty()),
        "the current process must find its own image name in the process table"
    );
}

#[test]
fn token_shape_mirrors_macos_is_two_component_creation_time() {
    let identity = ProcessIdentity {
        pid: ProcessId::new(1),
        creation_seconds: 1_700_000_000,
        creation_subtick: 123_456,
    };

    assert_eq!(identity.token(), "windows-proc-v1:1700000000:123456");
}

#[test]
fn two_processes_started_in_the_same_second_stay_distinct() {
    let first = ProcessIdentity {
        pid: ProcessId::new(1),
        creation_seconds: 1_700_000_000,
        creation_subtick: 100,
    };
    let second = ProcessIdentity {
        pid: ProcessId::new(1),
        creation_seconds: 1_700_000_000,
        creation_subtick: 200,
    };

    assert_ne!(first, second);
    assert_ne!(first.token(), second.token());
}

#[test]
fn a_different_generation_is_a_different_identity() {
    let before = ProcessIdentity {
        pid: ProcessId::new(1),
        creation_seconds: 1_700_000_000,
        creation_subtick: 100,
    };
    let after = ProcessIdentity {
        pid: ProcessId::new(1),
        creation_seconds: 1_700_000_100,
        creation_subtick: 100,
    };

    assert_ne!(before, after);
    assert_eq!(before.pid, after.pid);
    assert_ne!(before.token(), after.token());
}

#[test]
fn a_shape_mismatch_matches_nothing() {
    let pid = ProcessId::new(1);

    assert!(!matches_instance(pid, "macos-proc-v1:1:2").unwrap());
    assert!(!matches_instance(pid, "windows-proc-v2:1:2").unwrap());
    assert!(!matches_instance(pid, "windows-proc-v1:nope:2").unwrap());
    assert!(!matches_instance(pid, "").unwrap());
}

#[test]
fn a_fresh_token_matches_its_own_process() {
    let pid = ProcessId::from(std::process::id());
    let Some(token) = token_for_pid(pid).unwrap() else {
        return;
    };

    assert!(
        matches_instance(pid, &token).unwrap(),
        "a freshly captured token must match the same process"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn a_shared_table_walks_the_image_inventory_once_for_many_windows() {
    inventory_calls::take();
    let table = ProcessTable::new();
    let pid = ProcessId::from(std::process::id());

    for _ in 0..8 {
        let _ = table.image_name(pid);
    }

    let (table_walks, single_walks) = inventory_calls::take();
    assert_eq!(
        table_walks, 1,
        "the shared table must walk the process inventory exactly once"
    );
    assert_eq!(
        single_walks, 0,
        "lookups must not fall back to the per-pid walk"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn a_shared_table_captures_one_generation_token_per_process() {
    inventory_calls::take_identity_captures();
    let table = ProcessTable::new();
    let pid = ProcessId::from(std::process::id());

    let first = table.token(pid);
    let second = table.token(pid);

    assert_eq!(first, second);
    assert_eq!(
        inventory_calls::take_identity_captures(),
        1,
        "several windows of one process must share one token capture"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn an_exited_process_with_its_child_handle_held_is_not_running() {
    for exit_code in [0, 259] {
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "exit", &exit_code.to_string()])
            .spawn()
            .expect("spawn child");
        let pid = ProcessId::from(child.id());
        assert_eq!(
            child.wait().expect("wait for child exit").code(),
            Some(exit_code)
        );

        assert!(!process_is_running(pid), "child exited with {exit_code}");
    }
    assert!(process_is_running(ProcessId::from(std::process::id())));
}

/// The recycled-pid case: a well-formed token whose generation does not
/// match the live process at that pid - the shape a new process reusing
/// an old HWND's pid trips - must fail closed on both entry points.
#[cfg(target_os = "windows")]
fn table_with_image(captured_at_ticks: u64, name: &str, pid: ProcessId) -> ProcessTable {
    use std::cell::{OnceCell, RefCell};
    use std::collections::HashMap;

    ProcessTable {
        images: OnceCell::from(ImageTable {
            captured_at_ticks,
            names: HashMap::from([(pid, name.to_string())]),
        }),
        identities: RefCell::new(HashMap::new()),
    }
}

#[cfg(target_os = "windows")]
#[test]
fn a_cached_image_is_ignored_when_the_pid_generation_is_newer_than_the_snapshot() {
    let pid = ProcessId::from(std::process::id());
    let identity = ProcessIdentity::capture(pid)
        .expect("the current process is capturable")
        .expect("the current process has a generation");
    let table = table_with_image(
        identity.creation_ticks().saturating_sub(1),
        "stale.exe",
        pid,
    );

    let name = table.image_name(pid).expect("a live image name");

    assert_ne!(
        name, "stale.exe",
        "a pid recycled after the snapshot must not keep the predecessor's image"
    );
    assert!(!name.is_empty());
}

#[cfg(target_os = "windows")]
#[test]
fn a_cached_image_is_used_when_the_pid_generation_predates_the_snapshot() {
    let pid = ProcessId::from(std::process::id());
    let identity = ProcessIdentity::capture(pid)
        .expect("the current process is capturable")
        .expect("the current process has a generation");
    let table = table_with_image(identity.creation_ticks() + 1, "cached.exe", pid);

    assert_eq!(
        table.image_name(pid).as_deref(),
        Some("cached.exe"),
        "an image captured after the process started is valid for that generation"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn a_failed_identity_capture_is_not_cached() {
    let table = ProcessTable::new();

    assert!(
        table.identity(ProcessId::new(u32::MAX)).is_none(),
        "an unopenable pid has no identity"
    );
    assert_eq!(
        table.cached_identity_count(),
        0,
        "a failed capture must be retried rather than cached as a negative fact"
    );
}

#[test]
fn a_generation_mismatched_token_fails_closed() {
    let pid = ProcessId::from(std::process::id());
    let Some(current) = ProcessIdentity::capture(pid).unwrap() else {
        return;
    };
    let corrupted = ProcessIdentity {
        creation_seconds: current.creation_seconds.wrapping_add(1),
        ..current
    };

    assert!(
        !corrupted.still_matches().unwrap(),
        "a different generation of the same pid must not still-match"
    );
    assert!(
        !matches_instance(pid, &corrupted.token()).unwrap(),
        "a generation-mismatched token must not match the live process"
    );
}
