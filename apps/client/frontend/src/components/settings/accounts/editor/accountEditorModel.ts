import type { AccountSettingsResult } from '@/gen'
import type { EditorTarget } from '../../panel/types'

/** Whether the account's credentials are user-entered or provider-managed
 * (OAuth). Derived from the transport's auth kind — the secrets-safe
 * settings view carries no separate discriminator. */
interface ManualCredentialsConnectionModel {
  kind: 'manualCredentials'
  account: AccountSettingsResult | null
}

export interface ManagedOAuthConnectionModel {
  kind: 'managedOAuth'
  account: AccountSettingsResult
}

export type AccountEditorConnectionModel =
  | ManualCredentialsConnectionModel
  | ManagedOAuthConnectionModel

interface NewAccountEditorModel {
  kind: 'new'
  connection: ManualCredentialsConnectionModel
}

export interface ExistingAccountEditorModel {
  kind: 'existing'
  account: AccountSettingsResult
  connection: AccountEditorConnectionModel
}

export type AccountEditorModel =
  | NewAccountEditorModel
  | ExistingAccountEditorModel

export function buildAccountEditorModel(
  editorTarget: EditorTarget,
  editingAccount: AccountSettingsResult | null,
): AccountEditorModel {
  if (editorTarget === 'new' || editingAccount === null) {
    return {
      kind: 'new',
      connection: { kind: 'manualCredentials', account: null },
    }
  }

  const connection: AccountEditorConnectionModel =
    editingAccount.transport.auth === 'oauth2'
      ? { kind: 'managedOAuth', account: editingAccount }
      : { kind: 'manualCredentials', account: editingAccount }

  return { kind: 'existing', account: editingAccount, connection }
}
