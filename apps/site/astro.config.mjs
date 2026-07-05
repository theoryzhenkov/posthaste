import react from '@astrojs/react'
import starlight from '@astrojs/starlight'
import { defineConfig } from 'astro/config'
import rehypeDocLinks from './rehype-doc-links.mjs'

// Posthaste site. The bespoke marketing landing (`/`, `/releases`, `/wizard`)
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
                  label: 'Crate topology (L2)',
                  slug: 'docs/architecture/L2-crate-topology',
                },
              ],
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
              label: 'Runtime',
              items: [
                { label: 'Contract (L1)', slug: 'docs/runtime/L1' },
                {
                  label: 'Adapter — Surface (L1)',
                  slug: 'docs/runtime/adapter/L1',
                },
                {
                  label: 'Adapter — Sessions & views (L2)',
                  slug: 'docs/runtime/adapter/L2',
                },
                {
                  label: 'Adapter — Patterns (L3)',
                  slug: 'docs/runtime/adapter/L3',
                },
                {
                  label: 'Mutations — Catalog (L1)',
                  slug: 'docs/runtime/mutations/L1',
                },
                {
                  label: 'Mutations — Pipeline (L2)',
                  slug: 'docs/runtime/mutations/L2',
                },
                {
                  label: 'Internals — Contract (L1)',
                  slug: 'docs/runtime/internals/L1',
                },
                {
                  label: 'Internals — Assembly (L2)',
                  slug: 'docs/runtime/internals/L2',
                },
                {
                  label: 'Internals — Patterns (L3)',
                  slug: 'docs/runtime/internals/L3',
                },
              ],
            },
            {
              label: 'Replication',
              items: [
                { label: 'Coherent links (L1)', slug: 'docs/replication/L1' },
                {
                  label: 'Client link — Seam (L1)',
                  slug: 'docs/replication/client-link/L1',
                },
                {
                  label: 'Client link — Components (L2)',
                  slug: 'docs/replication/client-link/L2',
                },
                {
                  label: 'Client link — Integration (L3)',
                  slug: 'docs/replication/client-link/L3',
                },
                {
                  label: 'Authority-server link — Seam (L1)',
                  slug: 'docs/replication/authority-server-link/L1',
                },
                {
                  label: 'Authority-server link — Components (L2)',
                  slug: 'docs/replication/authority-server-link/L2',
                },
                {
                  label: 'Authority-server link — Implementation (L3)',
                  slug: 'docs/replication/authority-server-link/L3',
                },
              ],
            },
            {
              label: 'API',
              items: [
                { label: 'Contracts (L1)', slug: 'docs/api/L1' },
                { label: 'Structure & flows (L2)', slug: 'docs/api/L2' },
                { label: 'Implementation reference (L3)', slug: 'docs/api/L3' },
                { label: 'Endpoint inventory', slug: 'docs/api/endpoints' },
              ],
            },
            {
              label: 'Authority server',
              items: [
                { label: 'Contracts (L1)', slug: 'docs/authority-server/L1' },
                {
                  label: 'Structure & flows (L2)',
                  slug: 'docs/authority-server/L2',
                },
                {
                  label: 'Adapter patterns (L3)',
                  slug: 'docs/authority-server/L3',
                },
              ],
            },
            {
              label: 'Client',
              items: [
                { label: 'State contract (L1)', slug: 'docs/client/L1' },
                { label: 'Mail runtime (L2)', slug: 'docs/client/L2' },
                { label: 'Adapter patterns (L3)', slug: 'docs/client/L3' },
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
