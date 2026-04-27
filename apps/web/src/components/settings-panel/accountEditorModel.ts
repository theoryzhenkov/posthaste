import type {
  AccountOverview,
  ManagedOAuthAccountConnectionOverview,
  ManualCredentialsAccountConnectionOverview,
} from '../../api/types'
import type { EditorTarget } from './types'

export type ManualCredentialAccountOverview = AccountOverview & {
  connection: ManualCredentialsAccountConnectionOverview
}

export type ManagedOAuthAccountOverview = AccountOverview & {
  connection: ManagedOAuthAccountConnectionOverview
}

export interface ManualCredentialsConnectionModel {
  kind: 'manualCredentials'
  account: ManualCredentialAccountOverview | null
}

export interface ManagedOAuthConnectionModel {
  kind: 'managedOAuth'
  account: ManagedOAuthAccountOverview
}

export type AccountEditorConnectionModel =
  | ManualCredentialsConnectionModel
  | ManagedOAuthConnectionModel

export interface NewAccountEditorModel {
  kind: 'new'
  connection: ManualCredentialsConnectionModel
}

export interface ExistingManualAccountEditorModel {
  kind: 'existingManualCredentials'
  account: ManualCredentialAccountOverview
  connection: ManualCredentialsConnectionModel
}

export interface ExistingManagedOAuthAccountEditorModel {
  kind: 'existingManagedOAuth'
  account: ManagedOAuthAccountOverview
  connection: ManagedOAuthConnectionModel
}

export type ExistingAccountEditorModel =
  | ExistingManualAccountEditorModel
  | ExistingManagedOAuthAccountEditorModel

export type AccountEditorModel =
  | NewAccountEditorModel
  | ExistingAccountEditorModel

export function buildAccountEditorModel(
  editorTarget: EditorTarget,
  editingAccount: AccountOverview | null,
): AccountEditorModel {
  if (editorTarget === 'new' || editingAccount === null) {
    return {
      kind: 'new',
      connection: {
        kind: 'manualCredentials',
        account: null,
      },
    }
  }

  if (isManagedOAuthAccount(editingAccount)) {
    return {
      kind: 'existingManagedOAuth',
      account: editingAccount,
      connection: {
        kind: 'managedOAuth',
        account: editingAccount,
      },
    }
  }

  if (isManualCredentialsAccount(editingAccount)) {
    return {
      kind: 'existingManualCredentials',
      account: editingAccount,
      connection: {
        kind: 'manualCredentials',
        account: editingAccount,
      },
    }
  }

  throw new Error(
    `Unsupported account connection kind: ${editingAccount.connection.kind}`,
  )
}

function isManagedOAuthAccount(
  account: AccountOverview,
): account is ManagedOAuthAccountOverview {
  return account.connection.kind === 'managedOAuth'
}

function isManualCredentialsAccount(
  account: AccountOverview,
): account is ManualCredentialAccountOverview {
  return account.connection.kind === 'manualCredentials'
}
