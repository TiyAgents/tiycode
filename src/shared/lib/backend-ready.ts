import { once } from "@tauri-apps/api/event";

/**
 * Returns a promise that resolves once the Rust backend has signalled
 * readiness via the `backend-ready` event.
 *
 * On Windows the WebView2 IPC bridge may need a few seconds after
 * `on_page_load(Finished)` before it can reliably deliver `invoke`
 * responses.  By waiting for this event (which the Rust side pushes
 * via `webview.emit`), the frontend avoids queuing heavy IPC batches
 * into a channel that isn't fully warmed up yet.
 *
 * A 3-second timeout acts as a safety net in case the event was
 * already emitted before the listener was registered (race with
 * `on_page_load`).
 */
export function waitForBackendReady(): Promise<void> {
  return new Promise<void>((resolve) => {
    let resolved = false;
    const done = () => {
      if (!resolved) {
        resolved = true;
        resolve();
      }
    };
    once("backend-ready", () => done()).then((unlisten) => {
      // If already resolved via timeout, clean up the listener
      if (resolved) unlisten();
    });
    // Safety fallback: don't block forever if the event was already
    // emitted before the listener was registered.
    setTimeout(done, 3000);
  });
}
