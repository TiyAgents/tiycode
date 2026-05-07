import { describe, expect, it } from "vitest";

import {
  mergeArtifactPartIntoMessage,
  type ArtifactEvent,
  type SurfaceMessage,
} from "./runtime-thread-surface-state";

function makeMessage(overrides: Partial<SurfaceMessage> = {}): SurfaceMessage {
  return {
    id: "msg-1",
    createdAt: "2026-01-01T00:00:00Z",
    messageType: "plain_message",
    attachments: [],
    role: "assistant",
    runId: "run-1",
    content: "hello",
    parts: [{ type: "text", text: "hello" }],
    status: "completed",
    ...overrides,
  };
}

function makeEvent(overrides: Partial<ArtifactEvent> = {}): ArtifactEvent {
  return {
    artifactId: "art-1",
    artifactType: "chart",
    kind: "completed",
    payload: { library: "vega-lite", spec: { mark: "line" } },
    ...overrides,
  };
}

describe("mergeArtifactPartIntoMessage", () => {
  it("returns message unchanged when artifactType is not 'chart'", () => {
    const msg = makeMessage();
    const result = mergeArtifactPartIntoMessage(msg, makeEvent({ artifactType: "unknown" }));
    expect(result).toBe(msg);
  });

  it("appends a new chart part when no matching artifactId exists", () => {
    const msg = makeMessage();
    const result = mergeArtifactPartIntoMessage(msg, makeEvent());
    expect(result.parts).toHaveLength(2);
    expect(result.parts[1]).toMatchObject({
      type: "chart",
      artifactId: "art-1",
      library: "vega-lite",
      spec: { mark: "line" },
      status: "ready",
    });
  });

  it("updates existing chart part when artifactId matches", () => {
    const msg = makeMessage({
      parts: [
        { type: "text", text: "hello" },
        {
          type: "chart",
          artifactId: "art-1",
          library: "vega-lite",
          spec: { mark: "bar" },
          source: null,
          title: null,
          caption: null,
          status: "loading",
          error: null,
        },
      ],
    });
    const result = mergeArtifactPartIntoMessage(msg, makeEvent({ kind: "completed" }));
    expect(result.parts).toHaveLength(2);
    const chartPart = result.parts[1];
    expect(chartPart).toMatchObject({
      type: "chart",
      artifactId: "art-1",
      spec: { mark: "line" },
      status: "ready",
    });
  });

  it("maps 'started' kind to 'loading' status", () => {
    const result = mergeArtifactPartIntoMessage(makeMessage(), makeEvent({ kind: "started" }));
    const chart = result.parts[1];
    expect(chart).toMatchObject({ status: "loading" });
  });

  it("maps 'failed' kind to 'error' status", () => {
    const result = mergeArtifactPartIntoMessage(
      makeMessage(),
      makeEvent({ kind: "failed", error: "render failed" }),
    );
    const chart = result.parts[1];
    expect(chart).toMatchObject({ status: "error", error: "render failed" });
  });

  it("maps 'delta' and 'completed' kinds to 'ready' status", () => {
    for (const kind of ["delta", "completed"] as const) {
      const result = mergeArtifactPartIntoMessage(makeMessage(), makeEvent({ kind }));
      const chart = result.parts[1];
      expect(chart).toMatchObject({ status: "ready" });
    }
  });

  it("defaults library to 'vega-lite' when payload has no library field", () => {
    const result = mergeArtifactPartIntoMessage(
      makeMessage(),
      makeEvent({ payload: { spec: { mark: "point" } } }),
    );
    const chart = result.parts[1];
    expect(chart).toMatchObject({ library: "vega-lite" });
  });

  it("reads source, title, caption from payload for html/svg artifacts", () => {
    const result = mergeArtifactPartIntoMessage(
      makeMessage(),
      makeEvent({
        payload: {
          library: "html",
          source: "<div>hi</div>",
          title: "My Chart",
          caption: "A caption",
        },
      }),
    );
    const chart = result.parts[1];
    expect(chart).toMatchObject({
      library: "html",
      source: "<div>hi</div>",
      title: "My Chart",
      caption: "A caption",
    });
  });

  it("handles null/undefined payload gracefully", () => {
    const result = mergeArtifactPartIntoMessage(
      makeMessage(),
      makeEvent({ payload: undefined }),
    );
    const chart = result.parts[1];
    expect(chart).toMatchObject({
      library: "vega-lite",
      spec: {},
      source: null,
      title: null,
      caption: null,
    });
  });

  it("reads error from payload.error when event.error is not set", () => {
    const result = mergeArtifactPartIntoMessage(
      makeMessage(),
      makeEvent({ kind: "failed", payload: { library: "vega-lite", spec: {}, error: "payload error" } }),
    );
    const chart = result.parts[1];
    expect(chart).toMatchObject({ error: "payload error" });
  });

  it("prefers event.error over payload.error", () => {
    const result = mergeArtifactPartIntoMessage(
      makeMessage(),
      makeEvent({
        kind: "failed",
        error: "event error",
        payload: { library: "vega-lite", spec: {}, error: "payload error" },
      }),
    );
    const chart = result.parts[1];
    expect(chart).toMatchObject({ error: "event error" });
  });

  it("does not mutate the original message", () => {
    const msg = makeMessage();
    const original = { ...msg, parts: [...msg.parts] };
    mergeArtifactPartIntoMessage(msg, makeEvent());
    expect(msg.parts).toEqual(original.parts);
  });
});
