/**
 * Re-export the ThreadStreamEvent type for consumers.
 *
 * This file exists so thread-stream consumers can import from
 * `@/services/thread-stream` without knowing about the api types file.
 */
export type { ThreadStreamEvent } from "@/shared/types/api";
