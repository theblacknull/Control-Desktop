use agent_desktop_core::{AdapterError, ProcessId};
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

/// The process-generation token: a creation-time-derived identity that
/// survives HWND recycling, mirroring macOS's
/// `"macos-proc-v1:{start_seconds}:{start_microseconds}"` shape.
///
/// Windows FILETIME is 100-nanosecond ticks since 1601; the token keeps the
/// integer second and the sub-second tick so two processes started in the same
/// second stay distinct, the same way the macOS token's microsecond field
/// does. A token is read from the process handle the identity check already
/// needs, so the identity never costs a separate enumeration.
const TOKEN_PREFIX: &str = "windows-proc-v1";
const TICKS_PER_SECOND: u64 = 10_000_000;

/// A fixed-width, nul-padded wide buffer - `PROCESSENTRY32W::szExeFile`,
/// `WIN32_FIND_DATAW::cFileName` and their kin - cut at its first nul and
/// decoded. The tail after the terminator is whatever a previous, longer
/// value left there, so reading the whole array would append those
/// leftovers to a shorter name.
pub(crate) fn wide_buffer_to_string(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    pid: ProcessId,
    creation_seconds: u64,
    creation_subtick: u64,
}

impl ProcessIdentity {
    /// Captures the process-generation identity for `pid`.
    ///
    /// `None` is the honest answer for a process whose token cannot be read
    /// (an elevated-process handle the caller cannot open, per the split-
    /// integrity measurement A16-12): the window still lists, with
    /// `process_instance: None`, and fails closed on resolution.
    #[cfg(target_os = "windows")]
    pub(crate) fn capture(pid: ProcessId) -> Result<Option<Self>, AdapterError> {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        #[cfg(test)]
        inventory_calls::record_identity_capture();

        let raw_pid = u32::from(pid);
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, raw_pid) };
        if process.is_null() {
            return Ok(None);
        }
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let read_ok =
            unsafe { GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user) };
        unsafe { CloseHandle(process) };
        if read_ok == 0 {
            return Ok(None);
        }
        let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        if ticks == 0 {
            return Ok(None);
        }
        Ok(Some(Self {
            pid,
            creation_seconds: ticks / TICKS_PER_SECOND,
            creation_subtick: ticks % TICKS_PER_SECOND,
        }))
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn capture(_pid: ProcessId) -> Result<Option<Self>, AdapterError> {
        Ok(None)
    }

    pub(crate) fn token(self) -> String {
        format!(
            "{TOKEN_PREFIX}:{}:{}",
            self.creation_seconds, self.creation_subtick
        )
    }

    fn creation_ticks(self) -> u64 {
        self.creation_seconds * TICKS_PER_SECOND + self.creation_subtick
    }

    /// Whether this identity still matches the process at `pid` right now.
    pub(crate) fn still_matches(self) -> Result<bool, AdapterError> {
        Ok(Self::capture(self.pid)?.is_some_and(|current| current == self))
    }
}

pub(crate) fn token_for_pid(pid: ProcessId) -> Result<Option<String>, AdapterError> {
    Ok(ProcessIdentity::capture(pid)?.map(ProcessIdentity::token))
}

/// One process inventory shared by every window in a single listing.
///
/// The window inventory used to read the process image table once per visible
/// window, so a desktop with `n` windows paid `n` full `Toolhelp32` walks. This
/// table walks it at most once per listing, and caches the per-pid generation
/// token so several windows of the same application share one `OpenProcess`
/// and `GetProcessTimes` pair.
///
/// The image cache is generation-checked before it is trusted. A pid can be
/// recycled between the snapshot and the lookup, so a cached name is returned
/// only when the process's creation time is no later than the snapshot's;
/// otherwise the name is read live. A failed identity capture is never cached,
/// and the strict identity re-check still reads the live process, so a
/// recycled pid cannot pass on cached evidence alone.
pub(crate) struct ProcessTable {
    images: OnceCell<ImageTable>,
    identities: RefCell<HashMap<ProcessId, ProcessIdentity>>,
}

struct ImageTable {
    captured_at_ticks: u64,
    names: HashMap<ProcessId, String>,
}

impl ProcessTable {
    pub(crate) fn new() -> Self {
        Self {
            images: OnceCell::new(),
            identities: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn identity(&self, pid: ProcessId) -> Option<ProcessIdentity> {
        if let Some(cached) = self.identities.borrow().get(&pid).copied() {
            return Some(cached);
        }
        let captured = ProcessIdentity::capture(pid).ok().flatten()?;
        self.identities.borrow_mut().insert(pid, captured);
        Some(captured)
    }

    pub(crate) fn token(&self, pid: ProcessId) -> Option<String> {
        self.identity(pid).map(ProcessIdentity::token)
    }

    pub(crate) fn image_name(&self, pid: ProcessId) -> Option<String> {
        let table = self.images.get_or_init(process_image_table);
        let Some(cached) = table.names.get(&pid) else {
            // A process that started after the table was captured is not in
            // it; read that pid directly rather than reporting an empty app.
            return process_image_name(pid);
        };
        match self.identity(pid) {
            // The process was alive when the snapshot was taken, so the cached
            // name belongs to the same pid generation.
            Some(identity) if identity.creation_ticks() <= table.captured_at_ticks => {
                Some(cached.clone())
            }
            // The pid was recycled after the snapshot, or its creation time is
            // unreadable; read the live name instead of attributing the
            // predecessor's image to it.
            _ => process_image_name(pid),
        }
    }

    /// The token and image name one window's identity needs, read from the
    /// shared table.
    pub(crate) fn facts(&self, pid: ProcessId) -> (Option<String>, String) {
        (self.token(pid), self.image_name(pid).unwrap_or_default())
    }

    #[cfg(test)]
    pub(crate) fn cached_identity_count(&self) -> usize {
        self.identities.borrow().len()
    }
}

/// Every process image name the table can see, in one walk.
#[cfg(target_os = "windows")]
fn process_image_table() -> ImageTable {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    #[cfg(test)]
    inventory_calls::record_image_table_walk();

    let captured_at_ticks = filetime_ticks_now();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return ImageTable {
            captured_at_ticks,
            names: HashMap::new(),
        };
    }
    let mut names = HashMap::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        names.insert(
            ProcessId::from(entry.th32ProcessID),
            wide_buffer_to_string(&entry.szExeFile),
        );
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(snapshot);
    }
    ImageTable {
        captured_at_ticks,
        names,
    }
}

#[cfg(not(target_os = "windows"))]
fn process_image_table() -> ImageTable {
    ImageTable {
        captured_at_ticks: 0,
        names: HashMap::new(),
    }
}

/// The current wall clock in Windows FILETIME ticks, the same epoch and unit
/// as the `GetProcessTimes` creation time, so a pid generation can be compared
/// to the snapshot instant.
#[cfg(target_os = "windows")]
fn filetime_ticks_now() -> u64 {
    const WINDOWS_EPOCH_OFFSET_SECONDS: u64 = 11_644_473_600;
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (since_epoch.as_secs() + WINDOWS_EPOCH_OFFSET_SECONDS) * TICKS_PER_SECOND
        + u64::from(since_epoch.subsec_nanos() / 100)
}

#[cfg(not(target_os = "windows"))]
fn filetime_ticks_now() -> u64 {
    0
}

/// Test-visible counts for the two process reads, so a regression that walks
/// the whole process table per window fails a test rather than only a
/// benchmark.
#[cfg(test)]
#[path = "process_identity_inventory_calls.rs"]
pub(crate) mod inventory_calls;

#[cfg(target_os = "windows")]
pub(crate) fn process_is_running(pid: ProcessId) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
    let process = unsafe { OpenProcess(access, 0, u32::from(pid)) };
    if process.is_null() {
        return true;
    }
    let wait = unsafe { WaitForSingleObject(process, 0) };
    unsafe { CloseHandle(process) };
    wait != WAIT_OBJECT_0
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn process_is_running(_pid: ProcessId) -> bool {
    true
}

/// Blocks on the process object itself for at most `window` and reports
/// whether the process exited inside it.
///
/// `process_is_running` is a zero-timeout poll, so a process that is tearing
/// down but not yet signaled reads as running. This waits on the handle, an
/// event rather than a sleep, so the exit is observed the instant it happens.
/// A pid that no longer names a process (`ERROR_INVALID_PARAMETER`) has
/// exited; any other failure to open the handle cannot confirm an exit and
/// reports `false` without waiting.
#[cfg(target_os = "windows")]
pub(crate) fn wait_for_exit(pid: ProcessId, window: std::time::Duration) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
    let process = unsafe { OpenProcess(access, 0, u32::from(pid)) };
    if process.is_null() {
        return unsafe { GetLastError() } == ERROR_INVALID_PARAMETER;
    }
    let millis = u32::try_from(window.as_millis()).unwrap_or(u32::MAX);
    let wait = unsafe { WaitForSingleObject(process, millis) };
    unsafe { CloseHandle(process) };
    wait == WAIT_OBJECT_0
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn wait_for_exit(_pid: ProcessId, _window: std::time::Duration) -> bool {
    false
}

/// The image name (executable filename) of the process at `pid`, read from
/// the ToolHelp process table.
///
/// `CreateToolhelp32Snapshot` reports failure as `INVALID_HANDLE_VALUE`
/// (`-1`), never as a null handle, so an `is_null` guard can never fire and
/// a failed call would read as an empty enumeration rather than an error.
///
/// The single walk both window identity checks corroborate `app` against:
/// `window_ops.rs` and `window_identity.rs` each carried their own verbatim
/// copy of this loop before it was pulled out from underneath them.
#[cfg(target_os = "windows")]
pub(crate) fn process_image_name(pid: ProcessId) -> Option<String> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    #[cfg(test)]
    inventory_calls::record_single_image_walk(pid);

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = None;
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        if entry.th32ProcessID == u32::from(pid) {
            found = Some(wide_buffer_to_string(&entry.szExeFile));
            break;
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(snapshot);
    }
    found
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn process_image_name(_pid: ProcessId) -> Option<String> {
    None
}

/// Verifies a stored token against the process's current generation.
///
/// A recycled PID whose process is a different generation fails closed; a PID
/// whose process is gone altogether reads `None` and also fails. This is the
/// check a recycled HWND on a different process generation trips.
pub(crate) fn matches_instance(pid: ProcessId, token: &str) -> Result<bool, AdapterError> {
    let expected = match parse_token(pid, token)? {
        Some(identity) => identity,
        None => return Ok(false),
    };
    expected.still_matches()
}

/// Parses a token back into a comparable identity, or `None` for an
/// unrecognised shape - which must not match anything.
fn parse_token(pid: ProcessId, token: &str) -> Result<Option<ProcessIdentity>, AdapterError> {
    let mut parts = token.split(':');
    if parts.next() != Some(TOKEN_PREFIX) {
        return Ok(None);
    }
    let (seconds, subtick) = match (parts.next(), parts.next()) {
        (Some(seconds), Some(subtick)) => match (seconds.parse::<u64>(), subtick.parse::<u64>()) {
            (Ok(seconds), Ok(subtick)) => (seconds, subtick),
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(Some(ProcessIdentity {
        pid,
        creation_seconds: seconds,
        creation_subtick: subtick,
    }))
}

#[cfg(test)]
#[path = "process_identity_tests.rs"]
mod tests;
