export function InspectorItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-app-border bg-app-surface-muted p-3">
      <p className="text-xs uppercase tracking-[0.14em] text-app-subtle">{label}</p>
      <p className="mt-2 text-sm text-app-foreground">{value}</p>
    </div>
  );
}
