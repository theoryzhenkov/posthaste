export type ComposeIntent =
  | { kind: 'new'; sourceId: string }
  | { kind: 'reply'; sourceId: string; messageId: string }
  | { kind: 'replyAll'; sourceId: string; messageId: string }
  | { kind: 'forward'; sourceId: string; messageId: string }
  // Resume editing an existing draft. `messageId` is the draft's id, reused as
  // the autosave draft key so edits update that draft instead of creating one.
  | { kind: 'draft'; sourceId: string; messageId: string }
