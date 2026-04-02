import type {
  AccountDriver,
  AccountOverview,
  CreateAccountInput,
  SmartMailbox,
  SmartMailboxCondition,
  SmartMailboxField,
  SmartMailboxGroupOperator,
  SmartMailboxOperator,
  SmartMailboxRuleNode,
  SmartMailboxSummary,
  UpdateAccountInput,
} from "../../api/types";
import type {
  AccountFormState,
  SmartMailboxFormState,
} from "./types";

export const ACCOUNT_DRIVER_VALUES = ["jmap", "mock"] as const;

export const EMPTY_FORM: AccountFormState = {
  id: "",
  name: "",
  driver: "jmap",
  enabled: true,
  baseUrl: "",
  username: "",
  password: "",
  secretMode: "replace",
};

export const EMPTY_SMART_MAILBOX_FORM: SmartMailboxFormState = {
  name: "",
  position: 0,
  rule: {
    root: {
      operator: "all",
      negated: false,
      nodes: [],
    },
  },
};

export function formFromAccount(account: AccountOverview): AccountFormState {
  return {
    id: account.id,
    name: account.name,
    driver: account.driver,
    enabled: account.enabled,
    baseUrl: account.transport.baseUrl ?? "",
    username: account.transport.username ?? "",
    password: "",
    secretMode: account.transport.secret.configured ? "keep" : "replace",
  };
}

export function formFromSmartMailbox(
  smartMailbox: SmartMailbox | SmartMailboxSummary,
): SmartMailboxFormState {
  return {
    name: smartMailbox.name,
    position: smartMailbox.position,
    rule: "rule" in smartMailbox ? smartMailbox.rule : EMPTY_SMART_MAILBOX_FORM.rule,
  };
}

export const FIELD_OPTIONS: Array<{ value: SmartMailboxField; label: string }> = [
  { value: "sourceId", label: "Source ID" },
  { value: "sourceName", label: "Source Name" },
  { value: "mailboxId", label: "Mailbox ID" },
  { value: "mailboxRole", label: "Mailbox Role" },
  { value: "isRead", label: "Read state" },
  { value: "isFlagged", label: "Flagged" },
  { value: "hasAttachment", label: "Has attachment" },
  { value: "keyword", label: "Keyword" },
  { value: "fromName", label: "From name" },
  { value: "fromEmail", label: "From email" },
  { value: "subject", label: "Subject" },
  { value: "preview", label: "Preview" },
  { value: "receivedAt", label: "Received at" },
];

export const GROUP_OPERATOR_OPTIONS: Array<{
  value: SmartMailboxGroupOperator;
  label: string;
}> = [
  { value: "all", label: "All" },
  { value: "any", label: "Any" },
];

export function parseAccountDriver(
  value: string,
  fallback: AccountDriver,
): AccountDriver {
  return ACCOUNT_DRIVER_VALUES.find((candidate) => candidate === value) ?? fallback;
}

export function parseGroupOperator(
  value: string,
  fallback: SmartMailboxGroupOperator,
): SmartMailboxGroupOperator {
  return GROUP_OPERATOR_OPTIONS.find((option) => option.value === value)?.value ?? fallback;
}

export function parseField(
  value: string,
  fallback: SmartMailboxField,
): SmartMailboxField {
  return FIELD_OPTIONS.find((option) => option.value === value)?.value ?? fallback;
}

export function parseOperator(
  value: string,
  field: SmartMailboxField,
  fallback: SmartMailboxOperator,
): SmartMailboxOperator {
  return operatorOptionsForField(field).find((operator) => operator === value) ?? fallback;
}

export function operatorOptionsForField(
  field: SmartMailboxField,
): SmartMailboxOperator[] {
  switch (field) {
    case "sourceId":
    case "sourceName":
    case "mailboxId":
    case "mailboxRole":
    case "keyword":
      return ["equals", "in"];
    case "isRead":
    case "isFlagged":
    case "hasAttachment":
      return ["equals"];
    case "fromName":
    case "fromEmail":
    case "subject":
    case "preview":
      return ["equals", "contains", "in"];
    case "receivedAt":
      return ["before", "after", "onOrBefore", "onOrAfter"];
  }
}

export function defaultCondition(
  field: SmartMailboxField = "mailboxRole",
): SmartMailboxCondition {
  const operator = operatorOptionsForField(field)[0];
  const isBooleanField =
    field === "isRead" || field === "isFlagged" || field === "hasAttachment";
  return {
    type: "condition",
    field,
    operator,
    negated: false,
    value: isBooleanField ? false : "",
  };
}

export function defaultGroup(): SmartMailboxRuleNode {
  return {
    type: "group",
    operator: "all",
    negated: false,
    nodes: [],
  };
}

export function buildSecretInput(form: AccountFormState) {
  if (form.secretMode === "replace") {
    return { mode: "replace" as const, password: form.password };
  }
  return { mode: form.secretMode as "keep" | "clear" };
}

export function buildCreateAccountPayload(form: AccountFormState): CreateAccountInput {
  return {
    id: form.id.trim(),
    name: form.name.trim(),
    driver: form.driver,
    enabled: form.enabled,
    transport: {
      baseUrl: form.baseUrl,
      username: form.username,
    },
    secret: buildSecretInput(form),
  };
}

export function buildUpdateAccountPayload(form: AccountFormState): UpdateAccountInput {
  return {
    name: form.name.trim(),
    driver: form.driver,
    enabled: form.enabled,
    transport: {
      baseUrl: form.baseUrl,
      username: form.username,
    },
    secret: buildSecretInput(form),
  };
}

export function statusTone(status: AccountOverview["status"]): string {
  switch (status) {
    case "ready":
      return "text-emerald-700 border-emerald-500/30 bg-emerald-500/10";
    case "syncing":
      return "text-blue-700 border-blue-500/30 bg-blue-500/10";
    case "degraded":
      return "text-amber-700 border-amber-500/30 bg-amber-500/10";
    case "authError":
      return "text-rose-700 border-rose-500/30 bg-rose-500/10";
    case "offline":
      return "text-orange-700 border-orange-500/30 bg-orange-500/10";
    case "disabled":
      return "text-zinc-600 border-zinc-500/30 bg-zinc-500/10";
  }
}
