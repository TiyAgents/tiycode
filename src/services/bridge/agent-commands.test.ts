import { describe, expect, it, vi } from "vitest";

import { normalizeThreadStreamEvent } from "./agent-commands";
import type { RawThreadStreamEvent } from "./agent-commands";
import type { ThreadStreamEvent } from "@/shared/types/api";

function makeRawEvent(overrides: Record<string, unknown> = {}): RawThreadStreamEvent {
  return {
    type: "artifact_updated",
    runId: "run-1",
    run_id: "run-1",
    messageId: "msg-1",
    message_id: "msg-1",
    artifactId: "artifact-1",
    artifact_id: "artifact-1",
    artifactType: "chart",
    artifact_type: "chart",
    status: "started",
    payload: { library: "vega-lite", spec: { mark: "line" } },
    error: null,
    ...overrides,
  };
}

describe("normalizeThreadStreamEvent artifact_updated", () => {
  it("normalizes a valid artifact_updated event with all fields", () => {
    const result = normalizeThreadStreamEvent(makeRawEvent()) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.type).toBe("artifact_updated");
    expect(result.runId).toBe("run-1");
    expect(result.messageId).toBe("msg-1");
    expect(result.artifactId).toBe("artifact-1");
    expect(result.artifactType).toBe("chart");
    expect(result.status).toBe("started");
    expect(result.payload).toEqual({ library: "vega-lite", spec: { mark: "line" } });
    expect(result.error).toBeUndefined();
  });

  it("falls back to snake_case keys when camelCase is missing", () => {
    const raw = makeRawEvent();
    delete raw.runId;
    delete raw.messageId;
    delete raw.artifactId;
    delete raw.artifactType;

    const result = normalizeThreadStreamEvent(raw) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.runId).toBe("run-1");
    expect(result.messageId).toBe("msg-1");
    expect(result.artifactId).toBe("artifact-1");
    expect(result.artifactType).toBe("chart");
  });

  it("throws when a required field is missing entirely", () => {
    const raw = makeRawEvent();
    delete raw.runId;
    delete raw.run_id;

    expect(() => normalizeThreadStreamEvent(raw)).toThrow(/missing runId/);
  });

  it("maps null payload to undefined error", () => {
    const raw = makeRawEvent({ error: null, payload: null });
    const result = normalizeThreadStreamEvent(raw);

    expect(result).toMatchObject({ error: undefined });
  });

  it("maps a string error field", () => {
    const raw = makeRawEvent({ error: "rendering failed" });
    const result = normalizeThreadStreamEvent(raw) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.error).toBe("rendering failed");
  });

  it("validates and maps known status strings", () => {
    for (const status of ["started", "delta", "completed", "failed"]) {
      const result = normalizeThreadStreamEvent(makeRawEvent({ status }));
      expect(result).toMatchObject({ type: "artifact_updated", status });
    }
  });

  it("falls back to 'completed' for unknown status values", () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const raw = makeRawEvent({ status: "unknown_status" });

    const result = normalizeThreadStreamEvent(raw) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.status).toBe("completed");
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('Unknown artifact status "unknown_status"'),
    );

    warnSpy.mockRestore();
  });
});
