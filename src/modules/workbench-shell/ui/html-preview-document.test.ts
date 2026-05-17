import { describe, expect, it } from "vitest";

import { buildHtmlPreviewDocument, HTML_PREVIEW_CSP } from "./html-preview-document";

describe("buildHtmlPreviewDocument", () => {
  it("injects preview metadata into an existing head", () => {
    const document = buildHtmlPreviewDocument(
      '<!doctype html><html><head><title>Demo</title></head><body><img src="https://example.com/a.png"></body></html>',
    );

    expect(document).toContain('<head>\n<meta charset="utf-8">');
    expect(document).toContain(`content="${HTML_PREVIEW_CSP}"`);
    expect(document).toContain("img-src http: https: data: blob: asset:");
    expect(document).toContain("style-src 'unsafe-inline' http: https: asset:");
    expect(document).toContain('<base target="_blank">');
    expect(document).toContain("<title>Demo</title>");
  });

  it("adds a head when the source has an html element without one", () => {
    const document = buildHtmlPreviewDocument('<html lang="en"><body><h1>Demo</h1></body></html>');

    expect(document).toContain('<html lang="en">\n<head>\n<meta charset="utf-8">');
    expect(document).toContain("<body><h1>Demo</h1></body>");
  });

  it("wraps HTML fragments in a complete preview document", () => {
    const document = buildHtmlPreviewDocument('<link rel="stylesheet" href="https://cdn.example.com/app.css"><h1>Demo</h1>');

    expect(document).toMatch(/^<!doctype html>\n<html>\n<head>/);
    expect(document).toContain("<body>\n<link rel=\"stylesheet\" href=\"https://cdn.example.com/app.css\"><h1>Demo</h1>\n</body>");
  });
});
