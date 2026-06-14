import type { ExistingAccountEditorModel } from '../accountEditorModel'
import { StatusDot } from '../shared'
import { authLabel, providerLabel } from './labels'

export function AccountHeaderMeta({ model }: { model: ExistingAccountEditorModel }) {
  return (
    <>
      <StatusDot status={model.account.status} className="size-1.5" />
      <span className="font-mono uppercase tracking-[0.12em]">
        {model.account.status}
      </span>
      <span aria-hidden>·</span>
      <span>{providerLabel(model.account.connection.providerKind)}</span>
      <span aria-hidden>·</span>
      <span>{authLabel(model.account.connection.auth)}</span>
    </>
  )
}
