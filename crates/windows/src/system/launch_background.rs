//! How a launch without `--activate` stays behind the user's work.
//!
//! Windows lets a process started from the foreground process take the
//! foreground itself. An agent runs under the user's terminal, so every app
//! it launched inherited that right: it raised its first window over the
//! terminal and took the keyboard. `SW_SHOWNOACTIVATE` in the startup
//! information is not enough on its own, because an app that calls
//! `SetForegroundWindow` while it starts is still allowed to (measured with
//! `winver`: foreground for the whole sample). Three pieces together keep the
//! launch out of the way:
//!
//! - the child is created with the desktop shell as its parent through
//!   `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`, so it does not inherit the
//!   caller's right to take the foreground;
//! - its first `ShowWindow` is `SW_SHOWNOACTIVATE`;
//! - once its window is observed, the window is placed at the bottom of the
//!   z-order without being activated, so it does not cover the user's
//!   windows. It stays visible and restored, which keeps it drivable.
//!
//! Sending the window back without the shell parent was measured to leave a
//! window behind everything that still held the keyboard focus, so the order
//! is only correct as a whole. `--activate` asks for the app to come forward
//! and keeps the ordinary parent and show command.

use agent_desktop_core::{AdapterError, ErrorCode, WindowInfo};

use super::super::runtime::hresult::win32_last_error;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Threading::{
    DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    OpenProcess, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, PROCESS_CREATE_PROCESS,
    STARTF_USESHOWWINDOW, STARTUPINFOW, UpdateProcThreadAttribute,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GetWindowThreadProcessId, HWND_BOTTOM, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
};

const ACTIVATE_INSTEAD: &str = "Retry with --activate to launch it in the foreground.";

/// The desktop shell process, held open as the parent of a background launch.
///
/// `UpdateProcThreadAttribute` stores the address of the handle, not its
/// value, and `CreateProcessW` reads that address later. The handle is boxed
/// so the address survives this value being moved out of `open`; an inline
/// field moves with it and leaves the list naming stale memory (measured:
/// `ERROR_INVALID_HANDLE` from `CreateProcessW`). The list buffer is
/// `u64`-backed for pointer alignment.
///
/// There is no fallback to the caller as parent. A shell that cannot be opened
/// fails the launch before anything starts, rather than starting the app with
/// the right to take the foreground the caller did not ask for.
pub(super) struct ShellParent {
    process: Box<HANDLE>,
    list: Vec<u64>,
}

impl ShellParent {
    pub(super) fn open() -> Result<Self, AdapterError> {
        let shell = unsafe { GetShellWindow() };
        let mut shell_pid = 0u32;
        if shell.is_null()
            || unsafe { GetWindowThreadProcessId(shell, &mut shell_pid) } == 0
            || shell_pid == 0
        {
            return Err(AdapterError::new(
                ErrorCode::ActionFailed,
                "No desktop shell process to launch the application under in the background",
            )
            .with_suggestion(ACTIVATE_INSTEAD));
        }
        let process = unsafe { OpenProcess(PROCESS_CREATE_PROCESS, 0, shell_pid) };
        if process.is_null() {
            return Err(win32_last_error(
                "OpenProcess on the desktop shell failed, so the application cannot launch in the background",
            )
            .with_suggestion(ACTIVATE_INSTEAD));
        }
        let mut parent = Self {
            process: Box::new(process),
            list: Vec::new(),
        };
        let mut size = 0usize;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
        parent.list = vec![0u64; size.div_ceil(8)];
        if unsafe { InitializeProcThreadAttributeList(parent.attribute_list(), 1, 0, &mut size) }
            == 0
        {
            parent.list.clear();
            return Err(win32_last_error("InitializeProcThreadAttributeList failed"));
        }
        let updated = unsafe {
            UpdateProcThreadAttribute(
                parent.attribute_list(),
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                (&*parent.process as *const HANDLE).cast(),
                std::mem::size_of::<HANDLE>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if updated == 0 {
            return Err(win32_last_error(
                "UpdateProcThreadAttribute(PARENT_PROCESS) failed",
            ));
        }
        Ok(parent)
    }

    pub(super) fn attribute_list(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.list.as_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST
    }
}

impl Drop for ShellParent {
    fn drop(&mut self) {
        unsafe {
            if !self.list.is_empty() {
                DeleteProcThreadAttributeList(self.attribute_list());
            }
            CloseHandle(*self.process);
        }
    }
}

/// The startup information a launch hands to `CreateProcessW`. A background
/// launch replaces the child's first `SW_SHOWDEFAULT` with
/// `SW_SHOWNOACTIVATE`; an activating one leaves the show command to the app.
pub(super) fn startup_info(activate: bool) -> STARTUPINFOW {
    let mut startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    if !activate {
        startup.dwFlags = STARTF_USESHOWWINDOW;
        startup.wShowWindow = SW_SHOWNOACTIVATE as u16;
    }
    startup
}

/// Places a background launch's window at the bottom of the z-order without
/// activating it. The process is already running, so the caller reports a
/// failure here as delivered rather than skipping it.
pub(super) fn send_to_back(window: &WindowInfo) -> Result<(), AdapterError> {
    let handle = super::super::window::window_ops::parse_handle(&window.id);
    if handle.is_null() {
        return Err(AdapterError::new(
            ErrorCode::Internal,
            "The launched window id does not name a window handle, so it cannot be placed behind other windows",
        ));
    }
    let placed = unsafe {
        SetWindowPos(
            handle,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
    };
    if placed == 0 {
        return Err(win32_last_error(
            "SetWindowPos(HWND_BOTTOM) failed for the launched window",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "launch_background_tests.rs"]
mod tests;
