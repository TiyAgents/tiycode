import { describe, expect, it } from "vitest";
import { threadRunStatusToDisplayStatus } from "./types";
import type { ThreadRunStatus, ThreadStatus } from "./types";

describe("threadRunStatusToDisplayStatus", () => {
  const cases: Array<[ThreadRunStatus, ThreadStatus]> = [
    ["idle", "completed"],
    ["running", "running"],
    ["waiting_approval", "needs-reply"],
    ["needs_reply", "needs-reply"],
    ["completed", "completed"],
    ["failed", "failed"],
    ["cancelled", "completed"],
    ["interrupted", "interrupted"],
    ["limit_reached", "needs-reply"],
  ];

  for (const [input, expected] of cases) {
    it(`maps "${input}" → "${expected}"`, () => {
      expect(threadRunStatusToDisplayStatus(input)).toBe(expected);
    });
  }
});
