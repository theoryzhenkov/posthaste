// Full Settings page — sidebar rail + content pane.
// Opens as a full-screen overlay (glass blur backdrop) from gear icon / ⌘,.

const PH_SETTINGS_SECTIONS = [
  { id: 'accounts',  label: 'Accounts',           icon: 'At' },
  { id: 'mailboxes', label: 'Mailboxes & Rules',  icon: 'Folder' },
  { id: 'appearance',label: 'Appearance',         icon: 'Sparkle' },
  { id: 'signatures',label: 'Signatures',         icon: 'Edit' },
  { id: 'privacy',   label: 'Privacy & Tracking', icon: 'Shield' },
  { id: 'shortcuts', label: 'Keyboard',           icon: 'Keyboard' },
  { id: 'automation',label: 'Automation',         icon: 'Zap' },
  { id: 'about',     label: 'About',              icon: 'Info' },
];

function SettingsSheet({ T, onClose, onOpenEditor }) {
  const [section, setSection] = React.useState('accounts');
  // Esc to close
  React.useEffect(() => {
    const h = (e) => { if (e.key === 'Escape') { e.preventDefault(); onClose(); } };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [onClose]);
  const isDark = (T.mode || 'dark') === 'dark';
  return (
    <div
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
      style={{
      position: 'absolute', inset: 0, zIndex: 2100,
      background: isDark ? 'rgba(6,4,12,0.55)' : 'rgba(40,30,60,0.35)',
      backdropFilter: 'blur(18px) saturate(140%)',
      WebkitBackdropFilter: 'blur(18px) saturate(140%)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      padding: 32,
      animation: 'ph-modal-in 0.18s ease-out',
    }}>
      <div style={{
        width: '100%', maxWidth: 1080, height: '100%', maxHeight: 760,
        background: isDark ? 'rgba(22,20,28,0.88)' : 'rgba(253,252,250,0.94)',
        border: `1px solid ${T.border}`, borderRadius: 16,
        boxShadow: isDark
          ? '0 32px 80px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.04) inset'
          : '0 32px 80px rgba(40,30,60,0.22), 0 0 0 1px rgba(255,255,255,0.6) inset',
        display: 'flex', overflow: 'hidden',
        fontFamily: T.font.sans, color: T.fg,
        animation: 'ph-sheet-in 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.0)',
      }}>
        {/* Rail */}
        <div style={{
          width: 220, flexShrink: 0, background: 'rgba(0,0,0,0.12)',
          borderRight: `1px solid ${T.borderSoft}`,
          display: 'flex', flexDirection: 'column', padding: '16px 0',
        }}>
          <div style={{ padding: '0 16px 14px', display: 'flex', alignItems: 'center', gap: 8 }}>
            <PostmarkStamp size={20} color={T.accent.coral} />
            <div style={{ fontSize: T.type.head, fontWeight: 700, letterSpacing: -0.3 }}>Settings</div>
          </div>
          {PH_SETTINGS_SECTIONS.map((s) => {
            const Ico = Icons[s.icon] || Icons.Dot;
            const active = s.id === section;
            return (
              <button key={s.id} onClick={() => setSection(s.id)} style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '8px 14px', margin: '1px 8px', borderRadius: 7,
                border: 'none', textAlign: 'left', cursor: 'pointer',
                background: active ? T.accent.coralSoft : 'transparent',
                color: active ? T.accent.coralDeep : T.fg,
                fontFamily: 'inherit', fontSize: T.type.body, fontWeight: active ? 600 : 500,
              }}>
                <Ico size={15} style={{ color: active ? T.accent.coralDeep : T.fgMuted }} />
                {s.label}
              </button>
            );
          })}
          <div style={{ flex: 1 }} />
          <div style={{ padding: '12px 16px', fontSize: T.type.meta, color: T.fgFaint, fontFamily: T.font.mono }}>
            v1.0.0 · JMAP 0.3
          </div>
        </div>

        {/* Content */}
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', padding: '14px 22px',
            borderBottom: `1px solid ${T.borderSoft}` }}>
            <div style={{ fontSize: T.type.head, fontWeight: 700, letterSpacing: -0.3 }}>
              {PH_SETTINGS_SECTIONS.find((s) => s.id === section).label}
            </div>
            <div style={{ flex: 1 }} />
            <button onClick={onClose} style={{
              width: 30, height: 30, borderRadius: 8, border: 'none', background: 'transparent',
              color: T.fgMuted, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center',
            }}><Icons.X size={16} /></button>
          </div>
          <div className="ph-scroll" style={{ flex: 1, overflow: 'auto', padding: 22 }}>
            {section === 'accounts'   && <SAccounts T={T} />}
            {section === 'mailboxes'  && <SMailboxes T={T} onOpenEditor={onOpenEditor} />}
            {section === 'appearance' && <SAppearance T={T} />}
            {section === 'signatures' && <SSignatures T={T} />}
            {section === 'privacy'    && <SPrivacy T={T} />}
            {section === 'shortcuts'  && <SShortcuts T={T} />}
            {section === 'automation' && <SAutomation T={T} />}
            {section === 'about'      && <SAbout T={T} />}
          </div>
        </div>
      </div>
    </div>
  );
}

function SAccounts({ T }) {
  return (
    <div>
      <SectionLabel T={T} style={{ marginBottom: 12 }}>Connected accounts</SectionLabel>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginBottom: 20 }}>
        {PH_ACCOUNTS.map((a) => (
          <div key={a.id} style={{
            display: 'flex', alignItems: 'center', gap: 12,
            padding: 14, background: T.bgElev, borderRadius: 10,
            border: `1px solid ${T.borderSoft}`,
          }}>
            <div style={{ width: 38, height: 38, borderRadius: 8, background: a.color,
              color: '#fff', fontWeight: 700, fontFamily: T.font.mono, fontSize: 13,
              display: 'flex', alignItems: 'center', justifyContent: 'center' }}>{a.stamp}</div>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: T.type.body, fontWeight: 600 }}>{a.label}</div>
              <div style={{ fontSize: T.type.meta, color: T.fgMuted, fontFamily: T.font.mono }}>
                JMAP · sync ok · 12 mailboxes
              </div>
            </div>
            <ModalButton T={T} variant="ghost">Edit</ModalButton>
            <ModalButton T={T} variant="ghost" icon={Icons.Trash}>Remove</ModalButton>
          </div>
        ))}
      </div>
      <ModalButton T={T} variant="primary" icon={Icons.Plus}>Add account</ModalButton>
    </div>
  );
}

function SMailboxes({ T, onOpenEditor }) {
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', marginBottom: 14 }}>
        <SectionLabel T={T}>Smart mailboxes</SectionLabel>
        <div style={{ flex: 1 }} />
        <ModalButton T={T} variant="primary" icon={Icons.Plus}
          onClick={() => onOpenEditor && onOpenEditor('smart')}>New smart mailbox</ModalButton>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 24 }}>
        {PH_SMART_MAILBOXES.map((sm) => {
          const Ico = Icons[sm.icon] || Icons.Bolt;
          return (
            <div key={sm.id} style={{
              display: 'flex', alignItems: 'center', gap: 12,
              padding: 12, background: T.bgElev, borderRadius: 10,
              border: `1px solid ${T.borderSoft}`, cursor: 'pointer',
            }} onClick={() => onOpenEditor && onOpenEditor('smart', sm)}>
              <div style={{ width: 30, height: 30, borderRadius: 8,
                background: `color-mix(in srgb, ${T.accent[sm.accent] || T.accent.coral} 20%, transparent)`,
                color: T.accent[sm.accent] || T.accent.coral,
                display: 'flex', alignItems: 'center', justifyContent: 'center' }}><Ico size={14} /></div>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: T.type.body, fontWeight: 600 }}>{sm.name}</div>
                <div style={{ fontSize: T.type.meta, color: T.fgMuted, fontFamily: T.font.mono }}>
                  {sm.unread || 0} unread · 3 match conditions · 2 actions
                </div>
              </div>
              <Icons.Chevron size={14} style={{ color: T.fgFaint }} />
            </div>
          );
        })}
      </div>
      <SectionLabel T={T} style={{ marginBottom: 12 }}>Folder rules</SectionLabel>
      <div style={{ fontSize: T.type.ui, color: T.fgMuted, marginBottom: 14 }}>
        Attach rules to regular mailboxes (e.g. auto-tag everything in Work/Engineering).
      </div>
      <ModalButton T={T} variant="secondary" icon={Icons.Plus}
        onClick={() => onOpenEditor && onOpenEditor('mailbox')}>Add rules to mailbox</ModalButton>
    </div>
  );
}

function SAppearance({ T }) {
  const [theme, setTheme] = React.useState('dark');
  return (
    <div>
      <FormRow T={T} label="Theme mode" hint="System-matching, always light, or always dark.">
        <div style={{ display: 'flex', gap: 6 }}>
          {['system', 'light', 'dark'].map((m) => (
            <button key={m} onClick={() => setTheme(m)} style={{
              padding: '6px 14px', border: `1px solid ${theme === m ? T.accent.coral : T.border}`,
              borderRadius: 7, background: theme === m ? T.accent.coralSoft : T.bg,
              color: theme === m ? T.accent.coralDeep : T.fg, cursor: 'pointer',
              fontFamily: 'inherit', fontSize: T.type.ui, fontWeight: 500, textTransform: 'capitalize',
            }}>{m}</button>
          ))}
        </div>
      </FormRow>
      <FormRow T={T} label="Density" hint="How tightly rows pack.">
        <PhSelect T={T}><option>Compact</option><option>Standard</option><option>Roomy</option></PhSelect>
      </FormRow>
      <FormRow T={T} label="Zebra rows" hint="Alternate row tint for scanability.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
      <FormRow T={T} label="Show avatars" hint="Colored stamp next to each message.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
      <FormRow T={T} label="Inline reader" hint="Default: open selected message in the right pane.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
    </div>
  );
}

function SSignatures({ T }) {
  return (
    <div>
      <FormRow T={T} label="Default signature" stack>
        <textarea defaultValue={"— Theo\nposthaste.app"} style={{
          width: '100%', minHeight: 100, padding: 12, borderRadius: 10,
          border: `1px solid ${T.border}`, background: T.bg, color: T.fg,
          fontFamily: T.font.mono, fontSize: T.type.body, outline: 'none',
        }} />
      </FormRow>
      <FormRow T={T} label="Per-account signatures" hint="Configure different signatures for each account.">
        <ModalButton T={T} variant="secondary">Configure</ModalButton>
      </FormRow>
    </div>
  );
}

function SPrivacy({ T }) {
  return (
    <div>
      <FormRow T={T} label="Block remote images by default" hint="Images load only when you click Show images. Blocks tracking pixels.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
      <FormRow T={T} label="Strip tracking pixels" hint="Detected pixels are removed before display.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
      <FormRow T={T} label="Block read receipts" hint="Never confirm you opened a message.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
      <FormRow T={T} label="Sandbox HTML" hint="Render messages in an iframe with no script, no fetch, no storage.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
      <FormRow T={T} label="Warn on external links" hint="Show confirm dialog for unexpected domains.">
        <PhSwitch T={T} checked={false} />
      </FormRow>
    </div>
  );
}

function SShortcuts({ T }) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 24 }}>
      {PH_SHORTCUTS && PH_SHORTCUTS.map((grp) => (
        <div key={grp.group}>
          <SectionLabel T={T} style={{ marginBottom: 8 }}>{grp.group}</SectionLabel>
          {grp.items.map(([k, d]) => (
            <div key={k} style={{ display: 'flex', gap: 10, padding: '4px 0',
              fontSize: T.type.body, color: T.fg, alignItems: 'center' }}>
              <span style={{ display: 'flex', gap: 3, minWidth: 90 }}>
                {k.split(' ').map((x, i) => <Kbd key={i} T={T}>{x}</Kbd>)}
              </span>
              <span style={{ color: T.fgMuted }}>{d}</span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

function SAutomation({ T }) {
  return (
    <div>
      <FormRow T={T} label="Enable AI features" hint="Summaries, smart reply drafts, content extraction. Runs locally on-device when possible.">
        <PhSwitch T={T} checked={true} />
      </FormRow>
      <FormRow T={T} label="AI provider" hint="Default: on-device. Switch to cloud for higher quality.">
        <PhSelect T={T}><option>On-device (MLX)</option><option>Claude (haiku)</option><option>OpenAI</option></PhSelect>
      </FormRow>
      <FormRow T={T} label="Webhook URL" hint="POST on new message for integrations (Zapier, custom).">
        <PhInput T={T} placeholder="https://hooks.example.com/mail" mono />
      </FormRow>
      <FormRow T={T} label="Shell command on match" hint="Run a local command when a rule fires. Security-sensitive.">
        <PhInput T={T} placeholder="/usr/local/bin/notify" mono />
      </FormRow>
    </div>
  );
}

function SAbout({ T }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 20, padding: '20px 0' }}>
      <PostmarkStamp size={72} color={T.accent.coral} />
      <div>
        <div style={{ fontSize: 22, fontWeight: 700, letterSpacing: -0.4 }}>Posthaste</div>
        <div style={{ fontSize: T.type.body, color: T.fgMuted, marginTop: 4 }}>
          A modern JMAP client · v1.0.0
        </div>
        <div style={{ marginTop: 14, display: 'flex', gap: 8 }}>
          <ModalButton T={T} variant="ghost" icon={Icons.Globe}>Website</ModalButton>
          <ModalButton T={T} variant="ghost" icon={Icons.GitBranch}>Changelog</ModalButton>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { SettingsSheet });
