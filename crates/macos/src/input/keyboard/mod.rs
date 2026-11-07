#[cfg(target_os = "macos")]
pub(super) mod keyboard_event;
pub(crate) mod keyboard_layout;
pub(crate) mod keyboard_map;

use agent_desktop_core::{AdapterError, Deadline, ErrorCode, KeyCombo, Modifier};

#[cfg(target_os = "macos")]
pub(crate) fn synthesize_key(
    combo: &KeyCombo,
    target_pid: Option<i32>,
    deadline: Deadline,
) -> Result<(), AdapterError> {
    tracing::debug!(target_pid, "keyboard: synthesize atomic key press");
    let key_modifier = side_modifier_key(&combo.key);
    let (mut keys, mut held) = (Vec::new(), Vec::new());
    for modifier in combo.modifiers.iter().chain(&key_modifier) {
        match keyboard_event::side_key(modifier) {
            Some(key) if !keys.contains(&key) => keys.push(key),
            Some(_) => {}
            None => held.push(modifier.clone()),
        }
    }
    if key_modifier.is_none() {
        keys.push((
            keyboard_map::key_name_to_code(&combo.key)?,
            core_graphics::event::CGEventFlags::empty(),
        ));
    }
    let flags = crate::input::mouse::event_flags(&held);
    keyboard_event::post_key(&keys, flags, target_pid, deadline)
}

pub(crate) fn side_modifier_key(key: &str) -> Option<Modifier> {
    Modifier::from_name(key).filter(|modifier| modifier.generic() != *modifier)
}

#[cfg(target_os = "macos")]
pub(crate) fn reject_standalone_key_state(
    _combo: &KeyCombo,
    _down: bool,
) -> Result<(), AdapterError> {
    Err(AdapterError::new(
        ErrorCode::ActionNotSupported,
        "Standalone key-down/key-up is unavailable in stateless mode",
    )
    .with_details(serde_json::json!({
        "raw_input_emitted": false,
        "requires_daemon_owned_transaction": true,
    }))
    .with_suggestion(
        "Use the atomic 'press' command; spanning key holds require a daemon-owned session that can release keys after disconnect",
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn synthesize_text(
    text: &str,
    target_pid: i32,
    deadline: Deadline,
    verify_target: impl FnMut(Deadline) -> Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    tracing::debug!(
        characters = text.chars().count(),
        target_pid,
        "keyboard: synthesize Unicode text"
    );
    crate::input::keyboard::keyboard_event::post_text(text, target_pid, deadline, verify_target)
}

#[cfg(target_os = "macos")]
pub(crate) fn preflight_text(text: &str, deadline: Deadline) -> Result<(), AdapterError> {
    crate::input::keyboard::keyboard_event::preflight_text(text, deadline)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn synthesize_key(
    _combo: &KeyCombo,
    _target_pid: Option<i32>,
    _deadline: Deadline,
) -> Result<(), AdapterError> {
    Err(AdapterError::not_supported("synthesize_key"))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn reject_standalone_key_state(
    _combo: &KeyCombo,
    _down: bool,
) -> Result<(), AdapterError> {
    Err(AdapterError::not_supported("key_state"))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn synthesize_text(
    _text: &str,
    _target_pid: i32,
    _deadline: Deadline,
    _verify_target: impl FnMut(Deadline) -> Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    Err(AdapterError::not_supported("synthesize_text"))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn preflight_text(_text: &str, _deadline: Deadline) -> Result<(), AdapterError> {
    Err(AdapterError::not_supported("synthesize_text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_key_state_is_rejected_without_emission() {
        let error = reject_standalone_key_state(
            &KeyCombo {
                key: "shift".into(),
                modifiers: Vec::new(),
            },
            true,
        )
        .expect_err("stateless holds must fail closed");

        assert_eq!(error.code, ErrorCode::ActionNotSupported);
        assert_eq!(error.details.unwrap()["raw_input_emitted"], false);
    }
}
