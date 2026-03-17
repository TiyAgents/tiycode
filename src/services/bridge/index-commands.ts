import { invoke, isTauri } from "@tauri-apps/api/core";

export type GitFileState = "tracked" | "untracked" | "ignored";

export interface FileTreeNode {
  name: string;
  path: string;
  isDir: boolean;
  isExpandable: boolean;
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

export interface SearchResult {
  path: string;
  absolutePath: string;
  lineNumber: number;
  lineText: string;
}

export interface SearchResponse {
  query: string;
  results: SearchResult[];
  count: number;
}

export async function indexGetTree(
  workspaceId: string,
): Promise<FileTreeResponse | null> {
  if (!isTauri()) return null;
  return invoke<FileTreeResponse>("index_get_tree", { workspaceId });
}

export async function indexGetChildren(
  workspaceId: string,
  directoryPath: string,
): Promise<FileTreeNode[]> {
  return invoke<FileTreeNode[]>("index_get_children", {
    workspaceId,
    directoryPath,
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

export async function indexSearch(
  workspaceId: string,
  query: string,
  filePattern?: string,
  maxResults?: number,
): Promise<SearchResponse> {
  return invoke<SearchResponse>("index_search", {
    workspaceId,
    query,
    filePattern: filePattern ?? null,
    maxResults: maxResults ?? null,
  });
}
