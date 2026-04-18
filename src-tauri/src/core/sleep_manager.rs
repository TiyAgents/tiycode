use keepawake::{Builder, KeepAwake};
use tokio::sync::Mutex;

pub const PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY: &str = "general.prevent_sleep_while_running";

#[derive(Default)]
struct SleepManagerState {
    preference_enabled: bool,
    has_active_runs: bool,
    wake_lock: Option<KeepAwake>,
}

pub struct SleepManager {
    state: Mutex<SleepManagerState>,
}

impl SleepManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SleepManagerState::default()),
        }
    }

    pub async fn set_user_preference(&self, enabled: bool) {
        let mut state = self.state.lock().await;
        state.preference_enabled = enabled;
        refresh_wake_lock(&mut state);
    }

    pub async fn set_has_active_runs(&self, has_active_runs: bool) {
        let mut state = self.state.lock().await;
        state.has_active_runs = has_active_runs;
        refresh_wake_lock(&mut state);
    }
}

fn refresh_wake_lock(state: &mut SleepManagerState) {
    let should_hold_wake_lock = state.preference_enabled && state.has_active_runs;

    if should_hold_wake_lock {
        if state.wake_lock.is_some() {
            return;
        }

        let mut builder = Builder::default();
        builder
            .idle(true)
            .display(true)
            .reason("Active TiyCode run")
            .app_name("TiyCode")
            .app_reverse_domain("ai.tiy.tiycode");

        match builder.create() {
            Ok(lock) => {
                tracing::info!("sleep prevention enabled");
                state.wake_lock = Some(lock);
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to enable sleep prevention");
            }
        }

        return;
    }

    if state.wake_lock.take().is_some() {
        tracing::info!("sleep prevention disabled");
    }
}
