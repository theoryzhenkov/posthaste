---
scope: L1
summary: "Markdown subset, MIME structure rules, draft lifecycle, reply/forward quoting"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: docs/L0-compose
  - path: docs/L1-jmap
  - path: docs/L1-sync
dependents:
  - path: docs/L1-ui
---

# Compose domain -- L1

## Supported Markdown subset

CommonMark core plus these GFM extensions:

- Tables (pipe syntax)
- Strikethrough (`~~text~~`)
- Task lists (`- [ ] item`)
- Autolinks (bare URLs become clickable)
- Hard line breaks (trailing `\` or two spaces)

Intentionally excluded: raw HTML injection, custom CSS, embedded images via Markdown syntax. Users attach images as files instead.

## HTML output rules

The rendered HTML is self-contained. CSS is inlined; no external stylesheets are referenced. The output uses a minimal, email-safe CSS reset that works across Gmail, Outlook, Apple Mail, and Thunderbird. Code blocks use a monospace font stack. Tables get basic border styling. No JavaScript, no external resources of any kind.

## MIME structure

New message (no attachments):

```
multipart/alternative
├── text/plain (Markdown source)
└── text/html (rendered HTML)
```

New message (with attachments):

```
multipart/mixed
├── multipart/alternative
│   ├── text/plain (Markdown source)
│   └── text/html (rendered HTML)
├── attachment1 (application/pdf, etc.)
└── attachment2
```

Reply (no attachments):

```
multipart/alternative
├── text/plain (new text + quoted original as > prefixed lines)
└── text/html (new HTML + <blockquote> of original)
```

## Compose session states

```
New -> Editing -> Saving -> Saved -> Sending -> Sent
                    |                    |
                    v                    v
                SaveFailed          SendFailed
```

- `New`: fresh compose window, no server draft yet
- `Editing`: user is typing; autosave triggers after 5 seconds of inactivity
- `Saving`: `Email/set` in progress to create/update the draft on server
- `Saved`: draft exists on server with `$draft` keyword
- `Sending`: `EmailSubmission/set` in progress
- `Sent`: submission accepted, draft moved to Sent mailbox
- `SaveFailed` / `SendFailed`: error state, user can retry

## ComposeSession interface

Rust object managed by the backend. The frontend interacts with it via REST API endpoints; the session manages all internal state and JMAP interaction.

```
ComposeSession {
    // Creation
    fn new_message(accountId: String, identityId: String) -> ComposeSession
    fn reply(accountId: String, emailId: String, replyAll: Bool) -> ComposeSession
    fn forward(accountId: String, emailId: String) -> ComposeSession

    // Editing (all return Result)
    fn set_to(recipients: [FfiRecipient]) -> Result<()>
    fn set_cc(recipients: [FfiRecipient]) -> Result<()>
    fn set_bcc(recipients: [FfiRecipient]) -> Result<()>
    fn set_subject(subject: String) -> Result<()>
    fn set_body(markdown: String) -> Result<()>
    fn add_attachment(data: Vec<u8>, filename: String, mimeType: String) -> Result<FfiAttachmentId>
    fn remove_attachment(id: FfiAttachmentId) -> Result<()>

    // Preview
    fn render_preview() -> Result<String>  // returns HTML

    // Persistence
    fn save_draft() -> Result<()>          // creates/updates server draft
    fn send() -> Result<()>                // submits via EmailSubmission/set
    fn discard() -> Result<()>             // deletes server draft if exists

    // State
    fn state() -> ComposeState
}
```

## Reply quoting

On reply, the original message body is extracted as plain text (from the `text/plain` part if available, otherwise stripped from HTML via `mail-parser`). Each line is prefixed with `> `. An attribution line is prepended: `On {date}, {sender name} <{sender email}> wrote:`. The cursor is placed above the quoted text with a blank line separator.

On reply-all, the `To` field is set to the original sender, and `Cc` includes all original recipients minus the user's own address. The `In-Reply-To` and `References` headers are set correctly for threading per RFC 2822.

## Forward quoting

The original message is included below a separator line `--- Forwarded message ---` with headers (From, Date, Subject, To) listed before the body. Attachments from the original message are re-attached to the new draft.

## Signature insertion

The user configures a signature per Identity, stored locally. The signature is appended below a `-- ` (dash dash space) separator line, which is the standard email signature delimiter defined in RFC 3676. The signature content is Markdown text processed through the same rendering pipeline as the message body. For v1, one signature per account. Signature management UI is deferred.

## Attachment handling

Files are uploaded through the JMAP Session `uploadUrl` endpoint before the email is assembled. Each uploaded blob gets a `blobId` referenced in the MIME structure. RFC 9404 `Blob/upload` may be used only when the server advertises the blob-management capability; it is not the baseline RFC 8620 upload path. The compose session tracks pending uploads and blocks `send()` until all uploads complete. Maximum attachment size is determined by the server's `maxSizeUpload` capability, which the client reads from the JMAP Session object.

## Error model

```
ComposeError
  ├── DraftSaveError(JmapError)     -- Email/set failed
  ├── SendError(JmapError)          -- EmailSubmission/set failed
  ├── AttachmentUploadError(cause)  -- JMAP upload endpoint failed
  ├── AttachmentTooLarge(maxBytes)  -- exceeds server limit
  ├── NoRecipients                  -- tried to send with empty To
  └── IdentityNotFound              -- invalid identity ID
```

## Invariants

- Markdown source is always preserved as the text/plain part
- HTML output contains no external resource references
- Drafts use Email/set with `$draft` keyword, never raw SMTP
- Send uses EmailSubmission/set. Server-side draft cleanup or Sent placement is requested through `onSuccessUpdateEmail` and the implicit Email/set response is handled as part of the same JMAP operation.
- The compose session is a Rust object; the frontend interacts via REST API
- `render_preview()` is called on text change (debounced) and returns HTML for WKWebView
- Attachments are uploaded before send, not inline with the email body

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| markdown-preserved | MUST | The Markdown source is always the text/plain part of sent email |
| html-self-contained | MUST | Rendered HTML contains no external resource references |
| draft-jmap | MUST | Drafts are stored on server via Email/set with $draft keyword |
| send-submission | MUST | Sending uses EmailSubmission/set, not raw SMTP |
| reply-threading | MUST | Replies set In-Reply-To and References headers correctly |
| reply-quote | MUST | Reply body includes attribution line and > prefixed original text |
| forward-attachments | SHOULD | Forward re-attaches original message attachments |
| sig-delimiter | MUST | Signature is preceded by standard `-- ` delimiter line |
| upload-before-send | MUST | All attachments are uploaded through the JMAP upload endpoint before EmailSubmission |
| no-send-empty-to | MUST | send() returns NoRecipients error if To is empty |
