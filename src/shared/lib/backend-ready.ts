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
 *
 * All three paths share a `settled` flag so that whichever wins first
 * cleans up the other two (event listener + timer), preventing leaks.
 */
export function waitForBackendReady(timeoutMs = 3000): Promise<void> {
  let settled = false;
  let eventUnlisten: (() => void) | null = null;
  let timerId: ReturnType<typeof setTimeout> | null = null;

  const settle = () => {
    if (settled) return;
    settled = true;
    if (eventUnlisten) {
      eventUnlisten();
      eventUnlisten = null;
    }
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
  };

  return new Promise<void>((resolve) => {
    const done = () => {
      settle();
      resolve();
    };

    // 1. Listen for the explicit backend-ready event
    once("backend-ready", () => {
      if (!settled) done();
    }).then((unlisten) => {
      // If another signal already won the race, clean up immediately
      if (settled) unlisten();
      else eventUnlisten = unlisten;
    });

    // 2. Probe IPC with a no-side-effect invoke.  A successful response
    //    proves the bridge is up.  Rejections are ignored — if the bridge
    //    is not ready the invoke will hang rather than reject, and a real
    //    rejection (e.g. command error) still means the bridge is up, but
    //    we let the event or timeout handle that path to stay safe.
    invoke("workspace_list")
      .then(() => {
        if (!settled) done();
      })
      .catch(() => {
        // Intentionally ignored — rely on event or timeout.
      });

    // 3. Timeout fallback
    timerId = setTimeout(() => {
      if (!settled) done();
    }, timeoutMs);
  });
}
