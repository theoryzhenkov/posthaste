import type { AccountOverview } from '../../../api/types'
import type { AccountEditorConnectionModel } from '../accountEditorModel'
import { normalizeAccountInitials } from '../helpers'
import type { AccountFormState } from '../types'

export const accountHueGradient =
  'linear-gradient(90deg, oklch(0.68 0.17 0), oklch(0.68 0.17 45), oklch(0.68 0.17 90), oklch(0.68 0.17 145), oklch(0.68 0.17 205), oklch(0.68 0.17 260), oklch(0.68 0.17 315), oklch(0.68 0.17 360))'

export function accountAppearanceSignature(
  appearance: AccountOverview['appearance'],
): string {
  const imagePart = appearance.kind === 'image' ? appearance.imageId : ''
  return `${appearance.kind}:${appearance.initials}:${appearance.colorHue}:${imagePart}`
}

export function appearanceFromForm(
  form: AccountFormState,
): AccountOverview['appearance'] {
  return {
    kind: 'initials',
    initials: normalizeAccountInitials(form.appearanceInitials || form.name),
    colorHue: Math.min(360, Math.max(0, Math.round(form.appearanceColorHue))),
  }
}

export function accountFieldsSignature(
  form: AccountFormState,
  connection: AccountEditorConnectionModel,
): string {
  const signature = {
    name: form.name.trim(),
    fullName: form.fullName.trim(),
    signature: form.signature.trim(),
    emailPatternsText: form.emailPatternsText.trim(),
  }
  if (connection.kind === 'managedOAuth') {
    return JSON.stringify(signature)
  }
  return JSON.stringify({
    ...signature,
    driver: form.driver,
    baseUrl: form.baseUrl.trim(),
    username: form.username.trim(),
    imapHost: form.imapHost.trim(),
    imapPort: form.imapPort.trim(),
    imapSecurity: form.imapSecurity,
    smtpHost: form.smtpHost.trim(),
    smtpPort: form.smtpPort.trim(),
    smtpSecurity: form.smtpSecurity,
    passwordChanged: form.password.trim().length > 0,
  })
}
