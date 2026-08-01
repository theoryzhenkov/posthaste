/** Merge Tailwind class names with `clsx` and `tailwind-merge`. */
import { clsx, type ClassValue } from 'clsx'
import { extendTailwindMerge } from 'tailwind-merge'

/**
 * The type scale's utilities (`text-body`, `text-meta`, …) are this project's
 * own `@theme` names and are unknown to tailwind-merge, which reads an
 * unfamiliar `text-*` as a text COLOUR. `cn('text-body', 'text-foreground')`
 * then looks like a conflict and silently drops the size — the failure is
 * invisible in the source and only shows up as a row rendering at the wrong
 * size. Naming the five here keeps size and colour in separate groups, as
 * they are for the built-in sizes.
 */
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      'font-size': [{ text: ['meta', 'ui', 'body', 'emphasis', 'heading'] }],
    },
  },
})

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
