import type {
  AccountOverview,
  CreateAccountInput,
  UpdateAccountInput,
} from '../../../api/types'
import type { ExistingAccountEditorModel } from '../accountEditorModel'
import type { AccountFormState } from '../types'

/** Default empty form state for creating a new account. */
export const EMPTY_FORM: AccountFormState = {
  name: '',
  fullName: '',
  emailPatternsText: '',
  appearanceInitials: 'A',
  appearanceColorHue: 0,
  baseUrl: '',
  username: '',
  password: '',
}

export function emptyAccountForm(): AccountFormState {
  return {
    ...EMPTY_FORM,
    appearanceColorHue: Math.floor(Math.random() * 361),
  }
}

/** Convert an existing account overview into editable form state. */
export function formFromAccount(account: AccountOverview): AccountFormState {
  return {
    name: account.name,
    fullName: account.fullName ?? '',
    emailPatternsText: account.emailPatterns?.join('\n') ?? '',
    appearanceInitials: normalizeAccountInitials(account.appearance.initials),
    appearanceColorHue: account.appearance.colorHue,
    baseUrl:
      account.connection.kind === 'manualCredentials'
        ? (account.connection.baseUrl ?? '')
        : '',
    username: account.connection.username ?? '',
    password: '',
  }
}

/**
 * Build a secret instruction payload from the current form state.
 * @spec docs/L1-api#secret-management
 */
export function buildSecretInput(form: AccountFormState) {
  if (form.password.trim() !== '') {
    return { mode: 'replace' as const, password: form.password }
  }
  return { mode: 'keep' as const }
}

/** Parse newline/comma-separated addresses and catch-all patterns. */
export function parseEmailPatterns(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((pattern) => pattern.trim())
    .filter((pattern) => pattern.length > 0)
}

/** Build a create-account API payload from form state. */
export function buildCreateAccountPayload(
  form: AccountFormState,
): CreateAccountInput {
  return {
    name: form.name.trim(),
    fullName: form.fullName.trim() || null,
    emailPatterns: parseEmailPatterns(form.emailPatternsText),
    driver: 'jmap',
    enabled: true,
    appearance: buildAccountAppearanceInput(form),
    transport: {
      baseUrl: form.baseUrl,
      username: form.username,
    },
    secret: buildSecretInput(form),
  }
}

/**
 * Build an update-account API payload from form state.
 * @spec docs/L1-api#account-crud-lifecycle
 */
export function buildUpdateAccountPayload(
  form: AccountFormState,
  editorModel: ExistingAccountEditorModel,
): UpdateAccountInput {
  const input: UpdateAccountInput = {
    name: form.name.trim(),
    fullName: form.fullName.trim() || null,
    emailPatterns: parseEmailPatterns(form.emailPatternsText),
    appearance: buildAccountAppearanceInput(form),
  }

  switch (editorModel.connection.kind) {
    case 'managedOAuth':
      return input
    case 'manualCredentials':
      return {
        ...input,
        transport: {
          baseUrl: form.baseUrl,
          username: form.username,
        },
        secret: buildSecretInput(form),
      }
  }
}

export function buildAccountAppearanceInput(
  form: AccountFormState,
): CreateAccountInput['appearance'] {
  const initials = normalizeAccountInitials(
    form.appearanceInitials || form.name,
  )
  const colorHue = Math.min(
    360,
    Math.max(0, Math.round(form.appearanceColorHue)),
  )
  return {
    kind: 'initials',
    initials,
    colorHue,
  }
}

export function normalizeAccountInitials(value: string): string {
  const trimmed = value.trim().toUpperCase()
  return trimmed.length === 0 ? 'A' : Array.from(trimmed).slice(0, 1).join('')
}
