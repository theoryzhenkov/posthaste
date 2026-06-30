export interface HtmlPiece {
  html: string
}

export interface TitledHtmlPiece extends HtmlPiece {
  title: string
}

export interface SiteMessage extends TitledHtmlPiece {
  id: string
  from: string
  subject: string
  tag: string
  time: string
  color: string
  unread?: boolean
}

export type ReleaseOs = 'macOS' | 'Windows' | 'Linux'

export interface ReleaseAsset {
  os: ReleaseOs
  arch: string
  kind: string
  name: string
  url: string
  size: number
}

export interface ReleaseEntry {
  /** User-facing app version, e.g. "0.1.38". */
  version: string
  /** Underlying git tag, e.g. "v0.1.0-dogfood.38". */
  tag: string
  /** Release date, ISO `YYYY-MM-DD`. */
  date: string
  prerelease: boolean
  assets: ReleaseAsset[]
  /** Hand-authored dev notes, rendered from Markdown (empty when unwritten). */
  notesHtml: string
  sha256sums?: string
  gpgKey?: string
}

export interface HomeContent {
  messages: SiteMessage[]
  openSource: TitledHtmlPiece
  footer: {
    brand: string
    html: string
  }
}
