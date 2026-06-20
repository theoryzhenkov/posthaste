import { Loader2 } from 'lucide-react'
import { lazy, Suspense, type RefObject } from 'react'

import type { MarkdownComposerEditorHandle } from '../MarkdownComposerEditor'

const MarkdownComposerEditor = lazy(() =>
  import('../MarkdownComposerEditor').then((module) => ({
    default: module.MarkdownComposerEditor,
  })),
)

export function ComposeBodyEditor({
  bodyRef,
  isPreparingMessage,
  isReplyContextError,
  preparingLabel,
  onChange,
  value,
}: {
  bodyRef: RefObject<MarkdownComposerEditorHandle | null>
  isPreparingMessage: boolean
  isReplyContextError: boolean
  preparingLabel: string
  onChange: (value: string) => void
  value: string
}) {
  return (
    <div className="min-h-0 flex-1 bg-[color-mix(in_oklab,var(--background)_62%,transparent)]">
      {isPreparingMessage ? (
        <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
          <Loader2 size={16} className="animate-spin" />
          {preparingLabel}
        </div>
      ) : isReplyContextError ? (
        <div className="flex h-full items-center justify-center px-6 text-center text-sm text-destructive">
          Could not prepare this message. Close the composer and try again.
        </div>
      ) : (
        <Suspense
          fallback={
            <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
              <Loader2 size={16} className="animate-spin" />
              Loading editor...
            </div>
          }
        >
          <MarkdownComposerEditor
            ref={bodyRef}
            value={value}
            onChange={onChange}
            placeholder="Write Markdown"
          />
        </Suspense>
      )}
    </div>
  )
}
