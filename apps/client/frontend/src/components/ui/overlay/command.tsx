import { Command as CommandPrimitive } from 'cmdk'
import { Search } from 'lucide-react'
import * as React from 'react'

import { cn } from '@/lib/design/cn'

function Command({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive>) {
  return (
    <CommandPrimitive
      data-slot="command"
      className={cn(
        'flex h-full w-full flex-col overflow-hidden rounded-[inherit] bg-transparent text-foreground',
        className,
      )}
      {...props}
    />
  )
}

function CommandInput({
  className,
  wrapperClassName,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Input> & {
  wrapperClassName?: string
}) {
  return (
    <div
      data-slot="command-input-wrapper"
      className={cn('flex items-center gap-3', wrapperClassName)}
    >
      <Search
        className="size-4 shrink-0 text-muted-foreground"
        strokeWidth={1.75}
      />
      <CommandPrimitive.Input
        data-slot="command-input"
        className={cn(
          'flex h-12 w-full rounded-none border-0 bg-transparent text-[15px] outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50',
          className,
        )}
        {...props}
      />
    </div>
  )
}

function CommandList({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.List>) {
  return (
    <CommandPrimitive.List
      data-slot="command-list"
      className={cn(
        'max-h-[26rem] overflow-x-hidden overflow-y-auto',
        className,
      )}
      {...props}
    />
  )
}

function CommandItem({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Item>) {
  return (
    <CommandPrimitive.Item
      data-slot="command-item"
      className={cn(
        'relative flex cursor-default select-none items-center gap-3 px-4 py-2.5 text-sm outline-none data-[disabled=true]:pointer-events-none data-[disabled=true]:opacity-40 data-[selected=true]:bg-white/8',
        className,
      )}
      {...props}
    />
  )
}

export { Command, CommandInput, CommandItem, CommandList }
