/**
 * First-launch guided tour. Region-highlight style: a modal backdrop with a
 * spotlight ring around the step's anchor element and a positioned card. Steps
 * without an anchor (welcome / support) render a centered card over a dim.
 *
 * Mounted by `MailClient` once the mail UI is ready; persistence lives in
 * `./store`.
 */
import { useCallback, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import { openExternalUrl } from '@/desktop/runtime'
import { cn } from '@/lib/design/cn'
import { useEscapeToDismiss, useViewportRemeasure } from '@/lib/dom'
import { Button } from '@/components/ui/form/button'

import { DONATION_LINK, SOCIAL_LINKS, type OnboardingLink } from './links'
import { computeCardPosition, type Rect } from './position'
import { useOnboarding } from './useOnboarding'

const CARD_WIDTH = 340
const ANCHOR_PADDING = 6

export function OnboardingTour() {
  const { index, total, step, isFirst, isLast, next, back, finish } =
    useOnboarding()
  const cardRef = useRef<HTMLDivElement | null>(null)
  const [anchorRect, setAnchorRect] = useState<Rect | null>(null)
  const [cardPos, setCardPos] = useState<{ top: number; left: number } | null>(
    null,
  )

  const measure = useCallback(() => {
    const element = step.anchor
      ? document.querySelector<HTMLElement>(step.anchor)
      : null
    const domRect = element?.getBoundingClientRect() ?? null
    const padded: Rect | null = domRect
      ? {
          top: domRect.top - ANCHOR_PADDING,
          left: domRect.left - ANCHOR_PADDING,
          width: domRect.width + ANCHOR_PADDING * 2,
          height: domRect.height + ANCHOR_PADDING * 2,
        }
      : null
    setAnchorRect(padded)
    const card = cardRef.current
    setCardPos(
      computeCardPosition({
        anchor: padded,
        viewport: { width: window.innerWidth, height: window.innerHeight },
        card: {
          width: card?.offsetWidth ?? CARD_WIDTH,
          height: card?.offsetHeight ?? 200,
        },
      }),
    )
  }, [step.anchor])

  useLayoutEffect(() => {
    // Measure from the observer callback (not synchronously in the effect
    // body): observing the card fires an initial callback that positions it.
    const observer = new ResizeObserver(() => measure())
    if (cardRef.current) observer.observe(cardRef.current)
    return () => observer.disconnect()
  }, [measure])
  // Resize/scroll keep the spotlight + card aligned to the anchor; Escape
  // skips the tour (overlay dismissal primitive — the tour owns input).
  useViewportRemeasure(measure)
  useEscapeToDismiss(finish)

  if (typeof document === 'undefined') {
    return null
  }

  return createPortal(
    <div className="fixed inset-0 z-(--z-tooltip)" aria-live="polite">
      <div
        className="absolute inset-0"
        style={{ background: anchorRect ? 'transparent' : 'rgba(0,0,0,0.55)' }}
      />
      {anchorRect && (
        <div
          className="absolute rounded-lg ring-2 ring-[var(--ring)] transition-all duration-150"
          style={{
            top: anchorRect.top,
            left: anchorRect.left,
            width: anchorRect.width,
            height: anchorRect.height,
            boxShadow: '0 0 0 9999px rgba(0,0,0,0.55)',
            pointerEvents: 'none',
          }}
        />
      )}
      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-label="Posthaste tour"
        className="surface-floating absolute w-[340px] max-w-[calc(100vw-24px)] rounded-xl border p-4 text-foreground"
        style={
          cardPos
            ? { top: cardPos.top, left: cardPos.left }
            : { visibility: 'hidden' }
        }
      >
        <h2 className="text-[15px] font-semibold">{step.title}</h2>
        <p className="mt-1.5 text-[13px] leading-relaxed text-muted-foreground">
          {step.body}
        </p>

        {step.kind === 'final' && <LinksSection />}

        <div className="mt-4 flex items-center justify-between gap-3">
          <div className="flex items-center gap-1.5">
            {Array.from({ length: total }).map((_, dotIndex) => (
              <span
                key={dotIndex}
                aria-hidden
                className={cn(
                  'size-1.5 rounded-full transition-colors',
                  dotIndex === index ? 'bg-foreground' : 'bg-border',
                )}
              />
            ))}
          </div>
          <div className="flex items-center gap-2">
            {!isLast && (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="text-muted-foreground"
                onClick={finish}
              >
                Skip
              </Button>
            )}
            {!isFirst && (
              <Button type="button" size="sm" variant="outline" onClick={back}>
                Back
              </Button>
            )}
            <Button
              type="button"
              size="sm"
              className="bg-brand-coral text-white hover:bg-brand-coral/90"
              onClick={isLast ? finish : next}
            >
              {isLast ? 'Get started' : 'Next'}
            </Button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  )
}

function LinksSection() {
  return (
    <div className="mt-3 space-y-3">
      <div className="flex flex-wrap gap-1.5">
        {SOCIAL_LINKS.map((link) => (
          <LinkChip key={link.label} link={link} />
        ))}
      </div>
      <Button
        type="button"
        size="sm"
        variant="outline"
        disabled={DONATION_LINK.pending}
        title={DONATION_LINK.pending ? 'Coming soon' : undefined}
        className="w-full"
        onClick={() => {
          if (!DONATION_LINK.pending) void openExternalUrl(DONATION_LINK.href)
        }}
      >
        ♥ {DONATION_LINK.label}
        {DONATION_LINK.pending && (
          <span className="ml-1 text-[11px] text-muted-foreground">(soon)</span>
        )}
      </Button>
    </div>
  )
}

function LinkChip({ link }: { link: OnboardingLink }) {
  return (
    <button
      type="button"
      disabled={link.pending}
      title={link.pending ? 'Coming soon' : link.href}
      onClick={() => {
        if (!link.pending) void openExternalUrl(link.href)
      }}
      className={cn(
        'ph-focus-ring inline-flex items-center rounded-md border border-border-soft px-2 py-1 text-[12px] transition-colors',
        link.pending
          ? 'cursor-not-allowed text-muted-foreground/60'
          : 'text-foreground hover:bg-[var(--hover-bg)]',
      )}
    >
      {link.label}
    </button>
  )
}
