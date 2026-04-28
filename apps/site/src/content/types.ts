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

export interface SiteNote extends TitledHtmlPiece {
  label: string
}

export interface HomeContent {
  messages: SiteMessage[]
  openSource: TitledHtmlPiece
  notesHeading: TitledHtmlPiece
  notes: SiteNote[]
  theme: TitledHtmlPiece & {
    eyebrow: string
  }
  footer: {
    brand: string
    html: string
  }
}
