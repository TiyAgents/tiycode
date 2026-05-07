import { describe, expect, it } from "vitest";

import { validateSpec } from "./chart-spec-validation";

describe("validateSpec", () => {
  it("returns null for a valid spec with mark", () => {
    expect(validateSpec({ mark: "line", data: { values: [1, 2, 3] } })).toBeNull();
  });

  it("returns null for a valid spec with layer", () => {
    expect(validateSpec({ layer: [{ mark: "line" }] })).toBeNull();
  });

  it("returns null for a valid spec with concat", () => {
    expect(validateSpec({ concat: [{ mark: "bar" }] })).toBeNull();
  });

  it("returns null for a valid spec with hconcat", () => {
    expect(validateSpec({ hconcat: [{ mark: "area" }] })).toBeNull();
  });

  it("returns null for a valid spec with vconcat", () => {
    expect(validateSpec({ vconcat: [{ mark: "point" }] })).toBeNull();
  });

  it("returns null for a valid spec with facet", () => {
    expect(validateSpec({ facet: { row: { field: "x" } }, spec: { mark: "line" } })).toBeNull();
  });

  it("returns null for a valid spec with repeat", () => {
    expect(validateSpec({ repeat: { row: ["a", "b"] }, spec: { mark: "line" } })).toBeNull();
  });

  it("rejects null input", () => {
    expect(validateSpec(null)).toBe("Spec must be a non-null object");
  });

  it("rejects undefined input", () => {
    expect(validateSpec(undefined)).toBe("Spec must be a non-null object");
  });

  it("rejects string input", () => {
    expect(validateSpec("not an object")).toBe("Spec must be a non-null object");
  });

  it("rejects an empty object without mark or composition", () => {
    expect(validateSpec({})).toBe("Spec must include 'mark', 'layer', or a composition operator");
  });

  it("rejects an object with only data but no mark", () => {
    expect(validateSpec({ data: { values: [1, 2] } })).toBe(
      "Spec must include 'mark', 'layer', or a composition operator",
    );
  });

  it("rejects spec exceeding max size (512KB)", () => {
    // Create a spec with a very large data.values array that will exceed 512KB when serialized
    const largeArray = new Array(60000).fill({ x: 1, y: 2, label: "very long string to fill space" });
    const spec = { mark: "line", data: { values: largeArray } };
    const result = validateSpec(spec);
    expect(result).toBeTruthy();
    expect(result).toContain("Spec exceeds maximum size");
  });

  it("rejects spec with too many data points", () => {
    // Use tiny values so the spec stays under 512KB but exceeds 50K data points
    const largeArray = new Array(60000).fill(0);
    const result = validateSpec({ mark: "line", data: { values: largeArray } });
    expect(result).toBe("Data exceeds maximum points (60000 > 50000)");
  });

  it("accepts spec with exactly 50000 tiny data points", () => {
    const dataValues = new Array(50000).fill(0);
    expect(validateSpec({ mark: "line", data: { values: dataValues } })).toBeNull();
  });
});
