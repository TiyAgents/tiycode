import { useCallback, useRef, useState } from "react";

import { invoke, isTauri } from "@tauri-apps/api/core";

import type { InstallSource } from "@/shared/types/system";

type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "brewInstalled"
  | "downloading"
  | "readyToRestart"
  | "upToDate"
  | "error";

export interface UpdateInfo {
  version: string;
  body: string | null;
  date: string | null;
  currentVersion: string;
}

export interface AppUpdater {
  phase: UpdatePhase;
  updateInfo: UpdateInfo | null;
  downloadProgress: number;
  errorMessage: string | null;
  installSource: InstallSource;
  checkForUpdates: () => void;
  downloadAndInstall: () => void;
  restartApp: () => void;
  dismiss: () => void;
}

/**
 * Errors that indicate the update manifest couldn't be fetched.
 * These are expected when no release exists yet, the endpoint is unreachable,
 * or there's a network issue — treated as "up to date" rather than surfacing
 * an error dialog.
 */
const MANIFEST_FETCH_PATTERNS = [
  "could not fetch a valid release json",
  "network error",
  "failed to fetch",
  "status code: 404",
  "status code: 403",
  "timed out",
  "dns error",
  "connection refused",
];

function isManifestFetchError(message: string): boolean {
  const lower = message.toLowerCase();
  return MANIFEST_FETCH_PATTERNS.some((pattern) => lower.includes(pattern));
}

export function useAppUpdater(): AppUpdater {
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [installSource, setInstallSource] = useState<InstallSource>("standalone");

  // Hold the Update instance across renders so downloadAndInstall can use it
  const updateRef = useRef<Awaited<
    ReturnType<typeof import("@tauri-apps/plugin-updater").check>
  > | null>(null);

  const checkForUpdates = useCallback(() => {
    if (phase === "checking" || phase === "downloading") {
      return;
    }

    setPhase("checking");
    setErrorMessage(null);
    setUpdateInfo(null);
    setDownloadProgress(0);

    (async () => {
      try {
        if (!isTauri()) {
          // Web dev mode — graceful fallback
          await new Promise((resolve) => setTimeout(resolve, 600));
          setPhase("upToDate");
          return;
        }

        // Detect install source (Homebrew vs standalone)
        const isHomebrew = await invoke<boolean>("is_homebrew_installed").catch(
          () => false,
        );
        const source: InstallSource = isHomebrew ? "homebrew" : "standalone";
        setInstallSource(source);

        const { check } = await import("@tauri-apps/plugin-updater");
        const update = await check({ timeout: 15_000 });

        if (!update) {
          setPhase("upToDate");
          return;
        }

        updateRef.current = update;
        setUpdateInfo({
          version: update.version,
          body: update.body ?? null,
          date: update.date ?? null,
          currentVersion: update.currentVersion,
        });

        // If installed via Homebrew, show brew upgrade hint instead of in-app download
        if (source === "homebrew") {
          setPhase("brewInstalled");
        } else {
          setPhase("available");
        }
      } catch (error) {
        const message =
          error instanceof Error ? error.message : String(error);

        // Treat "can't fetch manifest" errors as "up to date" —
        // this covers 404 (no release yet), network errors, etc.
        // Only show the error dialog for truly unexpected failures.
        const isCheckError = isManifestFetchError(message);
        if (isCheckError) {
          setPhase("upToDate");
        } else {
          setErrorMessage(message);
          setPhase("error");
        }
      }
    })();
  }, [phase]);

  const downloadAndInstall = useCallback(() => {
    const update = updateRef.current;
    if (!update || phase !== "available") {
      return;
    }

    setPhase("downloading");
    setDownloadProgress(0);

    (async () => {
      try {
        let contentLength = 0;
        let downloaded = 0;

        await update.downloadAndInstall((event) => {
          switch (event.event) {
            case "Started":
              contentLength = event.data.contentLength ?? 0;
              break;
            case "Progress":
              downloaded += event.data.chunkLength;
              if (contentLength > 0) {
                setDownloadProgress(
                  Math.min(
                    100,
                    Math.round((downloaded / contentLength) * 100),
                  ),
                );
              }
              break;
            case "Finished":
              setDownloadProgress(100);
              break;
          }
        });

        setPhase("readyToRestart");
      } catch (error) {
        const message =
          error instanceof Error ? error.message : String(error);
        setErrorMessage(message);
        setPhase("error");
      }
    })();
  }, [phase]);

  const restartApp = useCallback(() => {
    (async () => {
      try {
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      } catch (error) {
        const message =
          error instanceof Error ? error.message : String(error);
        setErrorMessage(message);
        setPhase("error");
      }
    })();
  }, []);

  const dismiss = useCallback(() => {
    // Release the Update resource if we're not going to use it
    if (
      updateRef.current &&
      phase !== "downloading" &&
      phase !== "readyToRestart"
    ) {
      updateRef.current.close().catch(() => {});
      updateRef.current = null;
    }

    setPhase("idle");
    setErrorMessage(null);
    setDownloadProgress(0);
  }, [phase]);

  return {
    phase,
    updateInfo,
    downloadProgress,
    errorMessage,
    installSource,
    checkForUpdates,
    downloadAndInstall,
    restartApp,
    dismiss,
  };
}
