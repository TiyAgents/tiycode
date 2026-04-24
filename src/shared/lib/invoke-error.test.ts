import { describe, it, expect } from "vitest";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";

describe("getInvokeErrorMessage", () => {
  const FALLBACK = "Something went wrong";

  // -- String errors --
  it("returns string error directly", () => {
    expect(getInvokeErrorMessage("Connection failed", FALLBACK)).toBe("Connection failed");
  });

  it("returns fallback for empty string", () => {
    expect(getInvokeErrorMessage("", FALLBACK)).toBe(FALLBACK);
  });

  it("returns fallback for whitespace-only string", () => {
    expect(getInvokeErrorMessage("   ", FALLBACK)).toBe(FALLBACK);
  });

  // -- Error instances --
  it("returns Error.message", () => {
    expect(getInvokeErrorMessage(new Error("fail"), FALLBACK)).toBe("fail");
  });

  it("returns fallback for Error with empty message", () => {
    expect(getInvokeErrorMessage(new Error(""), FALLBACK)).toBe(FALLBACK);
  });

  // -- Object with userMessage --
  it("returns userMessage from object", () => {
    expect(getInvokeErrorMessage({ userMessage: "Oops!" }, FALLBACK)).toBe("Oops!");
  });

  it("prefers userMessage over message", () => {
    expect(getInvokeErrorMessage({ userMessage: "User msg", message: "Internal msg" }, FALLBACK)).toBe("User msg");
  });

  // -- Object with message --
  it("returns message from plain object", () => {
    expect(getInvokeErrorMessage({ message: "error detail" }, FALLBACK)).toBe("error detail");
  });

  // -- Object with detail --
  it("returns detail from object", () => {
    expect(getInvokeErrorMessage({ detail: "detailed error" }, FALLBACK)).toBe("detailed error");
  });

  // -- Object with description --
  it("returns description from object", () => {
    expect(getInvokeErrorMessage({ description: "desc error" }, FALLBACK)).toBe("desc error");
  });

  // -- Object with error --
  it("returns error field from object", () => {
    expect(getInvokeErrorMessage({ error: "error text" }, FALLBACK)).toBe("error text");
  });

  // -- Priority order --
  it("tries fields in priority: userMessage > message > detail > description > error", () => {
    expect(getInvokeErrorMessage({ detail: "d", description: "desc", error: "e" }, FALLBACK)).toBe("d");
    expect(getInvokeErrorMessage({ description: "desc", error: "e" }, FALLBACK)).toBe("desc");
    expect(getInvokeErrorMessage({ error: "e" }, FALLBACK)).toBe("e");
  });

  // -- JSON serializable object --
  it("JSON-stringifies object when no known fields", () => {
    const result = getInvokeErrorMessage({ code: 500, status: "internal" }, FALLBACK);
    expect(result).toContain('"code":500');
    expect(result).toContain('"status":"internal"');
  });

  it("returns fallback for empty object ({})", () => {
    expect(getInvokeErrorMessage({}, FALLBACK)).toBe(FALLBACK);
  });

  // -- Non-serializable objects --
  it("returns fallback for circular references", () => {
    const obj: Record<string, unknown> = {};
    obj.self = obj;
    expect(getInvokeErrorMessage(obj, FALLBACK)).toBe(FALLBACK);
  });

  // -- null / undefined / other types --
  it("returns fallback for null", () => {
    expect(getInvokeErrorMessage(null, FALLBACK)).toBe(FALLBACK);
  });

  it("returns fallback for undefined", () => {
    expect(getInvokeErrorMessage(undefined, FALLBACK)).toBe(FALLBACK);
  });

  it("returns fallback for number", () => {
    expect(getInvokeErrorMessage(42, FALLBACK)).toBe(FALLBACK);
  });

  it("returns fallback for boolean", () => {
    expect(getInvokeErrorMessage(true, FALLBACK)).toBe(FALLBACK);
  });

  it("JSON-stringifies arrays (they pass typeof object check)", () => {
    // Arrays are objects, so they get JSON.stringify'd
    const result = getInvokeErrorMessage(["error1", "error2"], FALLBACK);
    expect(result).toBe('["error1","error2"]');
  });

  // -- Whitespace handling in object fields --
  it("skips whitespace-only userMessage", () => {
    expect(getInvokeErrorMessage({ userMessage: "   ", message: "real" }, FALLBACK)).toBe("real");
  });

  it("skips whitespace-only message and uses detail", () => {
    expect(getInvokeErrorMessage({ message: " ", detail: "real detail" }, FALLBACK)).toBe("real detail");
  });
});
