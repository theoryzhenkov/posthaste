import { Renderer, marked, type MarkedOptions, type Tokens } from 'marked'

const previewRenderer = new Renderer()

previewRenderer.html = ({ text }: Tokens.HTML | Tokens.Tag): string =>
  escapeHtml(text)
previewRenderer.image = ({ text }: Tokens.Image): string => escapeHtml(text)

const PREVIEW_OPTIONS: MarkedOptions & { async: false } = {
  async: false,
  breaks: false,
  gfm: true,
  renderer: previewRenderer,
}

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

export function renderMarkdownPreview(markdown: string): string {
  return marked(markdown, PREVIEW_OPTIONS)
}
