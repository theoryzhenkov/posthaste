import matter from 'gray-matter'
import { marked } from 'marked'
import type {
  HomeContent,
  SiteMessage,
  SiteNote,
  TitledHtmlPiece,
} from './types'
import ametRaw from './home/messages/amet.md?raw'
import communityExtensionsRaw from './home/messages/community-extensions.md?raw'
import dolorRaw from './home/messages/dolor.md?raw'
import welcomeRaw from './home/messages/welcome.md?raw'
import noteDolorRaw from './home/notes/dolor.md?raw'
import noteIpsumRaw from './home/notes/ipsum.md?raw'
import noteLoremRaw from './home/notes/lorem.md?raw'
import footerRaw from './home/footer.md?raw'
import notesHeadingRaw from './home/notes-heading.md?raw'
import openSourceRaw from './home/open-source.md?raw'
import themeRaw from './home/theme.md?raw'

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

async function parseNote(raw: string, file: string): Promise<SiteNote> {
  const document = await parseDocument(raw)

  return {
    label: requireString(document.data, 'label', file),
    title: requireString(document.data, 'title', file),
    html: document.html,
  }
}

export async function getHomeContent(): Promise<HomeContent> {
  const [
    welcome,
    communityExtensions,
    dolor,
    amet,
    openSource,
    notesHeading,
    noteLorem,
    noteIpsum,
    noteDolor,
    themeDocument,
    footerDocument,
  ] = await Promise.all([
    parseMessage(welcomeRaw, 'home/messages/welcome.md'),
    parseMessage(
      communityExtensionsRaw,
      'home/messages/community-extensions.md',
    ),
    parseMessage(dolorRaw, 'home/messages/dolor.md'),
    parseMessage(ametRaw, 'home/messages/amet.md'),
    parseTitledPiece(openSourceRaw, 'home/open-source.md'),
    parseTitledPiece(notesHeadingRaw, 'home/notes-heading.md'),
    parseNote(noteLoremRaw, 'home/notes/lorem.md'),
    parseNote(noteIpsumRaw, 'home/notes/ipsum.md'),
    parseNote(noteDolorRaw, 'home/notes/dolor.md'),
    parseDocument(themeRaw),
    parseDocument(footerRaw),
  ])

  return {
    messages: [welcome, communityExtensions, dolor, amet],
    openSource,
    notesHeading,
    notes: [noteLorem, noteIpsum, noteDolor],
    theme: {
      title: requireString(themeDocument.data, 'title', 'home/theme.md'),
      html: themeDocument.html,
      eyebrow: requireString(themeDocument.data, 'eyebrow', 'home/theme.md'),
    },
    footer: {
      brand: requireString(footerDocument.data, 'brand', 'home/footer.md'),
      html: footerDocument.html,
    },
  }
}
