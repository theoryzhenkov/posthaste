import type {
  AccountDriver,
  SmartMailboxRule,
} from "../../api/types";

export type EditorTarget = "new" | string;
export type SmartMailboxEditorTarget = "new" | string;
export type SecretMode = "keep" | "replace" | "clear";

export interface AccountFormState {
  id: string;
  name: string;
  driver: AccountDriver;
  enabled: boolean;
  baseUrl: string;
  username: string;
  password: string;
  secretMode: SecretMode;
}

export interface SmartMailboxFormState {
  name: string;
  position: number;
  rule: SmartMailboxRule;
}
