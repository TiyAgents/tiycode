import { invoke, isTauri } from "@tauri-apps/api/core";
import type { SystemMetadata } from "@/shared/types/system";

export async function getSystemMetadata() {
  if (!isTauri()) {
    throw new Error("Desktop runtime info is only available inside the Tauri app.");
  }

  return invoke<SystemMetadata>("get_system_metadata");
}
