export function normalizeWorkspacePath(path: string | null | undefined) {
  if (!path) {
    return "";
  }

  const slashNormalized = path.replace(/\\/g, "/");
  const isUncPath = slashNormalized.startsWith("//");
  const prefix = isUncPath ? "//" : "";
  const remainder = (isUncPath ? slashNormalized.slice(2) : slashNormalized).replace(
    /\/{2,}/g,
    "/",
  );

  let normalized = `${prefix}${remainder}`;

  if (
    normalized.length > 1
    && normalized !== "//"
    && !/^[a-zA-Z]:\/$/.test(normalized)
  ) {
    normalized = normalized.replace(/\/+$/g, "");
  }

  if (/^[a-zA-Z]:\//.test(normalized)) {
    normalized = `${normalized[0].toLowerCase()}${normalized.slice(1)}`;
  }

  return normalized;
}

export function isSameWorkspacePath(
  left: string | null | undefined,
  right: string | null | undefined,
) {
  if (!left || !right) {
    return false;
  }

  return normalizeWorkspacePath(left) === normalizeWorkspacePath(right);
}

export function buildWorkspacePathKeys(
  ...paths: Array<string | null | undefined>
) {
  return [...new Set(paths.map((path) => normalizeWorkspacePath(path)).filter(Boolean))];
}
