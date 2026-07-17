/**
 * Links shown on the final onboarding step.
 *
 * The `pending` flag marks a link awaiting a real URL — it renders disabled and
 * is never opened. All current links are live.
 */
export interface OnboardingLink {
  label: string
  href: string
  /** Placeholder awaiting a real URL — rendered disabled, never opened. */
  pending?: boolean
}

export const SOCIAL_LINKS: OnboardingLink[] = [
  { label: 'GitHub', href: 'https://github.com/theoryzhenkov/posthaste' },
  { label: 'Discord', href: 'https://discord.gg/VZc4fUPpjG' },
  { label: 'Website', href: 'https://posthaste.theor.net' },
]

export const DONATION_LINK: OnboardingLink = {
  label: 'Buy me a coffee',
  href: 'https://buymeacoffee.com/theoryzhenkov',
}
