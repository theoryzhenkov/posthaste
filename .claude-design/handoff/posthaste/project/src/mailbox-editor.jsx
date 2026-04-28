// Mailbox / Rule editor modal — Arc-style.
//
// Smart mailboxes have both Match (inclusion query) + Then (actions) tabs.
// Regular mailboxes only have Then (rules attached to the folder).
//
// Match tab supports Visual row builder + Raw JMAP filter text, toggled.

const PH_FIELDS = [
  { id: 'from',      label: 'From',          type: 'text' },
  { id: 'to',        label: 'To / Cc',       type: 'text' },
  { id: 'subject',   label: 'Subject',       type: 'text' },
  { id: 'body',      label: 'Body',          type: 'text' },
  { id: 'thread',    label: 'Thread subject',type: 'text' },
  { id: 'list',      label: 'Mailing list',  type: 'text' },
  { id: 'hasAttach', label: 'Attachment',    type: 'bool' },
  { id: 'size',      label: 'Size',          type: 'number' },
  { id: 'date',      label: 'Date',          type: 'date' },
  { id: 'tag',       label: 'Tag',           type: 'enum' },
  { id: 'unread',    label: 'Unread',        type: 'bool' },
];

const PH_OPS = {
  text: [
    { id: 'contains',    label: 'contains' },
    { id: 'eq',          label: 'is' },
    { id: 'neq',         label: 'is not' },
    { id: 'startsWith',  label: 'starts with' },
    { id: 'endsWith',    label: 'ends with' },
    { id: 'matches',     label: 'matches (regex)' },
  ],
  bool: [
    { id: 'is', label: 'is' },
  ],
  number: [
    { id: 'gt', label: '>' },
    { id: 'lt', label: '<' },
    { id: 'eq', label: '=' },
  ],
  date: [
    { id: 'after',  label: 'after' },
    { id: 'before', label: 'before' },
    { id: 'last',   label: 'in last' },
  ],
  enum: [
    { id: 'eq',  label: 'is' },
    { id: 'neq', label: 'is not' },
  ],
};

const PH_ACTIONS = [
  { id: 'tag',        label: 'Add tag',         icon: 'Tag',    arg: 'enum' },
  { id: 'star',       label: 'Star',            icon: 'Star',   arg: null },
  { id: 'move',       label: 'Move to',         icon: 'Folder', arg: 'enum' },
  { id: 'read',       label: 'Mark as read',    icon: 'Check',  arg: null },
  { id: 'pin',        label: 'Pin to top',      icon: 'Pin',    arg: null },
  { id: 'skipInbox',  label: 'Skip inbox',      icon: 'Archive',arg: null },
  { id: 'mute',       label: 'Mute thread',     icon: 'VolumeX',arg: null },
  { id: 'forward',    label: 'Forward to',      icon: 'Forward',arg: 'email' },
  { id: 'notify',     label: 'Send notification',icon: 'Bell',   arg: 'text' },
  { id: 'aiSummary',  label: 'Generate AI brief',icon: 'Sparkle',arg: null },
  { id: 'webhook',    label: 'Run webhook',     icon: 'Webhook',arg: 'url' },
];

function makeDefaultRule() {
  return {
    name: 'Finance digest',
    icon: 'Bolt',
    accent: 'amber',
    combinator: 'all', // all | any
    conditions: [
      { id: crypto.randomUUID(), field: 'from', op: 'contains', value: '@stripe.com' },
      { id: crypto.randomUUID(), field: 'subject', op: 'contains', value: 'invoice' },
    ],
    actions: [
      { id: crypto.randomUUID(), type: 'tag', value: 'finance' },
      { id: crypto.randomUUID(), type: 'skipInbox' },
      { id: crypto.randomUUID(), type: 'aiSummary' },
    ],
  };
}

function MailboxEditor({ T, mode = 'smart', onClose, initial }) {
  // mode: 'smart' (match + then) | 'mailbox' (then only)
  const [rule, setRule] = React.useState(() => initial || makeDefaultRule());
  const [tab, setTab] = React.useState(mode === 'smart' ? 'match' : 'then');
  const [matchMode, setMatchMode] = React.useState('visual'); // visual | raw

  const update = (patch) => setRule((r) => ({ ...r, ...patch }));
  const isSmart = mode === 'smart';

  return (
    <Modal T={T} onClose={onClose} width={860} height={640}>
      {/* Header with name + icon editor inline */}
      <div style={{
        padding: '16px 22px 12px',
        borderBottom: `1px solid ${T.borderSoft}`,
        display: 'flex', alignItems: 'center', gap: 12,
      }}>
        <IconPicker T={T} value={rule.icon} accent={rule.accent}
          onChange={(icon, accent) => update({ icon, accent })} />
        <input
          value={rule.name}
          onChange={(e) => update({ name: e.target.value })}
          placeholder={isSmart ? 'Smart mailbox name' : 'Mailbox rules'}
          style={{
            flex: 1, fontSize: T.type.head, fontWeight: 700, letterSpacing: -0.3,
            color: T.fg, border: 'none', outline: 'none', background: 'transparent',
            fontFamily: 'inherit',
          }}
        />
        <div style={{ fontSize: T.type.meta, color: T.fgMuted, fontFamily: T.font.mono }}>
          {isSmart ? 'SMART MAILBOX' : 'MAILBOX RULES'}
        </div>
        <button onClick={onClose} title="Close (Esc)" style={{
          width: 30, height: 30, borderRadius: 8,
          border: 'none', background: 'transparent', color: T.fgMuted,
          cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}><Icons.X size={16} /></button>
      </div>

      {/* Tabs (only when smart) */}
      {isSmart && (
        <div style={{
          padding: '0 22px', borderBottom: `1px solid ${T.borderSoft}`,
          display: 'flex', gap: 2, flexShrink: 0,
        }}>
          {[
            { id: 'match', label: 'Match', hint: 'Inclusion rules — what belongs in this mailbox' },
            { id: 'then',  label: 'Then',  hint: 'Actions — what happens to matched messages' },
          ].map((t) => (
            <button key={t.id} onClick={() => setTab(t.id)} style={{
              padding: '10px 14px', border: 'none', background: 'transparent',
              color: tab === t.id ? T.fg : T.fgMuted,
              fontFamily: 'inherit', fontSize: T.type.body, fontWeight: tab === t.id ? 600 : 500,
              cursor: 'pointer', position: 'relative',
              borderBottom: `2px solid ${tab === t.id ? T.accent.coral : 'transparent'}`,
              marginBottom: -1,
            }}>{t.label}</button>
          ))}
          <div style={{ flex: 1 }} />
          {tab === 'match' && (
            <div style={{ alignSelf: 'center', display: 'flex', gap: 3, padding: 3,
              background: T.bg, border: `1px solid ${T.borderSoft}`, borderRadius: 7 }}>
              {['visual', 'raw'].map((m) => (
                <button key={m} onClick={() => setMatchMode(m)} style={{
                  padding: '3px 10px', border: 'none', borderRadius: 5,
                  background: matchMode === m ? T.bgElev : 'transparent',
                  color: matchMode === m ? T.fg : T.fgMuted,
                  fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 600,
                  cursor: 'pointer', textTransform: 'uppercase', letterSpacing: 0.5,
                }}>{m}</button>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Content */}
      <div className="ph-scroll" style={{ flex: 1, overflow: 'auto', padding: '20px 22px' }}>
        {tab === 'match' ? (
          matchMode === 'visual'
            ? <MatchVisual T={T} rule={rule} update={update} />
            : <MatchRaw T={T} rule={rule} />
        ) : (
          <ThenEditor T={T} rule={rule} update={update} />
        )}
      </div>

      {/* Footer */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '14px 22px', borderTop: `1px solid ${T.borderSoft}`, flexShrink: 0,
      }}>
        <div style={{ fontSize: T.type.meta, color: T.fgMuted, fontFamily: T.font.mono }}>
          {isSmart && rule.conditions.length
            ? `${rule.conditions.length} condition${rule.conditions.length !== 1 ? 's' : ''} · ${rule.actions.length} action${rule.actions.length !== 1 ? 's' : ''} · est. 142 messages`
            : `${rule.actions.length} action${rule.actions.length !== 1 ? 's' : ''}`}
        </div>
        <div style={{ flex: 1 }} />
        <ModalButton T={T} variant="ghost" onClick={onClose}>Cancel</ModalButton>
        <ModalButton T={T} variant="primary" onClick={onClose} kbd="⌘↵">
          {isSmart ? 'Save mailbox' : 'Save rules'}
        </ModalButton>
      </div>
    </Modal>
  );
}

// ───────────────────────────────────────────────── MATCH: visual

function MatchVisual({ T, rule, update }) {
  const setCombinator = (v) => update({ combinator: v });
  const addCond = () => update({
    conditions: [...rule.conditions, { id: crypto.randomUUID(), field: 'from', op: 'contains', value: '' }],
  });
  const setCond = (id, patch) => update({
    conditions: rule.conditions.map((c) => c.id === id ? { ...c, ...patch } : c),
  });
  const removeCond = (id) => update({
    conditions: rule.conditions.filter((c) => c.id !== id),
  });

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14 }}>
        <div style={{ fontSize: T.type.body, color: T.fg }}>Match messages where</div>
        <PhSelect T={T} value={rule.combinator} onChange={setCombinator}>
          <option value="all">all</option>
          <option value="any">any</option>
        </PhSelect>
        <div style={{ fontSize: T.type.body, color: T.fg }}>of these conditions are true:</div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {rule.conditions.map((c) => (
          <ConditionRow key={c.id} T={T} cond={c}
            onChange={(p) => setCond(c.id, p)}
            onRemove={() => removeCond(c.id)} />
        ))}
      </div>

      <button onClick={addCond} style={{
        marginTop: 12, display: 'inline-flex', alignItems: 'center', gap: 6,
        padding: '7px 12px', border: `1px dashed ${T.border}`, borderRadius: 8,
        background: 'transparent', color: T.fgMuted, cursor: 'pointer',
        fontFamily: 'inherit', fontSize: T.type.ui, fontWeight: 500,
      }}>
        <Icons.Plus size={14} /> Add condition
      </button>

      {/* Preview */}
      <div style={{ marginTop: 26, padding: 16, background: T.bg, borderRadius: 10, border: `1px solid ${T.borderSoft}` }}>
        <SectionLabel T={T} style={{ marginBottom: 10 }}>Preview · first 3 matches</SectionLabel>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          {PH_MESSAGES.slice(0, 3).map((m) => (
            <div key={m.id} style={{ display: 'flex', gap: 10, alignItems: 'center', fontSize: T.type.ui }}>
              <div style={{ width: 18, height: 18, borderRadius: 4, background: m.fromColor, color: '#fff', fontSize: 9, fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                {m.from.split(' ').map((x) => x[0]).join('').slice(0, 2)}
              </div>
              <span style={{ color: T.fg, fontWeight: 500, width: 140, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{m.from}</span>
              <span style={{ color: T.fgMuted, flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{m.subject}</span>
              <span style={{ color: T.fgFaint, fontFamily: T.font.mono, fontSize: T.type.meta }}>{m.dateShort}</span>
            </div>
          ))}
        </div>
        <div style={{ marginTop: 8, fontSize: T.type.meta, color: T.fgFaint, fontFamily: T.font.mono }}>
          + 139 more
        </div>
      </div>
    </div>
  );
}

function ConditionRow({ T, cond, onChange, onRemove }) {
  const field = PH_FIELDS.find((f) => f.id === cond.field) || PH_FIELDS[0];
  const ops = PH_OPS[field.type] || PH_OPS.text;
  return (
    <div style={{
      display: 'flex', gap: 8, alignItems: 'center',
      padding: 8, background: T.bgElev, borderRadius: 10,
      border: `1px solid ${T.borderSoft}`,
    }}>
      <PhSelect T={T} value={cond.field} onChange={(v) => onChange({ field: v })}>
        {PH_FIELDS.map((f) => <option key={f.id} value={f.id}>{f.label}</option>)}
      </PhSelect>
      <PhSelect T={T} value={cond.op} onChange={(v) => onChange({ op: v })}>
        {ops.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
      </PhSelect>
      {field.type === 'bool' ? (
        <PhSelect T={T} value={cond.value || 'true'} onChange={(v) => onChange({ value: v })}>
          <option value="true">yes</option>
          <option value="false">no</option>
        </PhSelect>
      ) : (
        <PhInput T={T} value={cond.value}
          onChange={(v) => onChange({ value: v })}
          placeholder={field.type === 'date' ? '2026-04-01' : field.type === 'number' ? 'MB' : 'value'}
          mono style={{ flex: 1 }} />
      )}
      <button onClick={onRemove} title="Remove" style={{
        width: 28, height: 28, borderRadius: 6, border: 'none', background: 'transparent',
        color: T.fgFaint, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}><Icons.X size={14} /></button>
    </div>
  );
}

// ───────────────────────────────────────────────── MATCH: raw JMAP filter

function conditionsToFilter(rule) {
  const lines = rule.conditions.map((c) => {
    const f = PH_FIELDS.find((x) => x.id === c.field);
    const op = c.op;
    const v = JSON.stringify(c.value ?? '');
    if (f?.type === 'bool') return `  ${c.field}: ${c.value || 'true'}`;
    return `  ${c.field} ${op} ${v}`;
  });
  return `{\n  operator: "${rule.combinator === 'all' ? 'AND' : 'OR'}",\n${lines.join(',\n')}\n}`;
}

function MatchRaw({ T, rule }) {
  const text = conditionsToFilter(rule);
  return (
    <div>
      <div style={{ fontSize: T.type.body, color: T.fg, marginBottom: 10 }}>
        JMAP filter expression — edit directly to use full query power.
      </div>
      <textarea
        defaultValue={text}
        spellCheck={false}
        style={{
          width: '100%', height: 220, resize: 'vertical',
          padding: 14, background: T.bg, color: T.fg,
          border: `1px solid ${T.border}`, borderRadius: 10,
          fontFamily: T.font.mono, fontSize: T.type.body, lineHeight: 1.55,
          outline: 'none', tabSize: 2,
        }} />
      <div style={{ marginTop: 12, padding: 12, background: T.bg, borderRadius: 8,
        border: `1px solid ${T.borderSoft}`, fontSize: T.type.meta,
        color: T.fgMuted, fontFamily: T.font.mono, lineHeight: 1.6 }}>
        <div style={{ color: T.fg, fontWeight: 600, marginBottom: 4 }}>Available fields</div>
        from, to, cc, subject, body, thread, list, hasAttachment, size, date, tag, unread, flagged, inMailbox<br/>
        <span style={{ color: T.fg, fontWeight: 600 }}>Operators</span> AND · OR · NOT · contains · eq · neq · startsWith · matches · {'>'} · {'<'} · after · before · last
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────── THEN: actions

function ThenEditor({ T, rule, update }) {
  const addAction = (type) => update({
    actions: [...rule.actions, { id: crypto.randomUUID(), type, value: '' }],
  });
  const setAction = (id, patch) => update({
    actions: rule.actions.map((a) => a.id === id ? { ...a, ...patch } : a),
  });
  const removeAction = (id) => update({
    actions: rule.actions.filter((a) => a.id !== id),
  });

  const usedTypes = new Set(rule.actions.map((a) => a.type));
  const available = PH_ACTIONS.filter((a) => !usedTypes.has(a.id) || a.id === 'tag' || a.id === 'forward');

  return (
    <div>
      <div style={{ fontSize: T.type.body, color: T.fg, marginBottom: 14 }}>
        When a message matches, do the following in order:
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {rule.actions.map((a, i) => (
          <ActionRow key={a.id} T={T} action={a} index={i}
            onChange={(p) => setAction(a.id, p)}
            onRemove={() => removeAction(a.id)} />
        ))}
      </div>

      {/* Add-action picker */}
      <div style={{ marginTop: 14 }}>
        <SectionLabel T={T} style={{ marginBottom: 8 }}>Add action</SectionLabel>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {available.map((def) => {
            const Ico = Icons[def.icon] || Icons.Bolt;
            return (
              <button key={def.id} onClick={() => addAction(def.id)} style={{
                display: 'inline-flex', alignItems: 'center', gap: 6,
                padding: '6px 10px', borderRadius: 999,
                background: T.bgElev, border: `1px solid ${T.borderSoft}`,
                color: T.fg, fontFamily: 'inherit', fontSize: T.type.ui, fontWeight: 500,
                cursor: 'pointer',
              }}>
                <Ico size={13} style={{ color: T.fgMuted }} />
                {def.label}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function ActionRow({ T, action, index, onChange, onRemove }) {
  const def = PH_ACTIONS.find((a) => a.id === action.type) || PH_ACTIONS[0];
  const Ico = Icons[def.icon] || Icons.Bolt;
  return (
    <div style={{
      display: 'flex', gap: 10, alignItems: 'center',
      padding: 10, background: T.bgElev, borderRadius: 10,
      border: `1px solid ${T.borderSoft}`,
    }}>
      <div style={{
        width: 22, height: 22, borderRadius: 5, flexShrink: 0,
        background: T.accent.coralSoft, color: T.accent.coralDeep,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}><Ico size={13} /></div>
      <div style={{ fontSize: T.type.body, fontWeight: 600, color: T.fg, width: 130, flexShrink: 0 }}>
        {def.label}
      </div>
      {def.arg && (
        <PhInput T={T} value={action.value}
          onChange={(v) => onChange({ value: v })}
          placeholder={
            def.arg === 'email' ? 'name@example.com' :
            def.arg === 'url' ? 'https://...' :
            def.arg === 'enum' ? (def.id === 'tag' ? 'tag name' : 'mailbox') :
            'value'
          }
          style={{ flex: 1 }} />
      )}
      {!def.arg && <div style={{ flex: 1 }} />}
      <button onClick={onRemove} title="Remove" style={{
        width: 28, height: 28, borderRadius: 6, border: 'none', background: 'transparent',
        color: T.fgFaint, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}><Icons.X size={14} /></button>
    </div>
  );
}

// ───────────────────────────────────────────────── Icon + color picker

const PICK_ICONS = ['Bolt','Tag','Star','Flag','Folder','Mail','Inbox','Sparkle','Bell','Calendar','Pin','Shield'];
const PICK_ACCENTS = ['coral', 'blue', 'sage', 'amber', 'violet', 'rose'];

function IconPicker({ T, value, accent, onChange }) {
  const [open, setOpen] = React.useState(false);
  const Ico = Icons[value] || Icons.Bolt;
  return (
    <div style={{ position: 'relative' }}>
      <button onClick={() => setOpen(!open)} style={{
        width: 36, height: 36, borderRadius: 10,
        background: `color-mix(in srgb, ${T.accent[accent] || T.accent.coral} 18%, transparent)`,
        color: T.accent[accent] || T.accent.coral,
        border: 'none', cursor: 'pointer',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}><Ico size={18} /></button>
      {open && (
        <div style={{
          position: 'absolute', top: 44, left: 0, zIndex: 20,
          background: T.bgElev, border: `1px solid ${T.border}`,
          borderRadius: 10, padding: 10, boxShadow: T.shadow,
          width: 220,
        }}>
          <SectionLabel T={T} style={{ marginBottom: 6 }}>Icon</SectionLabel>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(6, 1fr)', gap: 4, marginBottom: 10 }}>
            {PICK_ICONS.map((ic) => {
              const I = Icons[ic] || Icons.Bolt;
              return (
                <button key={ic} onClick={() => onChange(ic, accent)} style={{
                  width: 30, height: 30, borderRadius: 7,
                  background: value === ic ? T.accent.coralSoft : 'transparent',
                  color: value === ic ? T.accent.coralDeep : T.fg,
                  border: 'none', cursor: 'pointer',
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                }}><I size={14} /></button>
              );
            })}
          </div>
          <SectionLabel T={T} style={{ marginBottom: 6 }}>Color</SectionLabel>
          <div style={{ display: 'flex', gap: 6 }}>
            {PICK_ACCENTS.map((a) => (
              <button key={a} onClick={() => onChange(value, a)} style={{
                width: 22, height: 22, borderRadius: '50%',
                background: T.accent[a], cursor: 'pointer',
                border: accent === a ? `2px solid ${T.fg}` : `2px solid transparent`,
              }} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

Object.assign(window, { MailboxEditor });
