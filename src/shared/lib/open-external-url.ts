"use client";

import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

export async function openExternalUrl(url: string): Promise<void> {
  if (isTauri()) {
    await openUrl(url);
    return;
  }

  if (typeof window === "undefined") {
    return;
  }

  const openedWindow = window.open(url, "_blank", "noopener,noreferrer");
  if (openedWindow) {
    openedWindow.opener = null;
  } else {
    throw new Error(
      "The browser blocked the popup. Please allow popups for this site and try again.",
    );
  }
}
