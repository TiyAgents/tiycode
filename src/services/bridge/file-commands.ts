import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface FileContentDto {
  content: string;
  sizeBytes: number;
  isBinary: boolean;
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/**
 * Read file content from a workspace.
 * Returns base64 data URI for images, empty content for binary files,
 * and UTF-8 text for everything else. Max 5 MB.
 */
export async function fileRead(
  workspaceId: string,
  path: string,
): Promise<FileContentDto> {
  return invoke<FileContentDto>("file_read", {
    workspaceId,
    path,
  });
}

/** Write (overwrite) file content. */
export async function fileWrite(
  workspaceId: string,
  path: string,
  content: string,
): Promise<void> {
  return invoke<void>("file_write", {
    workspaceId,
    path,
    content,
  });
}

/** Create a new file or directory. */
export async function fileCreate(
  workspaceId: string,
  parentPath: string,
  name: string,
  isDir: boolean,
): Promise<void> {
  return invoke<void>("file_create", {
    workspaceId,
    parentPath,
    name,
    isDir,
  });
}

/** Delete a file or directory (recursive for directories). */
export async function fileDelete(
  workspaceId: string,
  path: string,
): Promise<void> {
  return invoke<void>("file_delete", {
    workspaceId,
    path,
  });
}

/** Rename a file or directory (same parent directory). */
export async function fileRename(
  workspaceId: string,
  oldPath: string,
  newName: string,
): Promise<void> {
  return invoke<void>("file_rename", {
    workspaceId,
    oldPath,
    newName,
  });
}
