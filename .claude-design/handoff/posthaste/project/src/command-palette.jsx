// Command palette — ⌘K. Glass modal pinned to top of viewport.

const PH_COMMANDS = [
  { id: 'compose',     label: 'Compose new message',       icon: 'Compose',  kbd: '⌘N',  group: 'Actions' },
  { id: 'reply',       label: 'Reply',                     icon: 'Reply',    kbd: '⌘R',  group: 'Actions' },
  { id: 'archive',     label: 'Archive selected',          icon: 'Archive',  kbd: 'E',   group: 'Actions' },
  { id: 'flag',        label: 'Flag message',              icon: 'Flag',     kbd: '⇧⌘L', group: 'Actions' },
  { id: 'snooze',      label: 'Snooze…',                   icon: 'Snooze',   kbd: 'H',   group: 'Actions' },
  { id: 'newSmart',    label: 'New smart mailbox…',        icon: 'Bolt',     group: 'Create' },
  { id: 'newRule',     label: 'New rule for mailbox…',     icon: 'Sliders',  group: 'Create' },
  { id: 'settings',    label: 'Open Settings',             icon: 'Settings', kbd: '⌘,',  group: 'Navigate' },
  { id: 'shortcuts',   label: 'Keyboard shortcuts',        icon: 'Keyboard', kbd: '?',   group: 'Navigate' },
  { id: 'account',     label: 'Add account…',              icon: 'At',       group: 'Navigate' },
];

function CommandPalette({ T, onClose, onCommand }) {
  const [q, setQ] = React.useState('');
  const [sel, setSel] = React.useState(0);
  const inputRef = React.useRef(null);

  // Build results: commands first, then messages, contacts, mailboxes
  const results = React.useMemo(() => {
    const Q = q.trim().toLowerCase();
    const matches = (s) => !Q || String(s).toLowerCase().includes(Q);
    const out = [];
    const cmds = PH_COMMANDS.filter((c) => matches(c.label));
    if (cmds.length) out.push({ group: 'Commands', items: cmds.map((c) => ({ ...c, kind: 'command' })) });

    const msgs = PH_MESSAGES.filter((m) => matches(m.subject) || matches(m.from) || matches(m.preview)).slice(0, 6);
    if (msgs.length) out.push({ group: 'Messages', items: msgs.map((m) => ({
      id: m.id, kind: 'message', label: m.subject, sub: `${m.from} · ${m.dateShort}`, icon: 'Thread',
    })) });

    const contacts = Array.from(new Set(PH_MESSAGES.map((m) => m.from))).filter(matches).slice(0, 5);
    if (contacts.length) out.push({ group: 'Contacts', items: contacts.map((c) => ({
      id: 'c:' + c, kind: 'contact', label: c, icon: 'User',
    })) });

    const mailboxes = [];
    for (const a of PH_ACCOUNTS) {
      for (const m of a.mailboxes) if (matches(m.name)) mailboxes.push({
        id: m.id, kind: 'mailbox', label: m.name, sub: a.label, icon: m.icon || 'Folder',
      });
    }
    if (mailboxes.length) out.push({ group: 'Mailboxes', items: mailboxes.slice(0, 6) });

    return out;
  }, [q]);

  const flat = results.flatMap((g) => g.items);

  React.useEffect(() => { setSel(0); }, [q]);
  React.useEffect(() => {
    const h = (e) => {
      if (e.key === 'ArrowDown') { e.preventDefault(); setSel((s) => Math.min(flat.length - 1, s + 1)); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
      else if (e.key === 'Enter') { e.preventDefault(); if (flat[sel]) { onCommand && onCommand(flat[sel]); onClose(); } }
    };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [flat, sel, onCommand, onClose]);

  return (
    <div
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
      style={{
        position: 'absolute', inset: 0, zIndex: 2500,
        display: 'flex', alignItems: 'flex-start', justifyContent: 'center',
        paddingTop: '9%',
        background: 'rgba(6,4,12,0.4)',
        backdropFilter: 'blur(22px) saturate(150%)',
        WebkitBackdropFilter: 'blur(22px) saturate(150%)',
        animation: 'ph-modal-in 0.16s ease-out',
      }}>
      <div style={{
        width: 640, maxWidth: '92vw',
        background: 'rgba(22,20,28,0.88)',
        border: '1px solid rgba(255,255,255,0.08)',
        borderRadius: 14,
        boxShadow: '0 28px 80px rgba(0,0,0,0.6)',
        overflow: 'hidden', color: T.fg, fontFamily: T.font.sans,
        animation: 'ph-sheet-in 0.2s cubic-bezier(0.2, 0.9, 0.3, 1.0)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', padding: '0 16px', borderBottom: `1px solid ${T.borderSoft}` }}>
          <Icons.Search size={18} style={{ color: T.fgMuted }} />
          <input
            ref={inputRef} autoFocus value={q} onChange={(e) => setQ(e.target.value)}
            placeholder="Search messages, contacts, commands…"
            style={{
              flex: 1, height: 48, padding: '0 12px', border: 'none', outline: 'none',
              background: 'transparent', color: T.fg, fontFamily: 'inherit', fontSize: 16,
            }} />
          <Kbd T={T}>Esc</Kbd>
        </div>
        <div className="ph-scroll" style={{ maxHeight: 440, overflow: 'auto', padding: '6px 0' }}>
          {results.length === 0 && (
            <div style={{ padding: '36px 20px', textAlign: 'center', color: T.fgMuted, fontSize: T.type.body }}>
              No results. Try a different query.
            </div>
          )}
          {results.map((grp) => (
            <div key={grp.group} style={{ padding: '6px 0' }}>
              <div style={{ padding: '4px 16px', fontSize: T.type.meta, color: T.fgFaint,
                fontFamily: T.font.mono, textTransform: 'uppercase', letterSpacing: 0.7, fontWeight: 600 }}>
                {grp.group}
              </div>
              {grp.items.map((it) => {
                const flatIdx = flat.indexOf(it);
                const active = flatIdx === sel;
                const Ico = Icons[it.icon] || Icons.Dot;
                return (
                  <button key={it.id || it.label} onClick={() => { onCommand && onCommand(it); onClose(); }}
                    onMouseEnter={() => setSel(flatIdx)}
                    style={{
                      width: '100%', display: 'flex', alignItems: 'center', gap: 10,
                      padding: '8px 16px', border: 'none',
                      background: active ? 'rgba(255,255,255,0.08)' : 'transparent',
                      color: T.fg, cursor: 'pointer', textAlign: 'left',
                      fontFamily: 'inherit', fontSize: T.type.body,
                    }}>
                    <Ico size={15} style={{ color: T.fgMuted, flexShrink: 0 }} />
                    <span style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{it.label}</span>
                    {it.sub && <span style={{ color: T.fgMuted, fontSize: T.type.ui, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 240 }}>{it.sub}</span>}
                    {it.kbd && <Kbd T={T}>{it.kbd}</Kbd>}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 14, padding: '8px 16px',
          borderTop: `1px solid ${T.borderSoft}`, fontSize: T.type.meta,
          color: T.fgFaint, fontFamily: T.font.mono,
        }}>
          <span style={{ display: 'flex', gap: 4, alignItems: 'center' }}><Kbd T={T}>↑</Kbd><Kbd T={T}>↓</Kbd> navigate</span>
          <span style={{ display: 'flex', gap: 4, alignItems: 'center' }}><Kbd T={T}>↵</Kbd> select</span>
          <span style={{ display: 'flex', gap: 4, alignItems: 'center' }}><Kbd T={T}>Esc</Kbd> close</span>
          <div style={{ flex: 1 }} />
          <span>posthaste</span>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { CommandPalette });
