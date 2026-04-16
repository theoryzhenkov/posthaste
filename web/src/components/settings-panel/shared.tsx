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
    <label className="grid gap-1.5 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <Input
        className="h-9 bg-card"
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
    <div className="rounded-lg border border-border bg-background/70 px-3 py-2">
      <div className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      <div className="mt-1 text-lg font-semibold tracking-tight">{value}</div>
    </div>
  );
}

/** Compact label/value stat row for metadata sections. */
export function MetaStat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
        {label}
      </dt>
      <dd className="mt-1 truncate text-sm">{value}</dd>
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
  title,
  description,
  actions,
}: {
  title: string;
  description?: string;
  actions?: React.ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h3 className="text-base font-semibold tracking-tight">{title}</h3>
        {description && (
          <p className="mt-1 text-sm text-muted-foreground">{description}</p>
        )}
      </div>
      {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
    </div>
  );
}
