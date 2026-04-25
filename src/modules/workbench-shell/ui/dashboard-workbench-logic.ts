export function resolveThreadProfileId(
  threadProfileId: string | null,
  globalActiveProfileId: string,
): string {
  return threadProfileId || globalActiveProfileId;
}

export function resolveActiveThreadWorkbenchProfileId(
  threadProfileId: string | null,
  globalActiveProfileId: string,
): string {
  return threadProfileId || globalActiveProfileId;
}
