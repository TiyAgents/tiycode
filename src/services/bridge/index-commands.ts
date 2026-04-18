import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { GitFileState } from "@/shared/types/api";

export interface FileTreeNode {
  name: string;
  path: string;
  isDir: boolean;
  isExpandable: boolean;
  childrenHasMore: boolean;
  childrenNextOffset?: number;
  gitState?: GitFileState;
  children?: FileTreeNode[];
}

export interface FileTreeResponse {
  repoAvailable: boolean;
  tree: FileTreeNode;
}

export interface FileFilterMatch {
  name: string;
  path: string;
  parentPath: string;
}

export interface FileFilterResponse {
  query: string;
  results: FileFilterMatch[];
  count: number;
}

export interface DirectoryChildrenResponse {
  children: FileTreeNode[];
  hasMore: boolean;
  nextOffset?: number;
}

export interface RevealPathSegment {
  directoryPath: string;
  children: FileTreeNode[];
  hasMore: boolean;
  nextOffset?: number;
}

export interface RevealPathResponse {
  targetPath: string;
  segments: RevealPathSegment[];
}

export interface IndexGitOverlayReadyPayload {
  workspaceId: string;
  repoAvailable: boolean;
  states: Record<string, GitFileState>;
}

export interface SearchResult {
  path: string;
  absolutePath: string;
  lineNumber: number;
  endLineNumber?: number;
  lineText: string;
  matchText?: string;
}

export interface SearchFileMatch {
  path: string;
  absolutePath: string;
}

export interface SearchFileCount {
  path: string;
  absolutePath: string;
  count: number;
}

export type SearchQueryMode = "literal" | "regex";
export type SearchOutputMode = "content" | "files_with_matches" | "count";

export interface SearchResponse {
  query: string;
  queryMode: SearchQueryMode;
  outputMode: SearchOutputMode;
  results: SearchResult[];
  files: SearchFileMatch[];
  fileCounts: SearchFileCount[];
  count: number;
  totalCount: number;
  totalFiles: number;
  completed: boolean;
  cancelled: boolean;
  timedOut: boolean;
  partial: boolean;
  elapsedMs: number;
  searchedFiles: number;
}

export interface SearchBatchResponse {
  query: string;
  outputMode: SearchOutputMode;
  results: SearchResult[];
  files: SearchFileMatch[];
  fileCounts: SearchFileCount[];
  count: number;
  totalCount: number;
  totalFiles: number;
  searchedFiles: number;
}

export type SearchStreamEvent =
  | { type: "started"; workspaceId: string; query: string }
  | { type: "batch"; workspaceId: string; batch: SearchBatchResponse }
  | { type: "completed"; workspaceId: string; response: SearchResponse }
  | { type: "failed"; workspaceId: string; query: string; error: string };

export interface IndexSearchOptions {
  filePattern?: string;
  fileType?: string;
  maxResults?: number;
  queryMode?: SearchQueryMode;
  outputMode?: SearchOutputMode;
  caseInsensitive?: boolean;
  multiline?: boolean;
  timeoutMs?: number;
}

export type IndexSearchStreamCancel = () => Promise<void>;

const activeSearchStreams = new Map<string, number>();

export async function indexGetTree(
  workspaceId: string,
): Promise<FileTreeResponse | null> {
  if (!isTauri()) return null;
  return invoke<FileTreeResponse>("index_get_tree", { workspaceId });
}

export async function indexGetChildren(
  workspaceId: string,
  directoryPath: string,
  offset?: number,
  maxResults?: number,
): Promise<DirectoryChildrenResponse> {
  return invoke<DirectoryChildrenResponse>("index_get_children", {
    workspaceId,
    directoryPath,
    offset: offset ?? null,
    maxResults: maxResults ?? null,
  });
}

export async function indexFilterFiles(
  workspaceId: string,
  query: string,
  maxResults?: number,
): Promise<FileFilterResponse> {
  return invoke<FileFilterResponse>("index_filter_files", {
    workspaceId,
    query,
    maxResults: maxResults ?? null,
  });
}

export async function indexRevealPath(
  workspaceId: string,
  targetPath: string,
): Promise<RevealPathResponse> {
  return invoke<RevealPathResponse>("index_reveal_path", {
    workspaceId,
    targetPath,
  });
}

export async function indexSearch(
  workspaceId: string,
  query: string,
  options?: IndexSearchOptions,
): Promise<SearchResponse> {
  return invoke<SearchResponse>("index_search", {
    workspaceId,
    query,
    filePattern: options?.filePattern ?? null,
    fileType: options?.fileType ?? null,
    maxResults: options?.maxResults ?? null,
    queryMode: options?.queryMode ?? null,
    outputMode: options?.outputMode ?? null,
    caseInsensitive: options?.caseInsensitive ?? null,
    multiline: options?.multiline ?? null,
    timeoutMs: options?.timeoutMs ?? null,
  });
}

export async function indexSearchStream(
  workspaceId: string,
  query: string,
  onEvent: (event: SearchStreamEvent) => void,
  options?: IndexSearchOptions,
): Promise<IndexSearchStreamCancel> {
  if (!isTauri()) return async () => {};

  const channel = new Channel<SearchStreamEvent>();
  const previousSearchId = activeSearchStreams.get(workspaceId);
  if (previousSearchId !== undefined && previousSearchId !== channel.id) {
    await invoke("index_cancel_search_stream", {
      searchId: previousSearchId,
    });
  }
  activeSearchStreams.set(workspaceId, channel.id);

  const clearIfCurrent = () => {
    if (activeSearchStreams.get(workspaceId) === channel.id) {
      activeSearchStreams.delete(workspaceId);
    }
  };

  channel.onmessage = (event) => {
    if (activeSearchStreams.get(workspaceId) !== channel.id) {
      return;
    }

    onEvent(event);

    if (event.type === "completed" || event.type === "failed") {
      clearIfCurrent();
    }
  };

  try {
    await invoke("index_search_stream", {
      workspaceId,
      query,
      filePattern: options?.filePattern ?? null,
      fileType: options?.fileType ?? null,
      maxResults: options?.maxResults ?? null,
      queryMode: options?.queryMode ?? null,
      outputMode: options?.outputMode ?? null,
      caseInsensitive: options?.caseInsensitive ?? null,
      multiline: options?.multiline ?? null,
      timeoutMs: options?.timeoutMs ?? null,
      onEvent: channel,
    });
  } catch (error) {
    clearIfCurrent();
    throw error;
  }

  return async () => {
    clearIfCurrent();
    await invoke("index_cancel_search_stream", {
      searchId: channel.id,
    });
  };
}
