export const HTML_PREVIEW_CSP = [
  "default-src 'none'",
  "img-src http: https: data: blob: asset:",
  "media-src http: https: data: blob: asset:",
  "style-src 'unsafe-inline' http: https: asset:",
  "font-src http: https: data: asset:",
  "script-src 'unsafe-inline' 'unsafe-eval' http: https: asset:",
  "connect-src http: https: data: blob: ws: wss:",
  "frame-src http: https:",
  "object-src 'none'",
  "form-action 'none'",
].join("; ");

const HTML_PREVIEW_HEAD_MARKUP = [
  '<meta charset="utf-8">',
  `<meta http-equiv="Content-Security-Policy" content="${HTML_PREVIEW_CSP}">`,
  '<base target="_blank">',
].join("\n");

const HEAD_OPEN_TAG_PATTERN = /<head(\s[^>]*)?>/i;
const HTML_OPEN_TAG_PATTERN = /<html(\s[^>]*)?>/i;

export function buildHtmlPreviewDocument(source: string): string {
  if (HEAD_OPEN_TAG_PATTERN.test(source)) {
    return source.replace(
      HEAD_OPEN_TAG_PATTERN,
      (headOpenTag) => `${headOpenTag}\n${HTML_PREVIEW_HEAD_MARKUP}`,
    );
  }

  if (HTML_OPEN_TAG_PATTERN.test(source)) {
    return source.replace(
      HTML_OPEN_TAG_PATTERN,
      (htmlOpenTag) => `${htmlOpenTag}\n<head>\n${HTML_PREVIEW_HEAD_MARKUP}\n</head>`,
    );
  }

  return `<!doctype html>\n<html>\n<head>\n${HTML_PREVIEW_HEAD_MARKUP}\n</head>\n<body>\n${source}\n</body>\n</html>`;
}
