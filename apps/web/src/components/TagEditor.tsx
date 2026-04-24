import { Tag, X } from 'lucide-react'
import { useMemo, useState } from 'react'

import type { MessageDetail, TagSummary } from '@/api/types'
import type { EmailActions } from '@/hooks/useEmailActions'
import { cn } from '@/lib/utils'

import { FloatingPanel } from './FloatingPanel'

interface TagEditorProps {
  actions: EmailActions
  knownTags: TagSummary[]
  message: MessageDetail
  onClose: () => void
}

const TAG_PANEL_STORAGE_KEY = 'posthaste.tags.panelOffset'

function userTags(keywords: string[]): string[] {
  return keywords.filter((keyword) => !keyword.startsWith('$'))
}

function normalizeTag(value: string): string | null {
  const normalized = value.trim().replace(/\s+/g, ' ')
  if (!normalized || normalized.startsWith('$') || normalized.includes('/')) {
    return null
  }
  return normalized
}

function hasTag(tags: string[], tag: string): boolean {
  return tags.some((candidate) => candidate.toLowerCase() === tag.toLowerCase())
}

export function TagEditor({
  actions,
  knownTags,
  message,
  onClose,
}: TagEditorProps) {
  const [draft, setDraft] = useState('')
  const tags = useMemo(() => userTags(message.keywords), [message.keywords])
  const suggestions = useMemo(
    () =>
      knownTags
        .filter((tag) => !hasTag(tags, tag.name))
        .filter((tag) => {
          const query = draft.trim().toLowerCase()
          return !query || tag.name.toLowerCase().includes(query)
        })
        .slice(0, 6),
    [draft, knownTags, tags],
  )

  function setTags(nextTags: string[]) {
    actions.setUserTags(
      {
        conversationId: message.conversationId,
        sourceId: message.sourceId,
        messageId: message.id,
        isFlagged: message.isFlagged,
        isRead: message.isRead,
        keywords: message.keywords,
      },
      nextTags,
    )
  }

  function addTag(value: string) {
    const tag = normalizeTag(value)
    if (!tag || hasTag(tags, tag)) {
      setDraft('')
      return
    }
    setTags([...tags, tag])
    setDraft('')
  }

  function removeTag(tag: string) {
    setTags(tags.filter((candidate) => candidate !== tag))
  }

  return (
    <FloatingPanel
      panelLabel="tag editor"
      storageKey={TAG_PANEL_STORAGE_KEY}
      closeIgnoreSelector="[data-tag-editor-trigger='true']"
      className="max-w-sm"
      header={
        <div className="flex h-12 min-w-0 items-center gap-2 px-3">
          <Tag
            size={15}
            strokeWidth={1.7}
            className="shrink-0 text-muted-foreground"
          />
          <span className="truncate text-sm font-semibold">Tags</span>
        </div>
      }
      onClose={onClose}
    >
      <div className="space-y-4 p-4">
        <form
          onSubmit={(event) => {
            event.preventDefault()
            addTag(draft)
          }}
        >
          <input
            autoFocus
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Add tag"
            className="ph-focus-ring h-9 w-full rounded-[6px] border border-border bg-background px-3 text-sm outline-none placeholder:text-muted-foreground"
          />
        </form>

        <div className="flex min-h-7 flex-wrap gap-1.5">
          {tags.length === 0 ? (
            <span className="text-sm text-muted-foreground">No tags</span>
          ) : (
            tags.map((tag) => (
              <span
                key={tag}
                className="inline-flex h-7 items-center gap-1.5 rounded-[5px] border border-border bg-[var(--bg-elev)] px-2 font-mono text-[11px] font-semibold"
              >
                {tag}
                <button
                  type="button"
                  aria-label={`Remove ${tag}`}
                  className="ph-focus-ring -mr-1 flex size-5 items-center justify-center rounded-[4px] text-muted-foreground hover:bg-[var(--hover-bg)] hover:text-foreground"
                  onClick={() => removeTag(tag)}
                >
                  <X size={12} strokeWidth={1.8} />
                </button>
              </span>
            ))
          )}
        </div>

        {suggestions.length > 0 && (
          <div className="space-y-1">
            {suggestions.map((tag) => (
              <button
                key={tag.name}
                type="button"
                className={cn(
                  'ph-focus-ring flex h-8 w-full items-center gap-2 rounded-[5px] px-2 text-left text-sm transition-colors',
                  'hover:bg-[var(--hover-bg)]',
                )}
                onClick={() => addTag(tag.name)}
              >
                <span className="min-w-0 flex-1 truncate">{tag.name}</span>
                {tag.unreadMessages > 0 && (
                  <span className="font-mono text-[11px] text-muted-foreground">
                    {tag.unreadMessages}
                  </span>
                )}
              </button>
            ))}
          </div>
        )}
      </div>
    </FloatingPanel>
  )
}
