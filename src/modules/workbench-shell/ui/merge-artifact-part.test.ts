import { describe, expect, it } from "vitest";

import {
  mergeArtifactEventIntoMessages,
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
    messageId: "msg-1",
    runId: "run-1",
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

describe("mergeArtifactEventIntoMessages", () => {
  it("creates a render artifact host message when no non-reasoning message exists", () => {
    const result = mergeArtifactEventIntoMessages(
      [],
      makeEvent({ messageId: "host-msg", runId: "run-host", kind: "started" }),
      "2026-01-02T00:00:00Z",
    );

    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({
      id: "host-msg",
      createdAt: "2026-01-02T00:00:00Z",
      messageType: "plain_message",
      role: "assistant",
      runId: "run-host",
      content: "",
      status: "streaming",
    });
    expect(result[0].parts).toHaveLength(1);
    expect(result[0].parts[0]).toMatchObject({
      type: "chart",
      artifactId: "art-1",
      status: "loading",
    });
  });

  it("updates the same host chart part from started to completed", () => {
    const started = mergeArtifactEventIntoMessages(
      [],
      makeEvent({ messageId: "host-msg", runId: "run-host", kind: "started" }),
      "2026-01-02T00:00:00Z",
    );
    const completed = mergeArtifactEventIntoMessages(
      started,
      makeEvent({ messageId: "host-msg", runId: "run-host", kind: "completed" }),
      "2026-01-02T00:00:01Z",
    );

    expect(completed).toHaveLength(1);
    expect(completed[0].id).toBe("host-msg");
    expect(completed[0].status).toBe("completed");
    expect(completed[0].parts).toHaveLength(1);
    expect(completed[0].parts[0]).toMatchObject({
      type: "chart",
      artifactId: "art-1",
      status: "ready",
    });
  });

  it("keeps host messages streaming while artifact deltas arrive", () => {
    const started = mergeArtifactEventIntoMessages(
      [],
      makeEvent({ messageId: "host-msg", runId: "run-host", kind: "started" }),
      "2026-01-02T00:00:00Z",
    );
    const delta = mergeArtifactEventIntoMessages(
      started,
      makeEvent({ messageId: "host-msg", runId: "run-host", kind: "delta" }),
      "2026-01-02T00:00:01Z",
    );

    expect(delta).toHaveLength(1);
    expect(delta[0].status).toBe("streaming");
    expect(delta[0].parts[0]).toMatchObject({
      type: "chart",
      artifactId: "art-1",
      status: "ready",
    });
  });

  it("does not attach artifacts to reasoning messages and creates a host instead", () => {
    const reasoning = makeMessage({
      id: "host-msg",
      messageType: "reasoning",
      content: "thinking",
      parts: [{ type: "text", text: "thinking" }],
    });

    const result = mergeArtifactEventIntoMessages(
      [reasoning],
      makeEvent({ messageId: "host-msg", runId: "run-1" }),
      "2026-01-02T00:00:00Z",
    );

    expect(result).toHaveLength(2);
    expect(result[0]).toBe(reasoning);
    expect(result[0].parts).toEqual([{ type: "text", text: "thinking" }]);
    expect(result[1]).toMatchObject({
      id: "host-msg",
      messageType: "plain_message",
      role: "assistant",
    });
    expect(result[1].parts[0]).toMatchObject({ type: "chart", artifactId: "art-1" });
  });

  it("does not treat non-chart non-text messages as artifact hosts", () => {
    const existing = makeMessage({
      id: "host-msg",
      content: "",
      parts: [{ type: "data-custom", data: { ok: true } }],
      status: "completed",
    });

    const result = mergeArtifactEventIntoMessages(
      [existing],
      makeEvent({ messageId: "host-msg", runId: "run-1", kind: "started" }),
      "2026-01-02T00:00:00Z",
    );

    expect(result).toHaveLength(1);
    expect(result[0].status).toBe("completed");
    expect(result[0].parts).toHaveLength(2);
    expect(result[0].parts[0]).toEqual({ type: "data-custom", data: { ok: true } });
    expect(result[0].parts[1]).toMatchObject({ type: "chart", artifactId: "art-1" });
  });

  it("returns messages unchanged for non-chart artifacts", () => {
    const messages = [makeMessage()];
    const result = mergeArtifactEventIntoMessages(
      messages,
      makeEvent({ artifactType: "unknown" }),
      "2026-01-02T00:00:00Z",
    );

    expect(result).toBe(messages);
  });
});
