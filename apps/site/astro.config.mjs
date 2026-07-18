import react from '@astrojs/react'
import starlight from '@astrojs/starlight'
import { defineConfig } from 'astro/config'
import rehypeDocLinks from './rehype-doc-links.mjs'

// Posthaste site. The bespoke marketing landing (`/`, `/releases`)
// is served by `src/pages/*.astro`; Starlight owns everything under `/docs`.
// The two coexist because every Starlight entry id is prefixed with `docs/`
// (see `src/content.config.ts`), so Starlight's routes never collide with the
// hand-built pages.
export default defineConfig({
  site: 'https://posthaste.theor.net',
  integrations: [
    react(),
    starlight({
      title: 'Posthaste',
      description:
        'JMAP mail client with MailMate-grade search and a conversation-first web UI.',
      logo: { src: './public/posthaste-logo.svg', alt: 'Posthaste' },
      favicon: '/favicon.svg',
      // Built-in search (Pagefind), TOC and prev/next stay on; last-updated and
      // edit links are off because the sources live outside `src/content/docs`.
      lastUpdated: false,
      credits: false,
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/theoryzhenkov/posthaste',
        },
        {
          icon: 'discord',
          label: 'Discord',
          href: 'https://discord.gg/8ARFrDa2Gv',
        },
      ],
      customCss: [
        '@fontsource-variable/geist/index.css',
        '@fontsource-variable/geist-mono/index.css',
        './src/styles/starlight-theme.css',
      ],
      components: {
        SiteTitle: './src/components/docs/SiteTitle.astro',
        Header: './src/components/docs/Header.astro',
        Footer: './src/components/docs/Footer.astro',
      },
      sidebar: [
        {
          label: 'Guide',
          items: [
            { label: 'Start here', slug: 'docs' },
            { label: 'Automations', slug: 'docs/automations' },
            { label: 'Plug in an agent', slug: 'docs/agents' },
            {
              label: 'Scripting quickstart',
              slug: 'docs/scripting-quickstart',
            },
            { label: 'Security', slug: 'docs/scripting-security' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'Overview', slug: 'docs/reference' },
            {
              label: 'Architecture',
              items: [
                {
                  label: 'Architecture (L1)',
                  slug: 'docs/architecture/L1-architecture',
                },
                {
                  label: 'Crate topology (L2)',
                  slug: 'docs/architecture/L2-crate-topology',
                },
              ],
            },
            {
              label: 'API',
              items: [{ label: 'API (L1)', slug: 'docs/api/L1-api' }],
            },
            {
              label: 'Backend',
              items: [
                { label: 'Backend (L1)', slug: 'docs/backend/L1-backend' },
              ],
            },
            {
              label: 'Client',
              items: [{ label: 'Client (L1)', slug: 'docs/client/L1-client' }],
            },
            {
              label: 'State',
              items: [
                { label: 'Model (L1)', slug: 'docs/state/mail/L1' },
                { label: 'Implementation (L2)', slug: 'docs/state/mail/L2' },
                {
                  label: 'Implementation patterns (L3)',
                  slug: 'docs/state/mail/L3',
                },
              ],
            },
            {
              label: 'UI',
              items: [
                { label: 'Navigation model (L0)', slug: 'docs/ui/L0' },
                { label: 'Keyboard contract (L1)', slug: 'docs/ui/L1' },
              ],
            },
            {
              label: 'Testing',
              items: [
                { label: 'Domain (L0)', slug: 'docs/testing/L0' },
                { label: 'Contracts (L1)', slug: 'docs/testing/L1' },
              ],
            },
          ],
        },
      ],
    }),
  ],
  markdown: {
    rehypePlugins: [rehypeDocLinks],
  },
})
