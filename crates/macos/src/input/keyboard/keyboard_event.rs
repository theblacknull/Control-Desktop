use agent_desktop_core::{AdapterError, Deadline, ErrorCode, Modifier};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use std::time::Duration;

const KEY_SETTLE: Duration = Duration::from_millis(4);
const TEXT_CHUNK_BUDGET: Duration = Duration::from_millis(400);
const MAX_TEXT_UTF16: usize = 1_000_000;
const TEXT_CHUNK_UTF16: usize = 32;

pub(crate) fn side_key(modifier: &Modifier) -> Option<(u16, CGEventFlags)> {
    let (key_code, flags, device) = match modifier {
        Modifier::LeftMeta => (55, CGEventFlags::CGEventFlagCommand, 0x08),
        Modifier::RightMeta => (54, CGEventFlags::CGEventFlagCommand, 0x10),
        Modifier::LeftAlt => (58, CGEventFlags::CGEventFlagAlternate, 0x20),
        Modifier::RightAlt => (61, CGEventFlags::CGEventFlagAlternate, 0x40),
        Modifier::Meta | Modifier::Ctrl | Modifier::Alt | Modifier::Shift => return None,
    };
    Some((key_code, flags | CGEventFlags::from_bits_retain(device)))
}

pub(crate) fn post_key(
    keys: &[(u16, CGEventFlags)],
    flags: CGEventFlags,
    target_pid: Option<i32>,
    deadline: Deadline,
) -> Result<(), AdapterError> {
    let mut delivery = crate::actions::DeliveryTracker::default();
    ensure_budget(deadline, 1, delivery)?;
    let events = event_source()
        .and_then(|source| key_events(&source, keys, flags))
        .map_err(|error| delivery.annotate(error))?;
    post_pairs(events, target_pid, deadline, &mut delivery, 1)
}

fn key_events(
    source: &CGEventSource,
    keys: &[(u16, CGEventFlags)],
    flags: CGEventFlags,
) -> Result<Vec<(CGEvent, CGEvent)>, AdapterError> {
    let mut sides = CGEventFlags::empty();
    keys.iter()
        .map(|&(key_code, own)| {
            let held = flags | sides;
            let released = if own.is_empty() || !sides.is_empty() {
                held
            } else {
                CGEventFlags::empty()
            };
            sides |= own;
            Ok((
                create_key_event(source, key_code, true, held | own)?,
                create_key_event(source, key_code, false, released)?,
            ))
        })
        .collect()
}

pub(crate) fn post_text(
    text: &str,
    target_pid: i32,
    deadline: Deadline,
    mut verify_target: impl FnMut(Deadline) -> Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    preflight_text(text, deadline)?;
    let chunks = text_chunks(text)?;
    let total = chunks.len();
    let mut delivery = crate::actions::DeliveryTracker::default();
    for chunk in chunks {
        ensure_budget(deadline, total, delivery)?;
        verify_target(deadline).map_err(|error| {
            delivery.annotate(error.with_details(serde_json::json!({
                "delivered_chunks": delivery.delivered_units(),
                "total_chunks": total,
            })))
        })?;
        let source = event_source().map_err(|error| delivery.annotate(error))?;
        let down = create_key_event(&source, 0, true, CGEventFlags::empty())
            .map_err(|error| delivery.annotate(error))?;
        down.set_string_from_utf16_unchecked(&chunk);
        let up = create_key_event(&source, 0, false, CGEventFlags::empty())
            .map_err(|error| delivery.annotate(error))?;
        post_pairs(
            vec![(down, up)],
            Some(target_pid),
            deadline,
            &mut delivery,
            total,
        )?;
    }
    Ok(())
}

pub(crate) fn preflight_text(text: &str, deadline: Deadline) -> Result<(), AdapterError> {
    let chunks = planned_chunk_count(text)?;
    let multiplier = u32::try_from(chunks)
        .map_err(|_| AdapterError::new(ErrorCode::InvalidArgs, "Text payload is too large"))?;
    let required = TEXT_CHUNK_BUDGET
        .checked_mul(multiplier)
        .ok_or_else(|| AdapterError::new(ErrorCode::InvalidArgs, "Text payload is too large"))?;
    let remaining = deadline.remaining();
    if remaining < required {
        return Err(crate::actions::DeliveryTracker::default().annotate(
            AdapterError::timeout("Text cannot be delivered safely within the remaining deadline")
                .with_details(serde_json::json!({
                    "delivered_chunks": 0,
                    "total_chunks": chunks,
                    "required_ms": required.as_millis(),
                    "remaining_ms": remaining.as_millis(),
                })),
        ));
    }
    Ok(())
}

fn post_pairs(
    events: Vec<(CGEvent, CGEvent)>,
    target_pid: Option<i32>,
    deadline: Deadline,
    delivery: &mut crate::actions::DeliveryTracker,
    total: usize,
) -> Result<(), AdapterError> {
    ensure_budget(deadline, total, *delivery)?;
    let mut release = KeyReleaseGuard {
        events: Vec::with_capacity(events.len()),
        target_pid,
    };
    for (down, up) in events {
        post(&down, target_pid);
        release.events.push(up);
    }
    delivery.mark_delivered();
    sleep_bounded(deadline, KEY_SETTLE, *delivery, total)?;
    drop(release);
    ensure_budget(deadline, total, *delivery)
}

struct KeyReleaseGuard {
    events: Vec<CGEvent>,
    target_pid: Option<i32>,
}

impl Drop for KeyReleaseGuard {
    fn drop(&mut self) {
        for (index, event) in self.events.iter().rev().enumerate() {
            if index > 0 {
                std::thread::sleep(KEY_SETTLE);
            }
            post(event, self.target_pid);
        }
    }
}

fn create_key_event(
    source: &CGEventSource,
    key_code: u16,
    down: bool,
    flags: CGEventFlags,
) -> Result<CGEvent, AdapterError> {
    let event = CGEvent::new_keyboard_event(source.clone(), key_code, down)
        .map_err(|()| AdapterError::internal("CGEvent::new_keyboard_event failed"))?;
    event.set_flags(flags);
    Ok(event)
}

fn event_source() -> Result<CGEventSource, AdapterError> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| AdapterError::internal("Failed to create keyboard CGEventSource"))
}

fn post(event: &CGEvent, target_pid: Option<i32>) {
    if let Some(pid) = target_pid {
        event.post_to_pid(pid);
    } else {
        event.post(CGEventTapLocation::HID);
    }
}

fn sleep_bounded(
    deadline: Deadline,
    duration: Duration,
    delivery: crate::actions::DeliveryTracker,
    total: usize,
) -> Result<(), AdapterError> {
    let pause = deadline
        .remaining_slice(duration)
        .map_err(|error| delivery.annotate(error))?;
    std::thread::sleep(pause);
    if pause < duration {
        ensure_budget(deadline, total, delivery)
    } else {
        Ok(())
    }
}

fn ensure_budget(
    deadline: Deadline,
    total: usize,
    delivery: crate::actions::DeliveryTracker,
) -> Result<(), AdapterError> {
    if !deadline.is_expired() {
        return Ok(());
    }
    Err(
        delivery.annotate(deadline.timeout_error().with_details(serde_json::json!({
            "delivered_chunks": delivery.delivered_units(),
            "total_chunks": total,
        }))),
    )
}

fn text_chunks(text: &str) -> Result<Vec<Vec<u16>>, AdapterError> {
    let _ = planned_chunk_count(text)?;
    let mut chunks = Vec::new();
    let mut current = Vec::with_capacity(TEXT_CHUNK_UTF16);
    for character in text.chars() {
        let mut encoded = [0_u16; 2];
        let encoded = character.encode_utf16(&mut encoded);
        if !current.is_empty() && current.len() + encoded.len() > TEXT_CHUNK_UTF16 {
            chunks.push(std::mem::take(&mut current));
        }
        current.extend_from_slice(encoded);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    Ok(chunks)
}

fn planned_chunk_count(text: &str) -> Result<usize, AdapterError> {
    let mut units = 0_usize;
    let mut chunks = 0_usize;
    let mut current = 0_usize;
    for character in text.chars() {
        let encoded = character.len_utf16();
        units = units.checked_add(encoded).ok_or_else(text_too_large)?;
        if units > MAX_TEXT_UTF16 {
            return Err(text_too_large());
        }
        if current > 0 && current + encoded > TEXT_CHUNK_UTF16 {
            chunks += 1;
            current = 0;
        }
        current += encoded;
    }
    Ok(chunks + usize::from(current > 0))
}

fn text_too_large() -> AdapterError {
    AdapterError::new(
        ErrorCode::InvalidArgs,
        "Text input must not exceed 1000000 UTF-16 code units",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_keys_go_down_in_order_and_release_their_own_device_bits() {
        let source = event_source().unwrap();
        let keys = [
            side_key(&Modifier::LeftMeta).unwrap(),
            side_key(&Modifier::RightMeta).unwrap(),
            (0, CGEventFlags::empty()),
        ];
        let events = key_events(&source, &keys, CGEventFlags::CGEventFlagShift).unwrap();
        let shape = |event: &CGEvent| {
            (
                event.get_integer_value_field(
                    core_graphics::event::EventField::KEYBOARD_EVENT_KEYCODE,
                ),
                event.get_type() as u32,
                event.get_flags().bits(),
            )
        };
        let shapes: Vec<_> = events
            .iter()
            .map(|(down, up)| (shape(down), shape(up)))
            .collect();

        assert_eq!(
            shapes,
            [
                ((55, 12, 0x12_0008), (55, 12, 0)),
                ((54, 12, 0x12_0018), (54, 12, 0x12_0008)),
                ((0, 10, 0x12_0018), (0, 11, 0x12_0018)),
            ]
        );
    }

    #[test]
    fn text_chunks_preserve_unicode_without_splitting_surrogates() {
        let text = format!("{}😀tail", "a".repeat(TEXT_CHUNK_UTF16 - 1));
        let chunks = text_chunks(&text).unwrap();
        let roundtrip = String::from_utf16(&chunks.concat()).unwrap();

        assert_eq!(roundtrip, text);
        assert!(chunks.iter().all(|chunk| chunk.len() <= TEXT_CHUNK_UTF16));
    }

    #[test]
    fn text_budget_rejects_unbounded_payloads() {
        let text = "x".repeat(MAX_TEXT_UTF16 + 1);
        let error = text_chunks(&text).expect_err("oversized text must fail before CGEvent");

        assert_eq!(error.code, ErrorCode::InvalidArgs);
    }

    #[test]
    fn impossible_delivery_deadline_fails_before_input() {
        let deadline = Deadline::after(1).unwrap();
        let error = preflight_text("payload", deadline).expect_err("deadline must reject plan");

        assert_eq!(error.code, ErrorCode::Timeout);
        assert_eq!(
            error.disposition,
            agent_desktop_core::DeliverySemantics::not_delivered()
        );
    }
}
