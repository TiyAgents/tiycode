import { describe, expect, it } from "vitest";
import { getInvokeErrorMessage } from "./invoke-error";

describe("getInvokeErrorMessage", () => {
  it("prefers string and Error messages", () => {
    expect(getInvokeErrorMessage("plain error", "fallback")).toBe("plain error");
    expect(getInvokeErrorMessage(new Error("boom"), "fallback")).toBe("boom");
  });

  it("uses known object message fields by priority", () => {
    expect(getInvokeErrorMessage({ userMessage: "user", message: "message" }, "fallback")).toBe("user");
    expect(getInvokeErrorMessage({ message: "message", detail: "detail" }, "fallback")).toBe("message");
    expect(getInvokeErrorMessage({ detail: "detail", description: "desc" }, "fallback")).toBe("detail");
    expect(getInvokeErrorMessage({ description: "desc", error: "err" }, "fallback")).toBe("desc");
    expect(getInvokeErrorMessage({ error: "err" }, "fallback")).toBe("err");
  });

  it("serializes non-empty unknown objects", () => {
    expect(getInvokeErrorMessage({ code: "E_TEST" }, "fallback")).toBe('{"code":"E_TEST"}');
  });

  it("falls back for blank or unserializable errors", () => {
    const circular: Record<string, unknown> = {};
    circular.self = circular;

    expect(getInvokeErrorMessage("   ", "fallback")).toBe("fallback");
    expect(getInvokeErrorMessage({}, "fallback")).toBe("fallback");
    expect(getInvokeErrorMessage(circular, "fallback")).toBe("fallback");
  });
});
