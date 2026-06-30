import { useLayoutEffect } from 'react'
import type { HomeContent } from './content/types'
import { FooterSection, InstallHeader } from './SiteChrome'
import { LandscapeValuesSection } from './HomeSections'
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
      <InstallHeader />
      <Hero messages={content.messages} />
      <LandscapeValuesSection content={content.openSource} />
      <FooterSection content={content.footer} />
    </main>
  )
}
