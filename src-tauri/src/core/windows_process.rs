#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Configure a background std command so it does not create a visible console
/// window on Windows when launched from the packaged GUI application.
#[cfg(target_os = "windows")]
pub fn configure_background_std_command(
    command: &mut std::process::Command,
) -> &mut std::process::Command {
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Configure a background tokio command so it does not create a visible console
/// window on Windows when launched from the packaged GUI application.
#[cfg(target_os = "windows")]
pub fn configure_background_tokio_command(
    command: &mut tokio::process::Command,
) -> &mut tokio::process::Command {
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
    command
}

/// No-op on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
pub fn configure_background_std_command(
    command: &mut std::process::Command,
) -> &mut std::process::Command {
    command
}

/// No-op on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
pub fn configure_background_tokio_command(
    command: &mut tokio::process::Command,
) -> &mut tokio::process::Command {
    command
}
