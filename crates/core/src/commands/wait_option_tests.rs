use super::test_support::{PredicateAdapter, snapshot_with_one_ref, wait_args};
use super::*;
use crate::adapter::{ActionOps, InputOps, ObservationOps, SystemOps};
use crate::refs::refs_test_support::HomeGuard;

struct RefuseObservation;

impl ObservationOps for RefuseObservation {
    fn list_windows(
        &self,
        _filter: &WindowFilter,
        _deadline: crate::Deadline,
    ) -> Result<Vec<crate::WindowInfo>, crate::AdapterError> {
        panic!("invalid wait options must return before window observation");
    }
}

impl ActionOps for RefuseObservation {}
impl InputOps for RefuseObservation {}
impl SystemOps for RefuseObservation {}

#[test]
fn invalid_wait_options_return_before_mode_observation() {
    for action in [true, false] {
        let mut args = wait_args();
        args.mode.window = Some("Document".into());
        args.timeout_ms = 1_000;
        if action {
            args.predicate.action = Some("type".into());
        } else {
            args.mode.window_id = Some("w-1".into());
        }
        let error = execute(args, &RefuseObservation, &CommandContext::default()).unwrap_err();
        assert_eq!(error.code(), "INVALID_ARGS");
        assert!(error.suggestion().is_some());
    }
}

#[test]
fn actionable_element_wait_executes_with_a_saved_snapshot() {
    let _guard = HomeGuard::new();
    let snapshot_id = snapshot_with_one_ref();
    let adapter = PredicateAdapter {
        state: Some(crate::tree::element_state::ElementState {
            role: "button".into(),
            states: vec![],
            value: None,
            enabled: Some(true),
            hidden: Some(false),
            offscreen: Some(false),
        }),
        value: None,
        bounds: Some(crate::Rect {
            x: 1.0,
            y: 1.0,
            width: 20.0,
            height: 20.0,
        }),
    };
    for action in [None, Some("click")] {
        let mut args = wait_args();
        args.mode.element = Some(crate::refs::ref_token::qualify_ref_id(&snapshot_id, "@e1"));
        args.predicate.predicate = Some("actionable".into());
        args.predicate.action = action.map(str::to_owned);
        args.timeout_ms = 500;
        let result = execute(args, &adapter, &CommandContext::default()).unwrap();
        assert_eq!(result["predicate"], "actionable");
        assert_eq!(result["observed"]["actionable"], true);
    }
}
