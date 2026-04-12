function normalizePathSegments(
  segments: ReadonlyArray<string>,
  clampAboveRoot: boolean,
) {
  const normalized: string[] = [];

  for (const segment of segments) {
    if (!segment || segment === ".") {
      continue;
    }

    if (segment === "..") {
      if (normalized.length > 0) {
        normalized.pop();
      } else if (!clampAboveRoot) {
        normalized.push("..");
      }
      continue;
    }

    normalized.push(segment);
  }

  return normalized;
}

function normalizeUncWorkspacePath(path: string) {
  const segments = path.replace(/^\/+/, "").split("/").filter(Boolean);

  if (segments.length === 0) {
    return "//";
  }

  if (segments.length === 1) {
    return `//${segments[0]}`;
  }

  const [server, share, ...remainder] = segments;
  const normalizedRemainder = normalizePathSegments(remainder, true);

  return `//${server}/${share}${normalizedRemainder.length > 0 ? `/${normalizedRemainder.join("/")}` : ""}`;
}

function normalizeDriveWorkspacePath(path: string, driveLetter: string) {
  const remainder = path.slice(2).replace(/\/{2,}/g, "/");
  const isAbsolute = remainder.startsWith("/");
  const normalizedSegments = normalizePathSegments(
    remainder.split("/").filter(Boolean),
    isAbsolute,
  );
  let normalized = isAbsolute ? `${driveLetter}:/` : `${driveLetter}:`;

  if (normalizedSegments.length > 0) {
    normalized += normalizedSegments.join("/");
  }

  return normalized;
}

function normalizePosixWorkspacePath(path: string) {
  const collapsed = path.replace(/\/{2,}/g, "/");
  const isAbsolute = collapsed.startsWith("/");
  const normalizedSegments = normalizePathSegments(
    collapsed.split("/").filter(Boolean),
    isAbsolute,
  );

  if (normalizedSegments.length === 0) {
    return isAbsolute ? "/" : ".";
  }

  return `${isAbsolute ? "/" : ""}${normalizedSegments.join("/")}`;
}

export function normalizeWorkspacePath(path: string | null | undefined) {
  if (!path) {
    return "";
  }

  const slashNormalized = path.replace(/\\/g, "/");
  if (slashNormalized.startsWith("//")) {
    return normalizeUncWorkspacePath(slashNormalized);
  }

  const driveMatch = slashNormalized.match(/^([a-zA-Z]):(?:\/|$)/);
  if (driveMatch) {
    return normalizeDriveWorkspacePath(
      slashNormalized,
      driveMatch[1].toLowerCase(),
    );
  }

  return normalizePosixWorkspacePath(slashNormalized);
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
