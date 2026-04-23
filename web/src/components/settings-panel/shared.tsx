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
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">{label}</span>
      <Input
        className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
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
    <div className="flex h-9 items-center justify-between gap-3 rounded-md border border-border-soft bg-bg-elev px-3">
      <div className="truncate text-[12px] font-medium text-muted-foreground">
        {label}
      </div>
      <div className="font-mono text-[13px] font-semibold text-foreground">
        {value}
      </div>
    </div>
  );
}

/** Compact label/value stat row for metadata sections. */
export function MetaStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 items-center justify-between gap-3">
      <dt className="shrink-0 text-[12px] font-medium text-muted-foreground">
        {label}
      </dt>
      <dd className="truncate font-mono text-[12px] text-foreground">{value}</dd>
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
  actions,
}: {
  eyebrow?: string;
  title: string;
  description?: string;
  actions?: React.ReactNode;
}) {
  return (
    <div className="flex min-h-8 items-center justify-between gap-3">
      <div className="min-w-0">
        {eyebrow && (
          <p className="font-mono text-[11px] font-semibold uppercase tracking-[0.7px] text-muted-foreground">
            {eyebrow}
          </p>
        )}
        <h3 className={cn(
          "truncate font-semibold text-foreground",
          eyebrow ? "text-[13px]" : "text-[15px]",
        )}>
          {title}
        </h3>
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-1.5">{actions}</div>}
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
        "mb-4 space-y-3 rounded-lg border border-border-soft bg-bg-elev/45 px-4 py-3 last:mb-0",
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
        "rounded-md border px-3 py-2 text-[12px]",
        tone === "success"
          ? "border-emerald-500/20 bg-emerald-500/5 text-emerald-700"
          : "border-destructive/20 bg-destructive/5 text-destructive",
      )}
    >
      {children}
    </p>
  );
}
