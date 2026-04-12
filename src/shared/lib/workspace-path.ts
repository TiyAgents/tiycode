function normalizePathSegments(
  segments: ReadonlyArray<string>,
  clampAboveRoot: boolean,
  protectedSegmentCount = 0,
) {
  const normalized: string[] = [];

  for (const segment of segments) {
    if (!segment || segment === ".") {
      continue;
    }

    if (segment === "..") {
      const lastSegment = normalized[normalized.length - 1];
      if (
        lastSegment &&
        lastSegment !== ".." &&
        normalized.length > protectedSegmentCount
      ) {
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

function normalizeWindowsSegments(segments: ReadonlyArray<string>) {
  return segments.map((segment) =>
    segment === "." || segment === ".." ? segment : segment.toLowerCase(),
  );
}

function normalizeUncWorkspacePath(path: string) {
  const segments = normalizeWindowsSegments(
    path.replace(/^\/+/, "").split("/").filter(Boolean),
  );
  const normalizedSegments = normalizePathSegments(
    segments,
    true,
    Math.min(2, segments.length),
  );

  if (normalizedSegments.length === 0) {
    return "//";
  }

  if (normalizedSegments.length === 1) {
    return `//${normalizedSegments[0]}`;
  }

  const [server, share, ...remainder] = normalizedSegments;

  return `//${server}/${share}${remainder.length > 0 ? `/${remainder.join("/")}` : ""}`;
}

function normalizeDriveWorkspacePath(path: string, driveLetter: string) {
  const remainder = path.slice(2).replace(/\/{2,}/g, "/");
  const isAbsolute = remainder.startsWith("/");
  const normalizedSegments = normalizePathSegments(
    normalizeWindowsSegments(remainder.split("/").filter(Boolean)),
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
