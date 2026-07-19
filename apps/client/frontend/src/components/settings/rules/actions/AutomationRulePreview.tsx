import type { MessageSummary } from '../../../../data/transport/api/index'
import { formatRelativeTime } from '../../../../lib/ambient/time'
import { Button } from '../../../ui/form/button'
import { FeedbackBanner } from '../../panel/shared'

export function AutomationRulePreview({
  accountId,
  preview,
  error,
  isPending,
  onPreview,
}: {
  accountId: string
  preview: { total: number; items: MessageSummary[] } | null
  error: string | null
  isPending: boolean
  onPreview: () => void
}) {
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="text-[12px] font-medium text-muted-foreground">
            Matching messages
          </p>
          {preview && (
            <p className="text-[12px] text-muted-foreground">
              {preview.total} {preview.total === 1 ? 'message' : 'messages'}{' '}
              match this rule.
            </p>
          )}
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 rounded-md border-border bg-background px-2 text-[12px]"
          disabled={isPending || accountId.trim().length === 0}
          onClick={onPreview}
        >
          {isPending ? 'Checking' : 'Preview'}
        </Button>
      </div>

      {error && <FeedbackBanner tone="error">{error}</FeedbackBanner>}

      {preview && (
        <div className="overflow-hidden rounded-lg border border-border-soft bg-bg-elev/25">
          {preview.items.length === 0 ? (
            <p className="px-3 py-2 text-[12px] text-muted-foreground">
              No synced messages match this rule.
            </p>
          ) : (
            preview.items.map((message) => (
              <AutomationRulePreviewRow key={message.id} message={message} />
            ))
          )}
        </div>
      )}
    </div>
  )
}

function AutomationRulePreviewRow({ message }: { message: MessageSummary }) {
  const sender = message.fromName ?? message.fromEmail ?? 'Unknown sender'
  return (
    <div className="grid min-h-11 grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-border-soft px-3 py-2 last:border-b-0">
      <div className="min-w-0">
        <p className="truncate text-[12px] font-medium text-foreground">
          {message.subject?.trim() || '(no subject)'}
        </p>
        <p className="truncate text-[12px] text-muted-foreground">{sender}</p>
      </div>
      <p className="shrink-0 text-[11px] text-muted-foreground">
        {formatRelativeTime(message.receivedAt)}
      </p>
    </div>
  )
}
