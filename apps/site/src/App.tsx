import { useLayoutEffect } from 'react'
import type { HomeContent } from './content/types'
import { FooterSection, InstallHeader } from './SiteChrome'
import {
  LandscapeValuesSection,
  ScreenshotsSection,
  WizardSection,
} from './HomeSections'
import { useReveal } from './hooks'
import { Hero } from './mail-mock/Hero'

export function App({ content }: { content: HomeContent }) {
  useLayoutEffect(() => {
    document.documentElement.classList.add('mail-mock-hydrated')

    return () => {
      document.documentElement.classList.remove('mail-mock-hydrated')
    }
  }, [])
  useReveal()

  return (
    <main className="site-shell">
      <InstallHeader active="home" />
      <Hero messages={content.messages} />
      <ScreenshotsSection />
      <WizardSection />
      <LandscapeValuesSection content={content.openSource} />
      <FooterSection content={content.footer} />
    </main>
  )
}
