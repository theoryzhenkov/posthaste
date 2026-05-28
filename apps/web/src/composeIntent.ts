export type ComposeIntent =
  | { kind: 'new'; sourceId: string }
  | { kind: 'reply'; sourceId: string; messageId: string }
  | { kind: 'forward'; sourceId: string; messageId: string }
