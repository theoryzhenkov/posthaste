import { useEffect, useState } from 'react'

type LandscapePhase = 'night' | 'morning' | 'day' | 'evening'

interface LandscapeTimeState {
  phase: LandscapePhase
  celestialX: number
  celestialY: number
}

const initialLandscapeTimeState: LandscapeTimeState = {
  phase: 'day',
  celestialX: 41,
  celestialY: 22,
}

function getLandscapeTimeState(date = new Date()): LandscapeTimeState {
  const minutes = date.getHours() * 60 + date.getMinutes()
  const sunrise = 6 * 60
  const morningEnd = 10 * 60
  const eveningStart = 17 * 60
  const sunset = 20 * 60

  let phase: LandscapePhase = 'night'

  if (minutes >= sunrise && minutes < morningEnd) {
    phase = 'morning'
  } else if (minutes >= morningEnd && minutes < eveningStart) {
    phase = 'day'
  } else if (minutes >= eveningStart && minutes < sunset) {
    phase = 'evening'
  }

  if (phase === 'night') {
    const nightStart = sunset
    const nightLength = 10 * 60
    const nightMinutes =
      minutes >= nightStart
        ? minutes - nightStart
        : minutes + 24 * 60 - nightStart
    const progress = nightMinutes / nightLength

    return {
      phase,
      celestialX: 8 + progress * 84,
      celestialY: 66 - Math.sin(progress * Math.PI) * 34,
    }
  }

  const dayProgress = Math.max(
    0,
    Math.min(1, (minutes - sunrise) / (sunset - sunrise)),
  )

  return {
    phase,
    celestialX: 8 + dayProgress * 66,
    celestialY: 72 - Math.sin(dayProgress * Math.PI) * 50,
  }
}

export function useLandscapeTime() {
  const [timeState, setTimeState] = useState(initialLandscapeTimeState)

  useEffect(() => {
    const initial = window.setTimeout(() => {
      setTimeState(getLandscapeTimeState())
    }, 0)
    const interval = window.setInterval(() => {
      setTimeState(getLandscapeTimeState())
    }, 60_000)

    return () => {
      window.clearTimeout(initial)
      window.clearInterval(interval)
    }
  }, [])

  return timeState
}

export function useReveal() {
  useEffect(() => {
    const reduceMotion = window.matchMedia(
      '(prefers-reduced-motion: reduce)',
    ).matches

    if (reduceMotion) {
      document
        .querySelectorAll<HTMLElement>('[data-reveal]')
        .forEach((element) => element.classList.add('is-visible'))
      return
    }

    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add('is-visible')
          }
        })
      },
      { rootMargin: '0px 0px -10% 0px', threshold: 0.18 },
    )

    document
      .querySelectorAll<HTMLElement>('[data-reveal]')
      .forEach((element) => observer.observe(element))

    return () => observer.disconnect()
  }, [])
}
