/**
 * Extract a human-readable message from an unknown error value.
 *
 * Returns `null` when no meaningful message could be extracted (empty string,
 * empty object, circular reference, etc.).
 */
export function formatInvokeErrorMessage(error: unknown): string | null {
  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }

  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }

  if (typeof error === "object" && error !== null) {
    const userMessage = Reflect.get(error, "userMessage");
    if (typeof userMessage === "string" && userMessage.trim().length > 0) {
      return userMessage;
    }

    const message = Reflect.get(error, "message");
    if (typeof message === "string" && message.trim().length > 0) {
      return message;
    }

    const detail = Reflect.get(error, "detail");
    if (typeof detail === "string" && detail.trim().length > 0) {
      return detail;
    }

    const description = Reflect.get(error, "description");
    if (typeof description === "string" && description.trim().length > 0) {
      return description;
    }

    const errorText = Reflect.get(error, "error");
    if (typeof errorText === "string" && errorText.trim().length > 0) {
      return errorText;
    }

    try {
      const serialized = JSON.stringify(error);
      if (serialized && serialized !== "{}") {
        return serialized;
      }
    } catch {
      // Ignore serialization issues and fall back below.
    }
  }

  return null;
}

/**
 * Extract a human-readable error message, falling back to a caller-provided
 * default when no meaningful message can be found.
 *
 * Uses the same extraction chain as {@link formatInvokeErrorMessage}.
 */
export function getInvokeErrorMessage(error: unknown, fallback: string): string {
  const formatted = formatInvokeErrorMessage(error);
  if (formatted !== null) return formatted;
  return fallback;
}
