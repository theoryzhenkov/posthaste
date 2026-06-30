import matter from 'gray-matter'
import { marked } from 'marked'
import type { HomeContent, SiteMessage, TitledHtmlPiece } from './types'
import welcomeRaw from './home/messages/welcome.md?raw'
import performanceRaw from './home/messages/performance.md?raw'
import wizardRaw from './home/messages/magick.md?raw'
import visualsRaw from './home/messages/visuals.md?raw'
import communityRaw from './home/messages/community.md?raw'
import supportRaw from './home/messages/support.md?raw'
import footerRaw from './home/footer.md?raw'
import openSourceRaw from './home/open-source.md?raw'

interface MarkdownDocument {
  data: Record<string, unknown>
  html: string
}

function requireString(
  data: Record<string, unknown>,
  key: string,
  file: string,
): string {
  const value = data[key]

  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`${file} must define a non-empty "${key}" string`)
  }

  return value
}

function optionalBoolean(data: Record<string, unknown>, key: string): boolean {
  return data[key] === true
}

async function parseDocument(raw: string): Promise<MarkdownDocument> {
  const parsed = matter(raw)
  const html = await marked.parse(parsed.content.trim())

  return {
    data: parsed.data as Record<string, unknown>,
    html,
  }
}

async function parseTitledPiece(
  raw: string,
  file: string,
): Promise<TitledHtmlPiece> {
  const document = await parseDocument(raw)

  return {
    title: requireString(document.data, 'title', file),
    html: document.html,
  }
}

async function parseMessage(raw: string, file: string): Promise<SiteMessage> {
  const document = await parseDocument(raw)

  return {
    id: requireString(document.data, 'id', file),
    from: requireString(document.data, 'from', file),
    subject: requireString(document.data, 'subject', file),
    title: requireString(document.data, 'subject', file),
    tag: requireString(document.data, 'tag', file),
    time: requireString(document.data, 'time', file),
    color: requireString(document.data, 'color', file),
    unread: optionalBoolean(document.data, 'unread'),
    html: document.html,
  }
}

export async function getHomeContent(): Promise<HomeContent> {
  const [
    welcome,
    performance,
    wizard,
    visuals,
    community,
    support,
    openSource,
    footerDocument,
  ] = await Promise.all([
    parseMessage(welcomeRaw, 'home/messages/welcome.md'),
    parseMessage(performanceRaw, 'home/messages/performance.md'),
    parseMessage(wizardRaw, 'home/messages/magick.md'),
    parseMessage(visualsRaw, 'home/messages/visuals.md'),
    parseMessage(communityRaw, 'home/messages/community.md'),
    parseMessage(supportRaw, 'home/messages/support.md'),
    parseTitledPiece(openSourceRaw, 'home/open-source.md'),
    parseDocument(footerRaw),
  ])

  return {
    messages: [welcome, performance, wizard, visuals, community, support],
    openSource,
    footer: {
      brand: requireString(footerDocument.data, 'brand', 'home/footer.md'),
      html: footerDocument.html,
    },
  }
}
