/**
 * Core button component with CVA variant support.
 * @spec docs/L0-branding#color-palette-light-mode-primary
 */
import * as React from 'react'
import { type VariantProps } from 'class-variance-authority'
import { Slot } from 'radix-ui'

import { cn } from '@/lib/utils'
import { buttonVariants } from './button-variants'

/**
 * Polymorphic button with variant and size props driven by `buttonVariants`.
 * Supports `asChild` for rendering as a Radix `Slot`.
 */
function Button({
  className,
  variant = 'default',
  size = 'default',
  asChild = false,
  ...props
}: React.ComponentProps<'button'> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot.Root : 'button'

  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Button }
