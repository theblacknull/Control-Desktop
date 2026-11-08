use super::*;
use agent_desktop_core::{Deadline, system::launch_options::LaunchOptions};

#[test]
fn a_background_launch_shows_its_window_without_activation() {
    let startup = startup_info(false);
    assert_eq!(startup.dwFlags & STARTF_USESHOWWINDOW, STARTF_USESHOWWINDOW);
    assert_eq!(startup.wShowWindow, SW_SHOWNOACTIVATE as u16);
}

#[test]
fn an_activating_launch_leaves_the_show_command_to_the_app() {
    let startup = startup_info(true);
    assert_eq!(startup.dwFlags & STARTF_USESHOWWINDOW, 0);
}

#[test]
fn the_desktop_shell_opens_as_a_launch_parent() {
    let parent = ShellParent::open().expect("the desktop shell opens as a launch parent");
    assert!(!parent.process.is_null());
    assert!(!parent.list.is_empty());
}

/// The attribute list names the handle by address, so it has to stay valid
/// after `ShellParent` moves out of `open` and into the launch; a launch
/// through it then starts. A console program is the sharper case, because
/// it starts only when the launch gives it a console of its own.
#[test]
fn a_background_launch_of_a_console_program_starts_under_the_desktop_shell() {
    let dir =
        std::env::temp_dir().join(format!("agent-desktop-shell-parent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let probe = dir.join("launchprobe_shell_parent.exe");
    std::fs::copy(r"C:\Windows\System32\cmd.exe", &probe).expect("copy cmd");
    let options = LaunchOptions {
        args: vec!["/C".into(), "exit 0".into()],
        attach_if_running: false,
        timeout_ms: 0,
        ..Default::default()
    };

    let result = super::super::launch_app_impl(
        probe.to_str().expect("utf8 path"),
        &options,
        Deadline::after(10_000).expect("deadline"),
    )
    .expect("a background launch through the shell parent starts");

    assert!(u32::from(result.pid) > 0);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(&dir);
}
