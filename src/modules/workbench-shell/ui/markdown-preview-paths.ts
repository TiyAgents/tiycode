export function normalizeRelativePath(raw: string): string {
  const out: string[] = [];

  for (const segment of raw.split("/")) {
    if (segment === "." || segment === "") continue;
    if (segment === "..") {
      if (out.length > 0 && out[out.length - 1] !== "..") {
        out.pop();
      } else {
        out.push(segment);
      }
      continue;
    }
    out.push(segment);
  }

  return out.join("/");
}

export function isLocalRelativeUrl(url: string | undefined): boolean {
  if (!url) return false;
  if (/^[a-z][a-z0-9+.-]*:/i.test(url)) return false;
  if (url.startsWith("//")) return false;
  if (url.startsWith("#")) return false;
  return true;
}
