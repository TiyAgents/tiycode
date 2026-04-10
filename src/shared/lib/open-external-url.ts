"use client";

import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

const ALLOWED_PROTOCOLS = new Set(["http:", "https:"]);

function assertSafeUrl(raw: string): URL {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    throw new Error(`Invalid URL: ${raw}`);
  }

  if (!ALLOWED_PROTOCOLS.has(parsed.protocol)) {
    throw new Error(
      `Blocked URL with disallowed protocol "${parsed.protocol}". Only http and https links are allowed.`,
    );
  }

  return parsed;
}

export async function openExternalUrl(url: string): Promise<void> {
  const safeUrl = assertSafeUrl(url);

  if (isTauri()) {
    await openUrl(safeUrl.href);
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
