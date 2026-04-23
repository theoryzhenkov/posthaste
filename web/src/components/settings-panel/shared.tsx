/** Reusable form and display primitives for the settings panel. */

import type { AccountOverview } from "../../api/types";
import { cn } from "../../lib/utils";
import { Input } from "../ui/input";

/** Labeled text input field. */
export function Field({
  label,
  value,
  placeholder,
  onChange,
  type,
  disabled,
}: {
  label: string;
  value: string | number;
  placeholder?: string;
  onChange: (value: string) => void;
  type?: string;
  disabled?: boolean;
}) {
  return (
    <label className="grid gap-2 text-sm">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      <Input
        className="h-9 border-border/80 bg-panel shadow-none"
        type={type}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

/** Rounded card displaying a label and large value (e.g., account count). */
export function SummaryCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border/80 bg-panel-muted/55 px-4 py-3 shadow-[var(--shadow-pane)]">
      <div className="text-[10px] font-mono uppercase tracking-[0.2em] text-muted-foreground">
        {label}
      </div>
      <div className="mt-2 text-xl font-semibold tracking-tight text-foreground">
        {value}
      </div>
    </div>
  );
}

/** Compact label/value stat row for metadata sections. */
export function MetaStat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-[10px] font-mono uppercase tracking-[0.18em] text-muted-foreground">
        {label}
      </dt>
      <dd className="mt-1 truncate text-sm text-foreground">{value}</dd>
    </div>
  );
}

const STATUS_DOT_COLOR: Record<AccountOverview["status"], string> = {
  ready: "bg-emerald-500",
  syncing: "bg-blue-500",
  degraded: "bg-amber-500",
  authError: "bg-rose-500",
  offline: "bg-orange-500",
  disabled: "bg-zinc-400",
};

/** Colored status dot for account health indicators. */
export function StatusDot({
  status,
  className,
}: {
  status: AccountOverview["status"];
  className?: string;
}) {
  return (
    <span
      aria-hidden
      title={status}
      className={cn(
        "inline-block h-2 w-2 shrink-0 rounded-full",
        STATUS_DOT_COLOR[status],
        className,
      )}
    />
  );
}

/** Section heading for detail panes. */
export function SectionHeader({
  eyebrow,
  title,
  description,
  actions,
}: {
  eyebrow?: string;
  title: string;
  description?: string;
  actions?: React.ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div className="min-w-0 space-y-1.5">
        {eyebrow && (
          <p className="font-mono text-[10px] uppercase tracking-[0.22em] text-muted-foreground">
            {eyebrow}
          </p>
        )}
        <h3 className="text-[1.05rem] font-semibold tracking-tight text-foreground">
          {title}
        </h3>
        {description && (
          <p className="max-w-2xl text-sm leading-6 text-muted-foreground">
            {description}
          </p>
        )}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function SectionCard({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section
      className={cn(
        "rounded-lg border border-border/80 bg-background/80 p-5 shadow-[var(--shadow-pane)]",
        className,
      )}
    >
      {children}
    </section>
  );
}

export function FeedbackBanner({
  tone,
  children,
}: {
  tone: "success" | "error";
  children: React.ReactNode;
}) {
  return (
    <p
      className={cn(
        "rounded-lg border px-3.5 py-2.5 text-sm shadow-[var(--shadow-pane)]",
        tone === "success"
          ? "border-emerald-500/20 bg-emerald-500/5 text-emerald-700"
          : "border-destructive/20 bg-destructive/5 text-destructive",
      )}
    >
      {children}
    </p>
  );
}
