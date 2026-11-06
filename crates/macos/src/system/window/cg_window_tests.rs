use super::*;

#[path = "cg_window_capture_tests.rs"]
mod capture;

#[test]
fn missing_optional_owner_name_does_not_block_scoped_windows() {
    use core_foundation::{base::TCFType, number::CFNumber};

    let unnamed = CFDictionary::from_CFType_pairs(&[
        (
            CFString::new("kCGWindowLayer"),
            CFNumber::from(0).as_CFType(),
        ),
        (
            CFString::new("kCGWindowOwnerPID"),
            CFNumber::from(418).as_CFType(),
        ),
        (
            CFString::new("kCGWindowNumber"),
            CFNumber::from(8).as_CFType(),
        ),
    ]);
    let bounds = CFDictionary::from_CFType_pairs(&[
        (CFString::new("X"), CFNumber::from(0).as_CFType()),
        (CFString::new("Y"), CFNumber::from(0).as_CFType()),
        (CFString::new("Width"), CFNumber::from(100).as_CFType()),
        (CFString::new("Height"), CFNumber::from(100).as_CFType()),
    ]);
    let named = CFDictionary::from_CFType_pairs(&[
        (
            CFString::new("kCGWindowLayer"),
            CFNumber::from(0).as_CFType(),
        ),
        (
            CFString::new("kCGWindowOwnerPID"),
            CFNumber::from(10).as_CFType(),
        ),
        (
            CFString::new("kCGWindowNumber"),
            CFNumber::from(7).as_CFType(),
        ),
        (
            CFString::new("kCGWindowOwnerName"),
            CFString::new("Target").as_CFType(),
        ),
        (CFString::new("kCGWindowBounds"), bounds.as_CFType()),
    ]);

    let records =
        records_from_dictionaries(vec![unnamed.clone(), named], WindowRecordScope::Pid(10))
            .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].app_name, "Target");
    assert!(
        records_from_dictionaries(vec![unnamed], WindowRecordScope::Pid(418))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn raised_window_layers_reach_accessibility_admission() {
    let records = scoped_records(
        vec![
            window_dictionary_with_layer("Zen", 10, 13458, 2, true),
            window_dictionary_with_layer("Zen", 10, 15089, 101, false),
        ],
        WindowRecordScope::Pid(10),
    );

    assert_eq!(
        records
            .iter()
            .map(|record| record.layer)
            .collect::<Vec<_>>(),
        [2, 101]
    );

    let mut records = records;
    capture_process_instances_with(
        &mut records,
        Instant::now() + std::time::Duration::from_secs(1),
        |pid| Ok(Some(format!("instance-{pid}"))),
    )
    .unwrap();
    let admitted = crate::system::window::window_admission::retain_ax_admitted(records, |_| {
        Ok(rustc_hash::FxHashSet::from_iter([13458]))
    });

    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].window_number, 13458);
}

#[test]
fn expired_cg_window_deadline_is_rejected_before_native_reads() {
    let error = window_records_until(Instant::now(), WindowRecordScope::Pid(1)).unwrap_err();

    assert_eq!(error.code.as_str(), "TIMEOUT");
}

#[test]
fn deadline_expired_before_any_capture_omits_the_unstable_inventory_kind() {
    let error = window_records_until(Instant::now(), WindowRecordScope::Pid(1)).unwrap_err();

    assert!(error.details.is_none());
}

#[test]
fn expired_deadline_before_any_attempt_is_a_plain_timeout() {
    let error = expired_deadline_error(0, 0, None);

    assert_eq!(error.code, ErrorCode::Timeout);
    assert!(error.details.is_none());
}

#[test]
fn expired_deadline_after_attempts_still_reports_unstable_inventory() {
    let error = expired_deadline_error(3, 2, None);

    let details = error.details.unwrap();
    assert_eq!(details["kind"], "core_graphics_window_inventory_unstable");
    assert_eq!(details["attempts"], 3);
    assert_eq!(details["churn_events"], 2);
}

#[test]
fn malformed_cg_window_data_is_a_retryable_source_failure() {
    let error = missing_field_error("kCGWindowNumber");

    assert_eq!(error.code, ErrorCode::AppUnresponsive);
    let details = error.details.unwrap();
    assert_eq!(details["source"], "core_graphics_windows");
    assert_eq!(details["retryable"], true);
}

#[test]
fn inventory_includes_offscreen_and_minimized_windows() {
    let options = window_list_options();

    assert_eq!(
        options & core_graphics::window::kCGWindowListOptionOnScreenOnly,
        0
    );
}

fn scoped_records(
    dictionaries: Vec<WindowDictionary>,
    scope: WindowRecordScope<'_>,
) -> Vec<WindowRecord> {
    records_from_dictionaries(dictionaries, scope).unwrap()
}

fn window_dictionary(
    app_name: &str,
    pid: i32,
    window_number: i64,
    visible: bool,
) -> WindowDictionary {
    window_dictionary_with_layer(app_name, pid, window_number, 0, visible)
}

fn window_dictionary_with_layer(
    app_name: &str,
    pid: i32,
    window_number: i64,
    layer: i32,
    visible: bool,
) -> WindowDictionary {
    use core_foundation::{base::TCFType, boolean::CFBoolean, number::CFNumber};

    let bounds = CFDictionary::from_CFType_pairs(&[
        (CFString::new("X"), CFNumber::from(0).as_CFType()),
        (CFString::new("Y"), CFNumber::from(0).as_CFType()),
        (CFString::new("Width"), CFNumber::from(100).as_CFType()),
        (CFString::new("Height"), CFNumber::from(100).as_CFType()),
    ]);
    CFDictionary::from_CFType_pairs(&[
        (
            CFString::new("kCGWindowLayer"),
            CFNumber::from(layer).as_CFType(),
        ),
        (
            CFString::new("kCGWindowOwnerPID"),
            CFNumber::from(pid).as_CFType(),
        ),
        (
            CFString::new("kCGWindowNumber"),
            CFNumber::from(window_number).as_CFType(),
        ),
        (
            CFString::new("kCGWindowOwnerName"),
            CFString::new(app_name).as_CFType(),
        ),
        (
            CFString::new("kCGWindowIsOnscreen"),
            CFBoolean::from(visible).as_CFType(),
        ),
        (CFString::new("kCGWindowBounds"), bounds.as_CFType()),
    ])
}

fn record(app_name: &str, pid: i32, window_number: i64, visible: bool) -> WindowRecord {
    WindowRecord {
        app_name: app_name.into(),
        pid,
        title: Some("Window".into()),
        window_number,
        layer: 0,
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
        visible,
        process_instance: Some(format!("instance-{pid}")),
    }
}
