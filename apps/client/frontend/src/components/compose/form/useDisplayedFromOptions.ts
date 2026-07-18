import { useMemo } from 'react'

import { optionLabel, type FromAddressOption } from './model'

export function useDisplayedFromOptions({
  formFrom,
  fromMenuOpen,
  fromOptions,
}: {
  formFrom: string
  fromMenuOpen: boolean
  fromOptions: FromAddressOption[]
}) {
  return useMemo(() => {
    const needle = formFrom.trim().toLowerCase()
    if (fromMenuOpen || needle.length === 0) {
      return fromOptions
    }
    return fromOptions
      .filter((option) => {
        const label = optionLabel(option).toLowerCase()
        return (
          option.email.toLowerCase().includes(needle) ||
          option.sourceName.toLowerCase().includes(needle) ||
          label.includes(needle)
        )
      })
      .slice(0, 6)
  }, [formFrom, fromMenuOpen, fromOptions])
}
