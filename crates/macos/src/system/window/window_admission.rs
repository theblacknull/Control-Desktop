use agent_desktop_core::AdapterError;
use rustc_hash::{FxHashMap, FxHashSet};
use std::time::{Duration, Instant};

use crate::system::window::cg_window::WindowRecord;

/// A hung background owner with a raised window must not spend the whole inventory deadline
/// on an admission probe. Each background owner gets a short slice; the frontmost owner keeps
/// the full remaining deadline because its raised window is the one the user is working in.
const BACKGROUND_PROBE_BUDGET: Duration = Duration::from_millis(250);
/// All background admission probes in one capture attempt share this ceiling, so several hung
/// owners cannot add up to the global deadline either.
const BACKGROUND_PROBE_TOTAL_BUDGET: Duration = Duration::from_millis(1_000);

/// Deadline shared by every background owner probed during one capture attempt.
pub(in crate::system) fn background_probe_deadline(deadline: Instant) -> Instant {
    budgeted_deadline(deadline, BACKGROUND_PROBE_TOTAL_BUDGET)
}

pub(in crate::system) fn admission_probe_deadline(
    pid: i32,
    frontmost_pid: Option<i32>,
    deadline: Instant,
    background_deadline: Instant,
) -> Instant {
    if Some(pid) == frontmost_pid {
        deadline
    } else {
        budgeted_deadline(background_deadline, BACKGROUND_PROBE_BUDGET)
    }
}

fn budgeted_deadline(deadline: Instant, budget: Duration) -> Instant {
    Instant::now()
        .checked_add(budget)
        .map_or(deadline, |capped| capped.min(deadline))
}

/// CoreGraphics reports every window layer, and window managers raise real application windows
/// above `kCGNormalWindowLevel` to place them under their own canvases. A raised window stays a
/// real application window when its owning process publishes it through accessibility as a
/// standard window, so the layer alone cannot decide. Menus, panels and status items stay
/// excluded because they are never `AXStandardWindow` elements of their owner.
pub(in crate::system) fn retain_ax_admitted(
    records: Vec<WindowRecord>,
    standard_window_ids: impl FnMut(i32) -> Result<FxHashSet<i64>, AdapterError>,
) -> Vec<WindowRecord> {
    retain_ax_admitted_prioritizing(records, None, standard_window_ids)
}

/// Admits raised windows for the global inventory. The frontmost owner is probed first and keeps
/// the full deadline; every background owner that owns a raised window gets a short slice of a
/// shared probe budget, so a hung background app cannot exhaust the inventory deadline.
pub(in crate::system) fn retain_ax_admitted_until(
    records: Vec<WindowRecord>,
    frontmost_pid: Option<i32>,
    deadline: Instant,
    mut standard_window_ids: impl FnMut(i32, Instant) -> Result<FxHashSet<i64>, AdapterError>,
) -> Vec<WindowRecord> {
    let background_deadline = background_probe_deadline(deadline);
    retain_ax_admitted_prioritizing(records, frontmost_pid, |pid| {
        standard_window_ids(
            pid,
            admission_probe_deadline(pid, frontmost_pid, deadline, background_deadline),
        )
    })
}

/// Same admission rule, but probes `priority_pid` first, before any other owner can spend the
/// probe budget.
pub(in crate::system) fn retain_ax_admitted_prioritizing(
    records: Vec<WindowRecord>,
    priority_pid: Option<i32>,
    mut standard_window_ids: impl FnMut(i32) -> Result<FxHashSet<i64>, AdapterError>,
) -> Vec<WindowRecord> {
    let mut admitted_by_pid: FxHashMap<i32, FxHashSet<i64>> = FxHashMap::default();
    if let Some(pid) = priority_pid {
        if records
            .iter()
            .any(|record| record.pid == pid && !record.is_normal_layer())
        {
            admitted_by_pid.insert(pid, probe(pid, &mut standard_window_ids));
        }
    }
    records
        .into_iter()
        .filter(|record| {
            record.is_normal_layer()
                || admitted_by_pid
                    .entry(record.pid)
                    .or_insert_with(|| probe(record.pid, &mut standard_window_ids))
                    .contains(&record.window_number)
        })
        .collect()
}

fn probe(
    pid: i32,
    standard_window_ids: &mut impl FnMut(i32) -> Result<FxHashSet<i64>, AdapterError>,
) -> FxHashSet<i64> {
    standard_window_ids(pid).unwrap_or_else(|error| {
        tracing::debug!(
            pid,
            error = %error.message,
            "raised-layer window admission probe failed"
        );
        FxHashSet::default()
    })
}

#[cfg(test)]
#[path = "window_admission_tests.rs"]
mod tests;
