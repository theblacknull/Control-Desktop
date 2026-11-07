use super::*;
use crate::commands::wait::{WaitModeArgs, WaitPredicateArgs};

fn args(mode: WaitModeArgs) -> WaitArgs {
    WaitArgs {
        mode,
        predicate: WaitPredicateArgs {
            predicate: None,
            value: None,
            action: None,
            count: None,
        },
        timeout_ms: 1_000,
        app: None,
    }
}

fn mode() -> WaitModeArgs {
    WaitModeArgs::default()
}

#[test]
fn event_with_window_title_narrowing_is_a_single_mode() {
    let result = validate_wait_mode(&args(WaitModeArgs {
        event: Some("window-opened".into()),
        window: Some("Untitled".into()),
        ..mode()
    }));
    assert!(
        result.is_ok(),
        "--window alongside --event must narrow the event wait, not count as a second mode: {result:?}"
    );
}

#[test]
fn event_alone_is_still_a_single_mode() {
    let result = validate_wait_mode(&args(WaitModeArgs {
        event: Some("window-opened".into()),
        ..mode()
    }));
    assert!(result.is_ok());
}

#[test]
fn window_and_element_together_remain_ambiguous() {
    let result = validate_wait_mode(&args(WaitModeArgs {
        window: Some("Untitled".into()),
        element: Some("@s8f3k2p9:e1".into()),
        ..mode()
    }));
    assert!(result.is_err());
}

#[test]
fn from_args_threads_window_title_into_event_mode() {
    let parsed = WaitMode::from_args(args(WaitModeArgs {
        event: Some("window-opened".into()),
        window: Some("Untitled".into()),
        ..mode()
    }))
    .unwrap();
    match parsed {
        WaitMode::Event {
            event,
            window_title,
            ..
        } => {
            assert_eq!(event, "window-opened");
            assert_eq!(window_title.as_deref(), Some("Untitled"));
        }
        _ => panic!("expected WaitMode::Event, got a different mode"),
    }
}

#[test]
fn from_args_maps_surface_variants_to_menu_open_state() {
    let open = WaitMode::from_args(args(WaitModeArgs {
        surface: Some(SurfaceWait::Menu),
        ..mode()
    }))
    .unwrap();
    assert!(matches!(open, WaitMode::Menu { open: true, .. }));

    let closed = WaitMode::from_args(args(WaitModeArgs {
        surface: Some(SurfaceWait::MenuClosed),
        ..mode()
    }))
    .unwrap();
    assert!(matches!(closed, WaitMode::Menu { open: false, .. }));
}

#[test]
fn from_args_threads_text_filter_into_notification_mode() {
    let parsed = WaitMode::from_args(args(WaitModeArgs {
        surface: Some(SurfaceWait::Notification),
        text: Some("done".into()),
        ..mode()
    }))
    .unwrap();
    match parsed {
        WaitMode::Notification { text, .. } => assert_eq!(text.as_deref(), Some("done")),
        _ => panic!("expected WaitMode::Notification, got a different mode"),
    }
}

#[test]
fn surface_and_element_together_remain_ambiguous() {
    let result = validate_wait_mode(&args(WaitModeArgs {
        surface: Some(SurfaceWait::Menu),
        element: Some("@s8f3k2p9:e1".into()),
        ..mode()
    }));

    assert_eq!(result.unwrap_err().code(), "INVALID_ARGS");
}

#[test]
fn app_event_rejects_window_filter_immediately() {
    let result = validate_wait_mode(&args(WaitModeArgs {
        event: Some("app-launched".into()),
        window_id: Some("w-1".into()),
        ..mode()
    }));

    assert_eq!(result.unwrap_err().code(), "INVALID_ARGS");
}

#[test]
fn surface_event_requires_app_scope() {
    let result = validate_wait_mode(&args(WaitModeArgs {
        event: Some("surface-appeared".into()),
        ..mode()
    }));

    assert_eq!(result.unwrap_err().code(), "INVALID_ARGS");
}

#[test]
fn surface_event_accepts_app_scope() {
    let mut request = args(WaitModeArgs {
        event: Some("surface-dismissed".into()),
        ..mode()
    });
    request.app = Some("TextEdit".into());

    assert!(validate_wait_mode(&request).is_ok());
}

fn ordinary_modes() -> Vec<(&'static str, WaitModeArgs)> {
    vec![
        (
            "duration",
            WaitModeArgs {
                ms: Some(0),
                ..mode()
            },
        ),
        (
            "element",
            WaitModeArgs {
                element: Some("@s8f3k2p9:e1".into()),
                ..mode()
            },
        ),
        (
            "window",
            WaitModeArgs {
                window: Some("Document".into()),
                ..mode()
            },
        ),
        (
            "text",
            WaitModeArgs {
                text: Some("ready".into()),
                ..mode()
            },
        ),
        (
            "menu",
            WaitModeArgs {
                surface: Some(SurfaceWait::Menu),
                ..mode()
            },
        ),
        (
            "menu-closed",
            WaitModeArgs {
                surface: Some(SurfaceWait::MenuClosed),
                ..mode()
            },
        ),
        (
            "notification",
            WaitModeArgs {
                surface: Some(SurfaceWait::Notification),
                ..mode()
            },
        ),
        (
            "event",
            WaitModeArgs {
                event: Some("window-opened".into()),
                ..mode()
            },
        ),
        ("none", mode()),
    ]
}

#[test]
fn action_requires_element_for_every_non_element_mode() {
    for (name, mode) in ordinary_modes()
        .into_iter()
        .filter(|(name, _)| *name != "element")
    {
        let mut request = args(mode);
        request.predicate.action = Some("type".into());
        let error = validate_wait_mode(&request).expect_err(name);
        assert_eq!(error.code(), "INVALID_ARGS", "{name}");
        assert_eq!(
            error.to_string(),
            "--action requires --element and --predicate actionable",
            "{name}"
        );
        assert!(
            error
                .suggestion()
                .is_some_and(|suggestion| suggestion.contains("--element")
                    && suggestion.contains("--predicate actionable")),
            "{name}"
        );
    }
}

#[test]
fn window_id_requires_event_for_every_non_event_mode() {
    for (name, mut mode) in ordinary_modes()
        .into_iter()
        .filter(|(name, _)| *name != "event")
    {
        mode.window_id = Some("w-1".into());
        let error = validate_wait_mode(&args(mode)).expect_err(name);
        assert_eq!(error.code(), "INVALID_ARGS", "{name}");
        assert_eq!(error.to_string(), "--window-id requires --event", "{name}");
        assert!(
            error
                .suggestion()
                .is_some_and(|suggestion| suggestion.contains("--event")),
            "{name}"
        );
    }
}

#[test]
fn ordinary_wait_modes_still_parse() {
    for (name, mode) in ordinary_modes()
        .into_iter()
        .filter(|(name, _)| *name != "none")
    {
        assert!(WaitMode::from_args(args(mode)).is_ok(), "{name}");
    }
}

#[test]
fn actionable_waits_preserve_each_supported_action() {
    use crate::actions::action::Action;

    for (action, expected) in [
        (None, Action::Click),
        (Some("click"), Action::Click),
        (Some("type"), Action::TypeText(String::new())),
        (Some("set-value"), Action::SetValue(String::new())),
        (Some("clear"), Action::Clear),
    ] {
        let mut request = args(WaitModeArgs {
            element: Some("@s8f3k2p9:e1".into()),
            ..mode()
        });
        request.predicate.predicate = Some("actionable".into());
        request.predicate.action = action.map(str::to_owned);
        let parsed = WaitMode::from_args(request).unwrap();
        let WaitMode::Element {
            ref_id,
            predicate: wait_predicate::ElementPredicate::Actionable(request),
        } = parsed
        else {
            panic!("expected actionable element wait");
        };
        assert_eq!(ref_id, "@s8f3k2p9:e1");
        assert_eq!(
            serde_json::to_value(request.action).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}

#[test]
fn element_action_errors_remain_specific() {
    for predicate in [
        None,
        Some("exists"),
        Some("enabled"),
        Some("visible"),
        Some("value"),
        Some("unknown"),
    ] {
        let mut request = args(WaitModeArgs {
            element: Some("@s8f3k2p9:e1".into()),
            ..mode()
        });
        request.predicate.predicate = predicate.map(str::to_owned);
        request.predicate.action = Some("type".into());
        let error = WaitMode::from_args(request).err().unwrap();
        assert_eq!(error.code(), "INVALID_ARGS");
        let expected = if predicate == Some("unknown") {
            "Unknown wait predicate 'unknown'"
        } else {
            "--action is only valid with --predicate actionable"
        };
        assert_eq!(error.to_string(), expected);
    }
    let mut request = args(WaitModeArgs {
        element: Some("@s8f3k2p9:e1".into()),
        ..mode()
    });
    request.predicate.predicate = Some("actionable".into());
    request.predicate.action = Some("hover".into());
    let error = WaitMode::from_args(request).err().unwrap();
    assert_eq!(error.code(), "INVALID_ARGS");
    assert_eq!(error.to_string(), "Unknown actionability action 'hover'");
}

#[test]
fn lifecycle_events_preserve_optional_window_filters() {
    for event in ["window-opened", "window-closed", "focus-changed"] {
        for (id, title) in [
            (None, None),
            (Some("w-1"), None),
            (None, Some("Document")),
            (Some("w-1"), Some("Document")),
        ] {
            let parsed = WaitMode::from_args(args(WaitModeArgs {
                event: Some(event.into()),
                window_id: id.map(str::to_owned),
                window: title.map(str::to_owned),
                ..mode()
            }))
            .unwrap();
            let WaitMode::Event {
                event: actual,
                window_id,
                window_title,
                ..
            } = parsed
            else {
                panic!("expected event wait");
            };
            assert_eq!(actual, event);
            assert_eq!(window_id.as_deref(), id);
            assert_eq!(window_title.as_deref(), title);
        }
    }
}

#[test]
fn app_and_surface_events_reject_all_window_filters() {
    for event in [
        "app-launched",
        "app-terminated",
        "surface-appeared",
        "surface-dismissed",
    ] {
        for (id, title) in [
            (Some("w-1"), None),
            (None, Some("Document")),
            (Some("w-1"), Some("Document")),
        ] {
            let mut request = args(WaitModeArgs {
                event: Some(event.into()),
                window_id: id.map(str::to_owned),
                window: title.map(str::to_owned),
                ..mode()
            });
            request.app = Some("Example".into());
            let error = WaitMode::from_args(request).err().unwrap();
            assert_eq!(error.code(), "INVALID_ARGS");
            assert_eq!(
                error.to_string(),
                format!("--event {event} does not carry window identity")
            );
        }
    }
}
