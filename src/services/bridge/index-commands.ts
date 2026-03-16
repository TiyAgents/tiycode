import { invoke, isTauri } from "@tauri-apps/api/core";

export interface FileTreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children?: FileTreeNode[];
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
): Promise<FileTreeNode | null> {
  if (!isTauri()) return null;
  return invoke<FileTreeNode>("index_get_tree", { workspaceId });
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
