use super::*;
use crate::{WindowInfo, WindowState};

fn window(id: &str) -> WindowInfo {
    WindowInfo {
        id: id.into(),
        title: "Window".into(),
        app: "TextEdit".into(),
        pid: crate::ProcessId::new(42),
        process_instance: Some("test-instance".into()),
        bounds: None,
        state: WindowState {
            is_focused: true,
            ..Default::default()
        },
    }
}

#[test]
fn suggestion_names_the_observed_baseline_and_batch() {
    let baseline = SignalBaseline {
        windows: vec![window("w-1"), window("w-2")],
        apps: Vec::new(),
        surfaces: Vec::new(),
        completeness: crate::SignalCompleteness::complete(),
    };

    let suggestion = timeout_suggestion(Some(&baseline));

    assert!(suggestion.contains("2 window(s)"));
    assert!(suggestion.contains("one 'batch'"));
    assert!(suggestion.contains("list-windows"));
    assert!(suggestion.contains("snapshot"));
}

#[test]
fn suggestion_without_baseline_still_mentions_the_race() {
    let suggestion = timeout_suggestion(None);

    assert!(suggestion.contains("No event baseline was captured"));
    assert!(suggestion.contains("one 'batch'"));
}
