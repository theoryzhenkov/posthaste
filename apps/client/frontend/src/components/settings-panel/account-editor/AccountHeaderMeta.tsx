import type { AccountRow } from '@/gen'
import type { ExistingAccountEditorModel } from '../accountEditorModel'
import { statusLabel } from '../helpers/accountStatus'
import { StatusDot } from '../shared'
import { authLabel, providerLabel } from './labels'

/** Provider/auth come from the settings answer; live status from the
 * accounts row (absent while the accounts list is still loading). */
export function AccountHeaderMeta({
  model,
  row,
}: {
  model: ExistingAccountEditorModel
  row: AccountRow | null
}) {
  return (
    <>
      {row && (
        <>
          <StatusDot status={row.status} className="size-1.5" />
          <span>{statusLabel(row.status)}</span>
          <span aria-hidden>·</span>
        </>
      )}
      <span>{providerLabel(model.account.transport.provider)}</span>
      <span aria-hidden>·</span>
      <span>{authLabel(model.account.transport.auth)}</span>
    </>
  )
}
