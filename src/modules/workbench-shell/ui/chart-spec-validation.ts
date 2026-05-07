const MAX_SPEC_SIZE_BYTES = 512_000;
const MAX_DATA_POINTS = 50_000;

export function validateSpec(spec: unknown): string | null {
  if (!spec || typeof spec !== "object") {
    return "Spec must be a non-null object";
  }

  const specStr = JSON.stringify(spec);
  if (specStr.length > MAX_SPEC_SIZE_BYTES) {
    return `Spec exceeds maximum size (${Math.round(specStr.length / 1024)}KB > ${MAX_SPEC_SIZE_BYTES / 1024}KB)`;
  }

  const record = spec as Record<string, unknown>;
  if (!record.mark && !record.layer && !record.concat && !record.hconcat && !record.vconcat && !record.facet && !record.repeat) {
    return "Spec must include 'mark', 'layer', or a composition operator";
  }

  const data = record.data as Record<string, unknown> | undefined;
  if (data && "values" in data && Array.isArray(data.values)) {
    if (data.values.length > MAX_DATA_POINTS) {
      return `Data exceeds maximum points (${data.values.length} > ${MAX_DATA_POINTS})`;
    }
  }

  return null;
}
