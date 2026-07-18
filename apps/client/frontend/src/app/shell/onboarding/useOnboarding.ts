import { useCallback, useState } from 'react'

import { markOnboardingComplete } from './store'
import { ONBOARDING_STEPS } from './steps'

/**
 * Tour step machine. Intended to be mounted only while the tour is needed (see
 * `MailClient`), so it always starts at the first step and unmounts on finish —
 * no reset effect, and a replay is a fresh mount.
 */
export function useOnboarding() {
  const [index, setIndex] = useState(0)
  const total = ONBOARDING_STEPS.length
  const clampedIndex = Math.min(index, total - 1)

  const next = useCallback(() => {
    setIndex((current) => Math.min(current + 1, total - 1))
  }, [total])
  const back = useCallback(() => {
    setIndex((current) => Math.max(0, current - 1))
  }, [])
  const finish = useCallback(() => {
    markOnboardingComplete()
  }, [])

  return {
    index: clampedIndex,
    total,
    step: ONBOARDING_STEPS[clampedIndex],
    isFirst: clampedIndex === 0,
    isLast: clampedIndex === total - 1,
    next,
    back,
    finish,
  }
}
