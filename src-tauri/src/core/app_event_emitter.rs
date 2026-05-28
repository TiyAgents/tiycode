use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Object-safe abstraction over frontend/global app events.
///
/// Desktop mode forwards events to Tauri windows. Headless ACP stdio mode uses
/// the no-op implementation so the core runtime can be reused without a Tauri
/// `AppHandle`.
pub trait AppEventEmitter: Send + Sync {
    fn emit_value(&self, event: &'static str, payload: serde_json::Value);
}

pub type AppEventEmitterRef = Arc<dyn AppEventEmitter>;

#[derive(Debug, Default)]
pub struct NoopAppEventEmitter;

impl AppEventEmitter for NoopAppEventEmitter {
    fn emit_value(&self, _event: &'static str, _payload: serde_json::Value) {
        // Headless ACP stdio mode has no frontend windows. These app events are
        // UI notifications only; runtime cleanup and persistence must not rely
        // on this emitter being observed.
    }
}

#[derive(Clone)]
pub struct TauriAppEventEmitter {
    app_handle: AppHandle,
}

impl TauriAppEventEmitter {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl AppEventEmitter for TauriAppEventEmitter {
    fn emit_value(&self, event: &'static str, payload: serde_json::Value) {
        let _ = self.app_handle.emit(event, payload);
    }
}

pub fn emit_app_event<T>(emitter: &dyn AppEventEmitter, event: &'static str, payload: T)
where
    T: Serialize,
{
    match serde_json::to_value(payload) {
        Ok(value) => emitter.emit_value(event, value),
        Err(error) => {
            tracing::warn!(event, error = %error, "failed to serialize app event payload")
        }
    }
}
