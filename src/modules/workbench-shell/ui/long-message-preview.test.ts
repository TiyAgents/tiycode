import { describe, expect, it } from "vitest";
import {
  getLongMessagePreview,
  shouldUseLongMessagePreview,
} from "./runtime-thread-surface";

// ── getLongMessagePreview ────────────────────────────────────

describe("getLongMessagePreview", () => {
  it("returns isLong=false for short content", () => {
    const result = getLongMessagePreview("hello world");
    expect(result.isLong).toBe(false);
    expect(result.hiddenLineCount).toBe(0);
    expect(result.previewText).toBe("hello world");
  });

  it("returns isLong=false for empty string", () => {
    const result = getLongMessagePreview("");
    expect(result.isLong).toBe(false);
    expect(result.previewText).toBe("");
  });

  it("returns isLong=false for whitespace-only content", () => {
    const result = getLongMessagePreview("   \n  \n  ");
    expect(result.isLong).toBe(false);
  });

  it("returns isLong=true when exceeding line limit", () => {
    const lines = Array.from({ length: 150 }, (_, i) => `line ${i + 1}`);
    const content = lines.join("\n");
    const result = getLongMessagePreview(content);

    expect(result.isLong).toBe(true);
    expect(result.previewText.split("\n").length).toBeLessThanOrEqual(120);
    expect(result.hiddenLineCount).toBe(150 - 120);
  });

  it("returns isLong=true when exceeding char limit", () => {
    const content = "x".repeat(15_000);
    const result = getLongMessagePreview(content);

    expect(result.isLong).toBe(true);
    expect(result.previewText.length).toBeLessThanOrEqual(12_000);
  });

  it("returns isLong=true when exceeding both limits", () => {
    const lines = Array.from({ length: 200 }, () => "a".repeat(200));
    const content = lines.join("\n");
    const result = getLongMessagePreview(content);

    expect(result.isLong).toBe(true);
    expect(result.previewText.split("\n").length).toBeLessThanOrEqual(120);
    expect(result.previewText.length).toBeLessThanOrEqual(12_000);
  });

  it("returns isLong=false at exactly the line limit", () => {
    const lines = Array.from({ length: 120 }, (_, i) => `l${i}`);
    const content = lines.join("\n");
    const result = getLongMessagePreview(content);

    expect(result.isLong).toBe(false);
  });

  it("hiddenLineCount is never negative", () => {
    const content = "x".repeat(15_000);
    const result = getLongMessagePreview(content);

    expect(result.hiddenLineCount).toBeGreaterThanOrEqual(0);
  });

  it("trims trailing whitespace from previewText", () => {
    const lines = Array.from({ length: 150 }, (_, i) => `line ${i}   `);
    const content = lines.join("\n");
    const result = getLongMessagePreview(content);

    expect(result.previewText).toBe(result.previewText.trimEnd());
  });
});

// ── shouldUseLongMessagePreview ─────────────────────────────

describe("shouldUseLongMessagePreview", () => {
  it("returns true for completed plain_message", () => {
    expect(
      shouldUseLongMessagePreview({
        content: "any",
        messageType: "plain_message",
        status: "completed",
      }),
    ).toBe(true);
  });

  it("returns true for failed plain_message", () => {
    expect(
      shouldUseLongMessagePreview({
        content: "any",
        messageType: "plain_message",
        status: "failed",
      }),
    ).toBe(true);
  });

  it("returns false for streaming message", () => {
    expect(
      shouldUseLongMessagePreview({
        content: "any",
        messageType: "plain_message",
        status: "streaming",
      }),
    ).toBe(false);
  });

  it("returns false for non-plain_message type", () => {
    expect(
      shouldUseLongMessagePreview({
        content: "any",
        messageType: "plan",
        status: "completed",
      }),
    ).toBe(false);
  });

  it("returns false for discarded message", () => {
    expect(
      shouldUseLongMessagePreview({
        content: "any",
        messageType: "plain_message",
        status: "discarded",
      }),
    ).toBe(false);
  });
});
