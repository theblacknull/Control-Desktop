use super::*;

#[test]
fn changing_inventory_retries_until_two_consecutive_captures_match() {
    let mut attempt = 0;
    let stable = records_fixture(true);
    let result =
        stabilize_records_until(Instant::now() + std::time::Duration::from_secs(1), || {
            attempt += 1;
            Ok(if attempt == 1 {
                records_fixture(false)
            } else {
                stable.clone()
            })
        })
        .unwrap();

    assert_eq!(attempt, 3);
    assert_eq!(result, stable);
}

#[test]
fn persistent_window_churn_times_out_with_exact_attempt_metrics() {
    let mut visible = false;
    let mut attempts = 0_u64;
    let error = stabilize_records_until(
        Instant::now() + std::time::Duration::from_millis(20),
        || {
            attempts += 1;
            visible = !visible;
            Ok(records_fixture(visible))
        },
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Timeout);
    let details = error.details.unwrap();
    assert!(attempts >= 1);
    assert_eq!(details["attempts"].as_u64(), Some(attempts));
    assert_eq!(
        details["churn_events"].as_u64(),
        Some(attempts.saturating_sub(1))
    );
}

#[test]
fn scoped_capture_never_probes_an_unrelated_inaccessible_owner() {
    let eligible = rustc_hash::FxHashSet::from_iter([10]);
    let mut records = scoped_records(
        vec![
            window_dictionary("Target", 10, 7, true),
            window_dictionary("Other", 418, 8, true),
        ],
        WindowRecordScope::Pids(&eligible),
    );
    let mut probed = Vec::new();

    capture_process_instances_with(
        &mut records,
        Instant::now() + std::time::Duration::from_secs(1),
        |pid| {
            probed.push(pid);
            if pid == 418 {
                Err(AdapterError::permission_denied())
            } else {
                Ok(Some(format!("instance-{pid}")))
            }
        },
    )
    .unwrap();

    assert_eq!(probed, [10]);
    assert_eq!(records[0].process_instance.as_deref(), Some("instance-10"));
}

#[test]
fn scoped_capture_propagates_the_selected_owners_denial() {
    let mut records = vec![record("Target", 418, 8, true)];
    let error = capture_process_instances_with(
        &mut records,
        Instant::now() + std::time::Duration::from_secs(1),
        |_| Err(AdapterError::permission_denied()),
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::PermDenied);
}

#[test]
fn pid_set_scope_keeps_all_matching_processes_and_ignores_unrelated_churn() {
    let eligible = rustc_hash::FxHashSet::from_iter([10, 11]);
    let scope = WindowRecordScope::Pids(&eligible);
    let first = scoped_records(
        vec![
            window_dictionary("Target", 10, 7, true),
            window_dictionary("Other", 20, 8, false),
            window_dictionary("Target", 11, 9, true),
        ],
        scope,
    );
    let second = scoped_records(
        vec![
            window_dictionary("Target", 10, 7, true),
            window_dictionary("Other", 20, 8, true),
            window_dictionary("Target", 11, 9, true),
        ],
        scope,
    );

    assert_eq!(first.len(), 2);
    assert_eq!(inventory_signature(&first), inventory_signature(&second));
}

#[test]
fn pid_and_window_scopes_exclude_unrelated_owners_before_identity_reads() {
    for scope in [WindowRecordScope::Pid(10), WindowRecordScope::Window(7)] {
        let mut records = scoped_records(
            vec![
                window_dictionary("Target", 10, 7, true),
                window_dictionary("Other", 20, 8, true),
            ],
            scope,
        );
        let mut probed = Vec::new();
        capture_process_instances_with(
            &mut records,
            Instant::now() + std::time::Duration::from_secs(1),
            |pid| {
                probed.push(pid);
                Ok(Some(format!("instance-{pid}")))
            },
        )
        .unwrap();

        assert_eq!(probed, [10]);
    }
}

#[test]
fn pid_set_scope_excludes_ineligible_owners_before_identity_reads() {
    let eligible = rustc_hash::FxHashSet::from_iter([10, 11]);
    let mut records = scoped_records(
        vec![
            window_dictionary("Target", 10, 7, true),
            window_dictionary("Accessory", 11, 8, true),
            window_dictionary("Protected", 418, 9, true),
        ],
        WindowRecordScope::Pids(&eligible),
    );
    let mut probed = Vec::new();

    capture_process_instances_with(
        &mut records,
        Instant::now() + std::time::Duration::from_secs(1),
        |pid| {
            probed.push(pid);
            if pid == 418 {
                Err(AdapterError::permission_denied())
            } else {
                Ok(Some(format!("instance-{pid}")))
            }
        },
    )
    .unwrap();

    assert_eq!(probed, [10, 11]);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].process_instance.as_deref(), Some("instance-10"));
    assert_eq!(records[1].process_instance.as_deref(), Some("instance-11"));
}

#[test]
fn target_churn_remains_visible_after_scoping() {
    let eligible = rustc_hash::FxHashSet::from_iter([10]);
    let scope = WindowRecordScope::Pids(&eligible);
    let first = scoped_records(vec![window_dictionary("Target", 10, 7, false)], scope);
    let second = scoped_records(vec![window_dictionary("Target", 10, 7, true)], scope);

    assert_eq!(first.len(), 1);
    assert_ne!(inventory_signature(&first), inventory_signature(&second));
}

#[test]
fn unfiltered_capture_remains_fail_closed() {
    let mut records = vec![record("Target", 10, 7, true), record("Other", 418, 8, true)];
    let error = capture_process_instances_with(
        &mut records,
        Instant::now() + std::time::Duration::from_secs(1),
        |pid| {
            if pid == 418 {
                Err(AdapterError::permission_denied())
            } else {
                Ok(Some(format!("instance-{pid}")))
            }
        },
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::PermDenied);
}

fn records_fixture(visible: bool) -> Vec<WindowRecord> {
    vec![record("Fixture", 1, 7, visible)]
}

#[test]
fn persistent_capture_failure_is_not_window_churn() {
    let mut attempts = 0;
    let error = stabilize_records_until(
        Instant::now() + std::time::Duration::from_millis(20),
        || {
            attempts += 1;
            Err(inventory_error("unavailable"))
        },
    )
    .unwrap_err();
    let details = error.details.unwrap();
    assert_eq!(details["attempts"], attempts);
    assert_eq!(details["churn_events"], 0);
    assert_eq!(
        details["last_failure_details"]["source"],
        "core_graphics_windows"
    );
}
