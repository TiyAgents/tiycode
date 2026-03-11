import { invoke } from "@tauri-apps/api/core";
import type { SystemMetadata } from "@/shared/types/system";

export async function getSystemMetadata() {
  return invoke<SystemMetadata>("get_system_metadata");
}
