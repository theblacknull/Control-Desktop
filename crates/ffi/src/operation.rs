use agent_desktop_core::{AdapterError, Deadline, InteractionLease, PlatformAdapter};

pub(crate) const DEFAULT_FFI_TIMEOUT_MS: u64 = 5_000;

pub(crate) fn deadline() -> Result<Deadline, AdapterError> {
    Deadline::after(DEFAULT_FFI_TIMEOUT_MS)
}

pub(crate) fn lease(adapter: &dyn PlatformAdapter) -> Result<InteractionLease, AdapterError> {
    adapter.acquire_interaction_lease(deadline()?)
}

macro_rules! operation_deadline {
    () => {{
        match $crate::operation::deadline() {
            Ok(deadline) => deadline,
            Err(error) => {
                $crate::error::set_last_error(&error);
                return $crate::error::last_error_code();
            }
        }
    }};
}

macro_rules! interaction_lease {
    ($adapter:expr) => {{
        match $crate::operation::lease($adapter) {
            Ok(lease) => lease,
            Err(error) => {
                $crate::error::set_last_error(&error);
                return $crate::error::last_error_code();
            }
        }
    }};
}

pub(crate) use interaction_lease;
pub(crate) use operation_deadline;
