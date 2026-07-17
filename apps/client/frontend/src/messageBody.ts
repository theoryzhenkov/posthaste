/** Message body rendering preference for the reader pane. */
export type MessageBodyRender =
  | { kind: 'html'; html: string }
  | { kind: 'text'; paragraphs: string[] }
  | { kind: 'empty'; fallback: string }

export interface MessageBodySource {
  bodyHtml: string | null
  bodyText: string | null
  preview: string | null
}

/** Prefer rich HTML alternatives, falling back to plaintext and then preview. */
export function resolveMessageBodyRender(
  message: MessageBodySource,
): MessageBodyRender {
  if (message.bodyHtml?.trim()) {
    return { kind: 'html', html: message.bodyHtml }
  }

  if (message.bodyText?.trim()) {
    return {
      kind: 'text',
      paragraphs: message.bodyText.split(/\n{2,}/),
    }
  }

  return {
    kind: 'empty',
    fallback: message.preview ?? 'No content available.',
  }
}
