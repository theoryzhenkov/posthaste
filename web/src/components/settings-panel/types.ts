/**
 * Shared types for the settings panel editor components.
 * @spec spec/L1-api#account-crud-lifecycle
 * @spec spec/L1-api#smart-mailbox-crud
 */
import type {
  AccountDriver,
  SmartMailboxRule,
} from "../../api/types";

/** Editor target: `"new"` for create mode, or an existing entity ID. */
export type EditorTarget = "new" | string;
/** Smart mailbox editor target: `"new"` for create mode, or an existing mailbox ID. */
export type SmartMailboxEditorTarget = "new" | string;
/**
 * Secret write mode tri-state.
 * @spec spec/L1-api#secret-management
 */
export type SecretMode = "keep" | "replace" | "clear";

/** @spec spec/L1-api#account-crud-lifecycle */
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

/** @spec spec/L1-api#smart-mailbox-crud */
export interface SmartMailboxFormState {
  name: string;
  position: number;
  rule: SmartMailboxRule;
}
