import { describe, expect, it } from "vitest";

import { parseRunEventMetadata } from "./runtime-thread-surface-metadata";

describe("parseRunEventMetadata", () => {
  it("parses persisted run retrying metadata", () => {
    expect(
      parseRunEventMetadata({
        kind: "run_retrying",
        attempt: 2,
        maxAttempts: 5,
        delayMs: 4000,
        reason: "Provider error: error sending request for url",
        previousRunId: "run-previous",
        nextRunId: "run-next",
      }),
    ).toEqual({
      kind: "run_retrying",
      attempt: 2,
      maxAttempts: 5,
      delayMs: 4000,
      reason: "Provider error: error sending request for url",
      previousRunId: "run-previous",
      nextRunId: "run-next",
    });
  });

  it("returns null for non-object metadata", () => {
    expect(parseRunEventMetadata(null)).toBeNull();
    expect(parseRunEventMetadata("run_retrying")).toBeNull();
  });
});
