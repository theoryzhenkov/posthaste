import type { ExistingAccountEditorModel } from '../accountEditorModel'
import { statusLabel } from '../helpers/accountStatus'
import { StatusDot } from '../shared'
import { authLabel, providerLabel } from './labels'

export function AccountHeaderMeta({
  model,
}: {
  model: ExistingAccountEditorModel
}) {
  return (
    <>
      <StatusDot status={model.account.runtime.status} className="size-1.5" />
      <span>{statusLabel(model.account.runtime.status)}</span>
      <span aria-hidden>·</span>
      <span>{providerLabel(model.account.connection.providerKind)}</span>
      <span aria-hidden>·</span>
      <span>{authLabel(model.account.connection.auth)}</span>
    </>
  )
}
