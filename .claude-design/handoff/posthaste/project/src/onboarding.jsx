// Onboarding / add-account flow — glass modal, 3 steps.

function Onboarding({ T, onClose }) {
  const [step, setStep] = React.useState(0);
  const [provider, setProvider] = React.useState('fastmail');

  const providers = [
    { id: 'fastmail', label: 'Fastmail',    hint: 'Native JMAP · fastest', icon: 'Bolt',  color: '#2563eb' },
    { id: 'jmap',     label: 'Other JMAP',  hint: 'Stalwart, Topicus, …',  icon: 'At',    color: '#d97706' },
    { id: 'gmail',    label: 'Gmail',       hint: 'via JMAP proxy',        icon: 'Mail',  color: '#db4437' },
    { id: 'imap',     label: 'IMAP / SMTP', hint: 'Any standard mailbox',  icon: 'Globe', color: '#7c3aed' },
  ];

  return (
    <Modal T={T} onClose={onClose} width={640} height={560}>
      {/* Progress */}
      <div style={{ padding: '22px 28px 0', display: 'flex', gap: 6 }}>
        {[0, 1, 2].map((i) => (
          <div key={i} style={{
            flex: 1, height: 3, borderRadius: 2,
            background: i <= step ? T.accent.coral : T.borderSoft,
            transition: 'background 0.2s',
          }} />
        ))}
      </div>

      <div style={{ flex: 1, padding: '28px 32px', overflow: 'auto' }} className="ph-scroll">
        {step === 0 && (
          <div>
            <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.4, marginBottom: 6 }}>
              Add your mailbox
            </div>
            <div style={{ fontSize: T.type.body, color: T.fgMuted, marginBottom: 22 }}>
              Pick a provider. JMAP is preferred — it's faster, saves battery, and supports smart mailboxes natively.
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
              {providers.map((p) => {
                const Ico = Icons[p.icon] || Icons.At;
                const active = provider === p.id;
                return (
                  <button key={p.id} onClick={() => setProvider(p.id)} style={{
                    display: 'flex', alignItems: 'center', gap: 12, padding: 14,
                    borderRadius: 12, border: `1.5px solid ${active ? T.accent.coral : T.border}`,
                    background: active ? T.accent.coralSoft : T.bgElev, cursor: 'pointer',
                    textAlign: 'left', color: T.fg, fontFamily: 'inherit',
                  }}>
                    <div style={{ width: 34, height: 34, borderRadius: 8, background: p.color,
                      color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center',
                      flexShrink: 0 }}><Ico size={16} /></div>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ fontSize: T.type.body, fontWeight: 600 }}>{p.label}</div>
                      <div style={{ fontSize: T.type.meta, color: T.fgMuted }}>{p.hint}</div>
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        )}

        {step === 1 && (
          <div>
            <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.4, marginBottom: 6 }}>
              Sign in
            </div>
            <div style={{ fontSize: T.type.body, color: T.fgMuted, marginBottom: 22 }}>
              Your credentials are stored encrypted in the macOS keychain. We never see them.
            </div>
            <FormRow T={T} label="Email address" stack>
              <PhInput T={T} placeholder="you@fastmail.com" />
            </FormRow>
            <FormRow T={T} label="App password" hint="Generate at fastmail.com/settings/security" stack>
              <PhInput T={T} placeholder="xxxx-xxxx-xxxx-xxxx" mono />
            </FormRow>
            {provider === 'imap' && (
              <>
                <FormRow T={T} label="IMAP host" stack><PhInput T={T} placeholder="imap.example.com" mono /></FormRow>
                <FormRow T={T} label="SMTP host" stack><PhInput T={T} placeholder="smtp.example.com" mono /></FormRow>
              </>
            )}
          </div>
        )}

        {step === 2 && (
          <div>
            <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.4, marginBottom: 6 }}>
              Syncing your mailboxes
            </div>
            <div style={{ fontSize: T.type.body, color: T.fgMuted, marginBottom: 22 }}>
              Pulling headers for Inbox, Sent, Drafts. Full-text indexing runs in the background.
            </div>
            {[
              ['Inbox',     '2,847 messages', 100],
              ['Sent',      '1,203 messages',  78],
              ['Drafts',    '17 messages',    100],
              ['Archive',   '28,912 messages', 34],
              ['Work',      '4,812 messages',  12],
            ].map(([name, count, pct]) => (
              <div key={name} style={{ padding: '10px 0', borderBottom: `1px solid ${T.borderSoft}` }}>
                <div style={{ display: 'flex', alignItems: 'center', marginBottom: 6 }}>
                  <Icons.Folder size={14} style={{ color: T.fgMuted, marginRight: 8 }} />
                  <span style={{ fontSize: T.type.body, fontWeight: 500 }}>{name}</span>
                  <div style={{ flex: 1 }} />
                  <span style={{ fontSize: T.type.meta, fontFamily: T.font.mono, color: T.fgMuted }}>{count}</span>
                  <span style={{ fontSize: T.type.meta, fontFamily: T.font.mono, color: pct === 100 ? T.accent.sage : T.fgMuted, marginLeft: 10, minWidth: 32, textAlign: 'right' }}>{pct}%</span>
                </div>
                <div style={{ height: 4, borderRadius: 2, background: T.borderSoft, overflow: 'hidden' }}>
                  <div style={{ width: `${pct}%`, height: '100%',
                    background: pct === 100 ? T.accent.sage : T.accent.coral, transition: 'width 0.3s' }} />
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <ModalFooter T={T}>
        {step > 0 && <ModalButton T={T} variant="ghost" onClick={() => setStep(step - 1)}>Back</ModalButton>}
        {step < 2 && <ModalButton T={T} variant="primary" onClick={() => setStep(step + 1)}>
          {step === 0 ? 'Continue' : 'Connect'}
        </ModalButton>}
        {step === 2 && <ModalButton T={T} variant="primary" onClick={onClose}>Start using Posthaste</ModalButton>}
      </ModalFooter>
    </Modal>
  );
}

Object.assign(window, { Onboarding });
