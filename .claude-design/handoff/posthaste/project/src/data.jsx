// Sample data for Posthaste — accounts, mailboxes, messages, tags
// Single source of truth for the prototype.

const PH_ACCOUNTS = [
  {
    id: 'gmail',
    name: 'theor@gmail.com',
    label: 'Gmail',
    color: 'oklch(0.72 0.15 25)',
    stamp: 'G',
    mailboxes: [
      { id: 'gmail/inbox', name: 'Inbox', icon: 'Inbox', unread: 8, total: 142 },
      { id: 'gmail/starred', name: 'Starred', icon: 'Star', unread: 0, total: 23 },
      { id: 'gmail/sent', name: 'Sent Mail', icon: 'Sent', unread: 0, total: 341 },
      { id: 'gmail/drafts', name: 'Drafts', icon: 'Drafts', unread: 0, total: 3 },
      { id: 'gmail/spam', name: 'Spam', icon: 'Junk', unread: 0, total: 12 },
      { id: 'gmail/trash', name: 'Trash', icon: 'Trash', unread: 0, total: 88 },
    ],
  },
  {
    id: 'work',
    name: 'theor@posthaste.app',
    label: 'Work',
    color: 'oklch(0.68 0.12 240)',
    stamp: 'W',
    mailboxes: [
      { id: 'work/inbox', name: 'Inbox', icon: 'Inbox', unread: 3, total: 58 },
      { id: 'work/sent', name: 'Sent', icon: 'Sent', unread: 0, total: 112 },
      { id: 'work/notes', name: 'Notes', icon: 'Folder', unread: 0, total: 7 },
      { id: 'work/system', name: 'System', icon: 'Folder', unread: 0, total: 24 },
    ],
  },
  {
    id: 'uni',
    name: 'theor@university.edu',
    label: 'University',
    color: 'oklch(0.68 0.10 145)',
    stamp: 'U',
    mailboxes: [
      { id: 'uni/inbox', name: 'Inbox', icon: 'Inbox', unread: 2, total: 87 },
      { id: 'uni/sent', name: 'Sent', icon: 'Sent', unread: 0, total: 64 },
      { id: 'uni/notes', name: 'Notes', icon: 'Folder', unread: 0, total: 15 },
    ],
  },
];

const PH_SMART_MAILBOXES = [
  { id: 'smart/relevant', name: 'Relevant', icon: 'Sparkle', query: 'is:unread from:frequent', unread: 5, accent: 'coral' },
  { id: 'smart/readlater', name: 'Read Later', icon: 'Snooze', query: 'tag:readlater', unread: 11, accent: 'amber' },
  { id: 'smart/bills', name: 'Bills', icon: 'Tag', query: 'tag:bills', unread: 2, accent: 'violet' },
  { id: 'smart/newsletters', name: 'Newsletters', icon: 'Layers', query: 'list-id:*', unread: 24, accent: 'sage' },
  { id: 'smart/today', name: 'Today', icon: 'Bolt', query: 'date:today', unread: 7, accent: 'blue' },
];

const PH_TAGS = [
  { id: 'work', name: 'work', color: 'oklch(0.68 0.12 240)' },
  { id: 'personal', name: 'personal', color: 'oklch(0.68 0.10 145)' },
  { id: 'billing', name: 'billing', color: 'oklch(0.65 0.13 295)' },
  { id: 'followup', name: 'follow-up', color: 'oklch(0.68 0.17 45)' },
  { id: 'readlater', name: 'read-later', color: 'oklch(0.78 0.13 78)' },
];

// Messages — variety of subjects to demo threading, attachments, tags
const PH_MESSAGES = [
  {
    id: 'm1', account: 'gmail', mailbox: 'gmail/inbox',
    from: { name: 'Leo Cancelmo', email: 'leocancelmophd@gmail.com' },
    to: 'theor@gmail.com',
    subject: 'March 2026 billing',
    preview: 'Hi Theo — see attached for March. Let me know if any questions about the line items this month.',
    body: `Hi Theo —

Attached is the billing statement for March 2026. All sessions are itemized by date, with the usual breakdown for insurance claim submission.

Let me know if any questions about the line items this month.

Best,
Leo Cancelmo, Ph.D.
Clinical Psychologist
917.740.9826
243 West End Avenue, Suite 101
NY, NY 10023
https://account.venmo.com/u/LeoCancelmoPhD`,
    date: 'Yesterday, 18:04',
    dateShort: 'Yesterday',
    size: '53.6 KiB',
    unread: false, flagged: true, hasAttachment: true,
    attachments: [{ name: 'TR_Billing March 2026.pdf', size: '53.6 KiB', type: 'pdf' }],
    tags: ['billing'],
    threadCount: 1,
  },
  {
    id: 'm2', account: 'work', mailbox: 'work/inbox',
    from: { name: 'Maya Okafor', email: 'maya@posthaste.app' },
    subject: 'JMAP spec 1.2 — review thread',
    preview: 'Pushed the v1.2 diff. The new blob-upload semantics look clean but we need to double-check how we handle…',
    body: `Team,

Pushed the v1.2 diff for review. Key changes:

  • New blob-upload semantics (cleaner but we need to double-check migration)
  • EmailSubmission response shape tweak
  • New /sessions endpoint ETag behavior
  • Push keepalive interval is now negotiated

I'd love comments by end of week. The implementation side is pretty localized — mostly src/jmap/session.ts and src/jmap/upload.ts.

m.`,
    date: 'Today, 11:42', dateShort: '11:42',
    unread: true, flagged: false, hasAttachment: false,
    tags: ['work', 'followup'],
    threadCount: 4,
  },
  {
    id: 'm3', account: 'gmail', mailbox: 'gmail/inbox',
    from: { name: 'Figma', email: 'no-reply@figma.com' },
    subject: 'Weekly digest: 12 comments on Posthaste v0.4',
    preview: '12 new comments across 3 files. Most active: "Compose modal", "Reader spacing", "Mobile thread view".',
    body: 'See the summary of recent activity across files you watch.',
    date: 'Today, 09:01', dateShort: '09:01',
    unread: true, flagged: false, hasAttachment: false,
    tags: ['readlater'],
    threadCount: 1,
  },
  {
    id: 'm4', account: 'gmail', mailbox: 'gmail/inbox',
    from: { name: 'Mom', email: 'mom@family.net' },
    subject: 'Re: weekend plans',
    preview: 'Sounds lovely. Dad is making his lasagna so bring your appetite. Call when you land!',
    body: `Sounds lovely, honey.

Dad is making his lasagna so bring your appetite. Call when you land — I'll pick you up from the station if you give me 20 min notice.

Love,
Mom`,
    date: 'Today, 08:15', dateShort: '08:15',
    unread: true, flagged: false, hasAttachment: false,
    tags: ['personal'],
    threadCount: 7,
  },
  {
    id: 'm5', account: 'uni', mailbox: 'uni/inbox',
    from: { name: 'Prof. D. Halperin', email: 'halperin@university.edu' },
    subject: 'Thesis committee — availability May 12–16',
    preview: 'Please send your availability for defense scheduling. I\'ve pencilled in May 14 but want to confirm with Roh…',
    body: `Dear Theo,

Please send your availability for your defense during the week of May 12–16. I've pencilled in the afternoon of May 14 but want to confirm with Prof. Rohnberg before I lock the room.

Also: please ensure your final draft is uploaded to the departmental system by May 5.

Best,
Prof. Halperin`,
    date: 'Today, 07:38', dateShort: '07:38',
    unread: true, flagged: true, hasAttachment: false,
    tags: ['followup'],
    threadCount: 3,
  },
  {
    id: 'm6', account: 'work', mailbox: 'work/inbox',
    from: { name: 'GitHub', email: 'notifications@github.com' },
    subject: '[posthaste/posthaste] PR #284: Reader pane virtualization',
    preview: 'sam-k opened a pull request. Adds windowed rendering to the reader pane for messages > 10k lines.',
    body: '...',
    date: 'Yesterday, 23:10', dateShort: 'Yesterday',
    unread: false, flagged: false, hasAttachment: false,
    tags: ['work'],
    threadCount: 12,
  },
  {
    id: 'm7', account: 'gmail', mailbox: 'gmail/inbox',
    from: { name: 'Substack — The Diff', email: 'thediff@substack.com' },
    subject: 'Vertical SaaS vs horizontal: the 2026 scoreboard',
    preview: 'Two years ago the answer seemed obvious. Today the picture is more complicated, and the comp tables tell…',
    body: '...',
    date: 'Yesterday, 16:22', dateShort: 'Yesterday',
    unread: false, flagged: false, hasAttachment: false,
    tags: ['readlater'],
    threadCount: 1,
  },
  {
    id: 'm8', account: 'gmail', mailbox: 'gmail/inbox',
    from: { name: 'Con Ed', email: 'billing@coned.com' },
    subject: 'Your April statement is ready',
    preview: 'Your electricity bill for the period Mar 15 – Apr 14 is now available. Amount due: $87.42.',
    body: '...',
    date: 'Yesterday, 14:00', dateShort: 'Yesterday',
    unread: false, flagged: false, hasAttachment: true,
    attachments: [{ name: 'statement-apr-2026.pdf', size: '124 KiB', type: 'pdf' }],
    tags: ['billing'],
    threadCount: 1,
  },
  {
    id: 'm9', account: 'work', mailbox: 'work/inbox',
    from: { name: 'Sam Kalani', email: 'sam@posthaste.app' },
    subject: 'Tauri 2 upgrade notes',
    preview: 'I took a pass at the upgrade branch. Mostly smooth — a couple IPC signatures changed and the updater…',
    body: '...',
    date: 'Mon 21 Apr', dateShort: 'Mon',
    unread: false, flagged: false, hasAttachment: false,
    tags: ['work'],
    threadCount: 2,
  },
  {
    id: 'm10', account: 'gmail', mailbox: 'gmail/inbox',
    from: { name: 'Hacker News Digest', email: 'digest@hndigest.com' },
    subject: '5 stories you might have missed',
    preview: '“Show HN: a JMAP client for the terminal” — 842 points, 213 comments. Plus four more you might like.',
    body: '...',
    date: 'Mon 21 Apr', dateShort: 'Mon',
    unread: false, flagged: false, hasAttachment: false,
    tags: ['readlater'],
    threadCount: 1,
  },
  {
    id: 'm11', account: 'uni', mailbox: 'uni/inbox',
    from: { name: 'Library Services', email: 'lib@university.edu' },
    subject: 'Books due May 3: 2 titles',
    preview: 'Your loan on "Patterns of Software" (Gabriel) and "The Psychology of Everyday Things" is due May 3.',
    body: '...',
    date: 'Sun 20 Apr', dateShort: 'Sun',
    unread: false, flagged: false, hasAttachment: false,
    tags: [],
    threadCount: 1,
  },
  {
    id: 'm12', account: 'gmail', mailbox: 'gmail/inbox',
    from: { name: 'Clara Beaumont', email: 'clara@studio.co' },
    subject: 'illustration commission — first pass',
    preview: 'Here is the first pass for the postmark illustrations we discussed. Happy to revise any of the weights.',
    body: '...',
    date: 'Sat 19 Apr', dateShort: 'Sat',
    unread: false, flagged: true, hasAttachment: true,
    attachments: [
      { name: 'posthaste-marks-v1.png', size: '1.4 MB', type: 'image' },
      { name: 'posthaste-marks-v1.ai', size: '2.8 MB', type: 'ai' },
    ],
    tags: ['work'],
    threadCount: 1,
  },
];

// Thread for m2 (JMAP spec) — used when the reader opens with thread view
const PH_THREAD = [
  {
    id: 't1', from: 'Maya Okafor', preview: 'Pushed the v1.2 diff for review…',
    date: 'Today 11:42', excerpt: 'Team — pushed the v1.2 diff. Key changes: blob-upload, EmailSubmission, /sessions ETag, push keepalive…'
  },
  { id: 't2', from: 'Sam Kalani', preview: 're: love the blob change, but…', date: 'Today 12:08', excerpt: 'Love the blob change, but we should probably keep the legacy endpoint alive for one minor…' },
  { id: 't3', from: 'Maya Okafor', preview: 'Good point — draft 2 attached', date: 'Today 13:15', excerpt: 'Good point. I pushed a second draft that keeps the legacy blob-upload path for 1 minor release…' },
  { id: 't4', from: 'You', preview: 'LGTM. I\'ll handle the client side', date: 'Today 14:30', excerpt: 'LGTM. I\'ll handle the client-side migration and ship a feature flag so we can switch atomically…' },
];

Object.assign(window, {
  PH_ACCOUNTS, PH_SMART_MAILBOXES, PH_TAGS, PH_MESSAGES, PH_THREAD,
});
