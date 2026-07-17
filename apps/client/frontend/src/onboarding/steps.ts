/**
 * Brief first-launch tour. Most of Posthaste is intuitive, so the tour only
 * points out the two genuinely-hidden affordances (command search + keyboard
 * shortcuts) plus the conversation-view toggle, bookended by a welcome and a
 * support/socials card.
 */
export interface OnboardingStep {
  id: string
  /** `center` cards dim the screen; `highlight` cards spotlight an anchor. */
  kind: 'center' | 'highlight' | 'final'
  title: string
  body: string
  /** CSS selector for the element to spotlight (highlight steps only). */
  anchor?: string
}

export const ONBOARDING_STEPS: OnboardingStep[] = [
  {
    id: 'welcome',
    kind: 'center',
    title: 'Welcome to Posthaste',
    body: 'A fast, keyboard-friendly email client. Here are a few things worth knowing — it takes about twenty seconds.',
  },
  {
    id: 'conversation-view',
    kind: 'highlight',
    anchor: '[data-tour-anchor="conversation-view"]',
    title: 'Messages or conversations',
    body: 'Switch any view between a flat message list and a conversation tree. The choice is remembered per view.',
  },
  {
    id: 'command-search',
    kind: 'highlight',
    anchor: '[data-command-search-trigger="true"]',
    title: 'Command search',
    body: 'Press / (or click here) to search your mail and run commands. Enter dives into the results; Shift+Enter filters the whole view.',
  },
  {
    id: 'shortcuts',
    kind: 'highlight',
    anchor: '[data-shortcut-reference-trigger="true"]',
    title: 'Keyboard shortcuts',
    body: 'Press ? at any time to see every shortcut. Posthaste is built to be driven from the keyboard.',
  },
  {
    id: 'support',
    kind: 'final',
    title: "That's it!",
    body: 'Posthaste is built by a small team. If you’d like to follow along or support the project:',
  },
]
