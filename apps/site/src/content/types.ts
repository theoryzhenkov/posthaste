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
/** Which shipped artifact a download is for. */
export type ReleaseProduct = 'desktop' | 'cli' | 'daemon'
/** Release channel: promoted stable builds vs rolling nightly pre-releases. */
export type ReleaseChannel = 'stable' | 'nightly'

export interface ReleaseAsset {
  product: ReleaseProduct
  os: ReleaseOs
  arch: string
  kind: string
  name: string
  url: string
  size: number
}

export interface ReleaseEntry {
  /** User-facing app version, e.g. "0.2.0" or "0.2.0-nightly.44". */
  version: string
  /** Underlying git tag, e.g. "v0.2.0-nightly.44". */
  tag: string
  /** Release date, ISO `YYYY-MM-DD`. */
  date: string
  channel: ReleaseChannel
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
