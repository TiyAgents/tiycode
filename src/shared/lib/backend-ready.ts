import { invoke } from "@tauri-apps/api/core";
import { once } from "@tauri-apps/api/event";

/**
 * Returns a promise that resolves once the Rust backend is ready to
 * handle IPC calls.
 *
 * Uses a race between three signals:
 * 1. The `backend-ready` event emitted by the Rust `on_page_load` handler.
 * 2. A lightweight invoke probe — if IPC is already working this
 *    resolves immediately (typical on macOS where WebKit is fast).
 * 3. A timeout safety net (default 3 s) for the rare case where the
 *    event was emitted before the listener registered and the probe
 *    also stalls.
 *
 * On macOS the probe wins almost instantly; on Windows the event or
 * probe will resolve once WebView2's IPC bridge is warmed up.
 */
export function waitForBackendReady(timeoutMs = 3000): Promise<void> {
  // 1. Listen for the explicit backend-ready event
  const fromEvent = new Promise<void>((resolve) => {
    once("backend-ready", () => resolve()).then((unlisten) => {
      // Event may have already fired; schedule cleanup on next tick
      queueMicrotask(() => unlisten());
    });
  });

  // 2. Probe IPC with a no-side-effect invoke.  Any response (success
  //    or error) proves the bridge is up.  We use workspace_list which
  //    is always registered and fast (~0 ms on macOS).
  const fromProbe = invoke("workspace_list")
    .then(() => {})
    .catch(() => {});

  // 3. Timeout fallback
  const fromTimeout = new Promise<void>((resolve) => {
    setTimeout(resolve, timeoutMs);
  });

  return Promise.race([fromEvent, fromProbe, fromTimeout]);
}
