use std::sync::atomic::{AtomicBool, Ordering};

pub const LAUNCH_AT_LOGIN_SETTING_KEY: &str = "general.launch_at_login";
pub const MINIMIZE_TO_TRAY_SETTING_KEY: &str = "general.minimize_to_tray";

#[derive(Default)]
pub struct DesktopRuntimeState {
    minimize_to_tray: AtomicBool,
    is_quitting: AtomicBool,
}

impl DesktopRuntimeState {
    pub fn set_minimize_to_tray(&self, enabled: bool) {
        self.minimize_to_tray.store(enabled, Ordering::Relaxed);
    }

    pub fn minimize_to_tray_enabled(&self) -> bool {
        self.minimize_to_tray.load(Ordering::Relaxed)
    }

    pub fn mark_quitting(&self) {
        self.is_quitting.store(true, Ordering::Relaxed);
    }

    pub fn is_quitting(&self) -> bool {
        self.is_quitting.load(Ordering::Relaxed)
    }
}
