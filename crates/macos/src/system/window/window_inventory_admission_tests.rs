use super::*;

fn ax_state_with_standard_windows(
    focused: FocusedWindowIdentity,
    standard_window_ids: impl IntoIterator<Item = i64>,
) -> crate::system::window::window_ax_state::WindowAxState {
    let mut state = ax_state(focused);
    state.standard_window_ids = standard_window_ids.into_iter().collect();
    state
}

#[test]
fn raised_standard_window_is_listed_and_joins_the_ax_focus_number() {
    let windows = windows_from_records_with_focus(
        vec![record_at_layer("Zen", 10, "Home / X", 13458, 2)],
        false,
        |_| {
            Ok(ax_state_with_standard_windows(
                Some((Some("Home / X".to_string()), Some(13458))),
                [13458],
            ))
        },
        |_, _| Ok(true),
    )
    .unwrap();

    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].id, "w-13458");
    assert!(windows[0].state.is_focused);
}

#[test]
fn raised_window_without_a_standard_ax_subrole_stays_excluded() {
    let windows = windows_from_records_with_focus(
        vec![record_at_layer("Zen", 10, "Notification", 15089, 101)],
        false,
        |_| Ok(ax_state_with_standard_windows(None, [13458])),
        |_, _| Ok(true),
    )
    .unwrap();

    assert!(windows.is_empty());
}

#[test]
fn raised_window_admission_probes_only_the_owning_process() {
    let windows = windows_from_records_with_focus(
        vec![record_at_layer("Zen", 10, "Home / X", 13458, 2)],
        false,
        |pid| {
            assert_eq!(pid, 10);
            Ok(ax_state_with_standard_windows(None, [13458]))
        },
        |_, _| Ok(true),
    )
    .unwrap();

    assert_eq!(windows.len(), 1);
}

#[test]
fn normal_layer_windows_are_kept_without_standard_subrole_evidence() {
    let windows = windows_from_records_with_focus(
        vec![record_at_layer("Preview", 10, "Document", 7, 0)],
        false,
        |_| Ok(ax_state_with_standard_windows(None, [])),
        |_, _| Ok(true),
    )
    .unwrap();

    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].id, "w-7");
}
