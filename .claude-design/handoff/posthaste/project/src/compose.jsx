// Posthaste compose modal — polished with schedule send, tracking toggle,
// follow-up, templates, and inline AI assist.

const PH_TEMPLATES = [
  { id: 't1', name: 'Quick thanks', body: 'Thanks — got it. Will circle back shortly.\n\n— Theo' },
  { id: 't2', name: 'Intro + 3-line update', body: 'Hey {{name}},\n\nQuick update:\n• {{x}}\n• {{y}}\n• {{z}}\n\nLet me know if any of this warrants a deeper sync.\n\n— Theo' },
  { id: 't3', name: 'Decline politely', body: 'Appreciate the offer. Not a fit for us right now, but I\'ll keep you in mind if that changes.\n\n— Theo' },
  { id: 't4', name: 'Scheduling',    body: 'Here are a few times that work on my side:\n  • Tue 2–3pm PT\n  • Wed 10am–12pm PT\n  • Thu 4–5pm PT\n\nLet me know what works.\n\n— Theo' },
];

const PH_AI_ACTIONS = [
  { id: 'concise',   label: 'Make concise',      icon: 'Sparkle' },
  { id: 'softer',    label: 'Soften tone',       icon: 'Sparkle' },
  { id: 'confident', label: 'More confident',    icon: 'Sparkle' },
  { id: 'bullets',   label: 'Rewrite as bullets',icon: 'Sparkle' },
  { id: 'grammar',   label: 'Fix grammar',       icon: 'Check' },
];

const PH_SCHEDULE_PRESETS = [
  { id: 'later',   label: 'Later today',    sub: 'Today · 6:00 PM' },
  { id: 'tomorrow',label: 'Tomorrow 9 AM',  sub: 'Wed · 9:00 AM' },
  { id: 'monday',  label: 'Monday 9 AM',    sub: 'Apr 27 · 9:00 AM' },
  { id: 'week',    label: 'Next week',      sub: 'Mon · 9:00 AM' },
  { id: 'custom',  label: 'Pick a time…',   sub: '' },
];

const PH_FOLLOWUP_PRESETS = [
  { id: '1d', label: 'Tomorrow' },
  { id: '3d', label: 'In 3 days' },
  { id: '1w', label: 'Next week' },
  { id: 'custom', label: 'Custom…' },
];

function Compose({ T, onClose }) {
  const [to, setTo] = React.useState('maya@lumen.studio');
  const [cc, setCc] = React.useState('');
  const [subject, setSubject] = React.useState('Re: Pentagram review · follow-up on specimen set');
  const [body, setBody] = React.useState(
    'Maya,\n\nQuick follow-up on yesterday\'s review. A few thoughts on the specimen set:\n\n• The display cut at 96pt has real presence — keep it.\n• The numerals in the text weight feel a touch cramped at the tabular widths.\n• I\'d love to see one more optical size between 18 and 32.\n\nHappy to walk through on Thursday if useful.\n\n— Theo'
  );
  const [showCc, setShowCc] = React.useState(false);
  const [sending, setSending] = React.useState(false);
  const [sent, setSent] = React.useState(false);

  // Compose options
  const [tracking, setTracking] = React.useState(false);      // read receipt
  const [encrypt, setEncrypt] = React.useState(false);        // S/MIME placeholder
  const [scheduleAt, setScheduleAt] = React.useState(null);   // label or null
  const [followUp, setFollowUp] = React.useState(null);       // '3d' | …
  const [showSchedule, setShowSchedule] = React.useState(false);
  const [showFollow, setShowFollow] = React.useState(false);
  const [showTemplates, setShowTemplates] = React.useState(false);
  const [showAI, setShowAI] = React.useState(false);
  const [aiBusy, setAiBusy] = React.useState(null);

  // Attachments
  const [attachments, setAttachments] = React.useState([
    { name: 'fig-typeface-specimens-v3.pdf', size: '2.4 MB', kind: 'pdf' },
  ]);

  const send = () => {
    setSending(true);
    setTimeout(() => { setSent(true); setTimeout(onClose, 1100); }, 400);
  };

  const runAI = (id) => {
    setAiBusy(id); setShowAI(false);
    setTimeout(() => {
      if (id === 'concise') {
        setBody('Maya,\n\nThree quick notes on the specimens:\n\n• Keep the 96pt display cut.\n• Text-weight numerals feel cramped at tabular widths.\n• One more optical size between 18 and 32 would help.\n\nThursday works if you want to walk through.\n\n— Theo');
      } else if (id === 'bullets') {
        setBody('Maya,\n\n• 96pt display — strong, keep it.\n• Text-weight tabular numerals — cramped.\n• Optical size gap between 18 and 32 — add one.\n\nThursday walkthrough? up to you.\n\n— Theo');
      } else if (id === 'softer') {
        setBody('Hi Maya,\n\nThanks so much for yesterday — really enjoyable review. A few gentle thoughts on the specimens, no rush:\n\n• The 96pt display cut has so much presence, would love to keep it.\n• The numerals in the text weight might feel a touch cramped at tabular widths.\n• An intermediate optical size between 18 and 32 might be worth exploring.\n\nHappy to walk through on Thursday if that would help!\n\n— Theo');
      }
      setAiBusy(null);
    }, 900);
  };

  return (
    <div style={{
      position: 'absolute', inset: 0, background: 'rgba(10,8,6,0.5)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      zIndex: 100, backdropFilter: 'blur(4px) saturate(140%)',
      WebkitBackdropFilter: 'blur(4px) saturate(140%)',
    }} onClick={onClose}>
      <div onClick={(e) => e.stopPropagation()} style={{
        width: 700, maxWidth: '92%', maxHeight: '92%',
        background: T.bgReader,
        borderRadius: 12, border: `1px solid ${T.border}`,
        boxShadow: '0 28px 80px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.03) inset',
        display: 'flex', flexDirection: 'column',
        overflow: 'visible', position: 'relative',
      }}>
        {/* Header */}
        <div style={{
          height: 40, display: 'flex', alignItems: 'center',
          padding: '0 14px', gap: 10,
          background: T.bgTitlebar,
          borderBottom: `1px solid ${T.borderSoft}`,
          borderRadius: '12px 12px 0 0',
        }}>
          <div style={{ display: 'flex', gap: 6 }}>
            <div onClick={onClose} style={{ width: 12, height: 12, borderRadius: '50%', background: '#ff5f57', cursor: 'pointer' }} />
            <div style={{ width: 12, height: 12, borderRadius: '50%', background: '#febc2e' }} />
            <div style={{ width: 12, height: 12, borderRadius: '50%', background: '#28c940' }} />
          </div>
          <div style={{ flex: 1, textAlign: 'center', fontSize: 12, fontWeight: 600, color: T.fgMuted, letterSpacing: 0.2 }}>
            New Message
          </div>
          {scheduleAt && <ComposeBadge T={T} icon={Icons.Calendar} color={T.accent.sage} text={`Scheduled · ${scheduleAt.label}`} onClear={() => setScheduleAt(null)} />}
          {followUp && <ComposeBadge T={T} icon={Icons.Snooze} color={T.accent.amber} text={`Follow up ${PH_FOLLOWUP_PRESETS.find(f => f.id === followUp)?.label}`} onClear={() => setFollowUp(null)} />}
          {tracking && <ComposeBadge T={T} icon={Icons.Eye} color={T.accent.blue} text="Tracked" onClear={() => setTracking(false)} />}
          {encrypt && <ComposeBadge T={T} icon={Icons.Lock} color={T.accent.coral} text="Encrypted" onClear={() => setEncrypt(false)} />}
        </div>

        {/* Field rows */}
        <ComposeField T={T} label="From">
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12.5, color: T.fg }}>
            <div style={{ width: 14, height: 14, borderRadius: 3, background: PH_ACCOUNTS[0].color, color: '#fff', fontSize: 9, fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center', fontFamily: T.font.mono }}>G</div>
            theor@gmail.com
            <Icons.ChevronDown2 style={{ color: T.fgFaint, width: 10, height: 10 }} />
          </div>
        </ComposeField>
        <ComposeField T={T} label="To">
          <input value={to} onChange={(e) => setTo(e.target.value)}
            placeholder="Recipients…"
            style={{
              flex: 1, border: 'none', outline: 'none', background: 'transparent',
              fontFamily: T.font.sans, fontSize: 13, color: T.fg,
            }} />
          {!showCc && (
            <button onClick={() => setShowCc(true)} style={{
              border: 'none', background: 'transparent', color: T.fgFaint,
              fontSize: 11, cursor: 'pointer', padding: '2px 6px', fontFamily: 'inherit',
            }}>+ Cc/Bcc</button>
          )}
        </ComposeField>
        {showCc && (
          <ComposeField T={T} label="Cc">
            <input value={cc} onChange={(e) => setCc(e.target.value)}
              style={{ flex: 1, border: 'none', outline: 'none', background: 'transparent', fontFamily: T.font.sans, fontSize: 13, color: T.fg }} />
          </ComposeField>
        )}
        <ComposeField T={T} label="Subject">
          <input value={subject} onChange={(e) => setSubject(e.target.value)}
            placeholder="Subject"
            style={{ flex: 1, border: 'none', outline: 'none', background: 'transparent', fontFamily: T.font.sans, fontSize: 13.5, color: T.fg, fontWeight: 500 }} />
        </ComposeField>

        {/* Body */}
        <div style={{ position: 'relative', flex: 1, minHeight: 220 }}>
          <textarea value={body} onChange={(e) => setBody(e.target.value)}
            placeholder="Write your message…"
            style={{
              width: '100%', height: '100%', minHeight: 220,
              border: 'none', outline: 'none', resize: 'none',
              padding: '14px 16px 8px',
              fontFamily: T.font.sans, fontSize: 13.5, lineHeight: 1.55,
              color: T.fg, background: T.bgReader,
              opacity: aiBusy ? 0.45 : 1, transition: 'opacity 0.15s',
            }} />
          {aiBusy && (
            <div style={{
              position: 'absolute', top: 14, right: 16,
              padding: '4px 10px', borderRadius: 999,
              background: T.accent.coralSoft, color: T.accent.coralDeep,
              fontSize: 11, fontFamily: T.font.mono, fontWeight: 600,
              display: 'flex', alignItems: 'center', gap: 6,
            }}>
              <span style={{ width: 6, height: 6, borderRadius: '50%', background: T.accent.coralDeep, animation: 'ph-pulse 1s infinite' }} />
              Rewriting · {PH_AI_ACTIONS.find((a) => a.id === aiBusy)?.label}
            </div>
          )}
        </div>

        {/* Attachment chips */}
        {attachments.length > 0 && (
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', padding: '0 14px 10px' }}>
            {attachments.map((a, i) => (
              <div key={i} style={{
                display: 'inline-flex', alignItems: 'center', gap: 6,
                padding: '4px 4px 4px 8px', borderRadius: 6,
                background: T.bgElev, border: `1px solid ${T.borderSoft}`,
                fontSize: 11.5, color: T.fg,
              }}>
                <Icons.Attach size={11} style={{ color: T.fgMuted }} />
                <span>{a.name}</span>
                <span style={{ fontFamily: T.font.mono, color: T.fgFaint, fontSize: 10 }}>{a.size}</span>
                <button onClick={() => setAttachments(attachments.filter((_, j) => j !== i))}
                  style={{
                    width: 16, height: 16, border: 'none', background: 'transparent',
                    cursor: 'pointer', color: T.fgFaint, display: 'flex', alignItems: 'center', justifyContent: 'center',
                    borderRadius: 3,
                  }}><Icons.X size={10} /></button>
              </div>
            ))}
          </div>
        )}

        {/* Footer */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 6,
          padding: '10px 12px',
          borderTop: `1px solid ${T.borderSoft}`,
          background: T.bgElev,
          borderRadius: '0 0 12px 12px',
          position: 'relative',
        }}>
          {/* Send split button */}
          <div style={{ display: 'flex', borderRadius: 6, overflow: 'hidden', boxShadow: '0 1px 2px rgba(0,0,0,0.15)' }}>
            <button onClick={send} disabled={sending} style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '7px 14px', border: 'none',
              background: T.accent.coral, color: '#fff',
              cursor: sending ? 'default' : 'pointer',
              fontSize: 12.5, fontWeight: 600, fontFamily: 'inherit',
              opacity: sending ? 0.7 : 1,
            }}>
              <Icons.Sent style={{ width: 13, height: 13 }} />
              {sent ? 'Sent!' : sending ? 'Sending…' : scheduleAt ? `Send ${scheduleAt.label}` : 'Send'}
              <span style={{ fontFamily: T.font.mono, fontSize: 10, opacity: 0.85, padding: '0 4px', background: 'rgba(0,0,0,0.18)', borderRadius: 3 }}>⌘↵</span>
            </button>
            <button onClick={() => setShowSchedule(!showSchedule)} title="Schedule send"
              style={{
                padding: '7px 8px', border: 'none',
                background: 'color-mix(in srgb, black 10%, ' + T.accent.coral + ')',
                borderLeft: '1px solid rgba(0,0,0,0.18)',
                color: '#fff', cursor: 'pointer',
                display: 'flex', alignItems: 'center',
              }}><Icons.Chevron size={10} style={{ transform: 'rotate(-90deg)' }} /></button>
          </div>

          <ComposeToolBtn T={T} icon={Icons.Attach} hint="Attach file" kbd="⇧⌘A" />
          <ComposeToolBtn T={T} icon={Icons.Sparkle} hint="AI assist" onClick={() => { setShowAI(!showAI); setShowTemplates(false); setShowSchedule(false); setShowFollow(false); }} active={showAI} />
          <ComposeToolBtn T={T} icon={Icons.Edit} hint="Insert template" onClick={() => { setShowTemplates(!showTemplates); setShowAI(false); setShowSchedule(false); setShowFollow(false); }} active={showTemplates} />
          <ComposeToolBtn T={T} icon={Icons.Snooze} hint="Follow up if no reply" onClick={() => { setShowFollow(!showFollow); setShowAI(false); setShowTemplates(false); setShowSchedule(false); }} active={showFollow || !!followUp} />
          <div style={{ width: 1, height: 18, background: T.borderSoft, margin: '0 4px' }} />
          <ComposeToolBtn T={T} icon={Icons.Eye} hint="Track opens" onClick={() => setTracking(!tracking)} active={tracking} />
          <ComposeToolBtn T={T} icon={Icons.Lock} hint="Encrypt (S/MIME)" onClick={() => setEncrypt(!encrypt)} active={encrypt} />

          <div style={{ flex: 1 }} />
          <button onClick={onClose} style={{
            border: 'none', background: 'transparent', color: T.fgMuted,
            fontSize: 12, cursor: 'pointer', padding: '6px 10px', borderRadius: 5,
            fontFamily: 'inherit',
          }}>Discard</button>

          {/* Popovers */}
          {showSchedule && <SchedulePopover T={T} onPick={(p) => { setScheduleAt(p); setShowSchedule(false); }} onClose={() => setShowSchedule(false)} />}
          {showFollow && <FollowPopover T={T} value={followUp} onPick={(f) => { setFollowUp(f); setShowFollow(false); }} onClose={() => setShowFollow(false)} />}
          {showTemplates && <TemplatesPopover T={T} onPick={(t) => { setBody(t.body); setShowTemplates(false); }} onClose={() => setShowTemplates(false)} />}
          {showAI && <AIPopover T={T} onRun={runAI} onClose={() => setShowAI(false)} />}
        </div>

        {/* Stamp send animation */}
        {sent && (
          <div style={{
            position: 'absolute', inset: 0,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            background: 'rgba(255,255,255,0.02)', pointerEvents: 'none',
            animation: 'phStampIn 0.6s cubic-bezier(.2,.9,.4,1.4) forwards',
          }}>
            <PostmarkStamp size={140} color={T.accent.coralDeep} text="DELIVERED" date={new Date().toDateString().toUpperCase().slice(4, 10)} />
            <style>{`
              @keyframes phStampIn { from { transform: scale(3) rotate(-20deg); opacity: 0 } to { transform: scale(1) rotate(-8deg); opacity: 1 } }
              @keyframes ph-pulse { 0%, 100% { opacity: 1 } 50% { opacity: 0.3 } }
            `}</style>
          </div>
        )}
        <style>{`@keyframes ph-pulse { 0%, 100% { opacity: 1 } 50% { opacity: 0.3 } }`}</style>
      </div>
    </div>
  );
}

function ComposeField({ T, label, children }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center',
      padding: '0 16px', minHeight: 34,
      borderBottom: `1px solid ${T.borderSoft}`,
    }}>
      <div style={{
        width: 56, fontSize: 11, color: T.fgFaint,
        fontFamily: T.font.mono, fontWeight: 500,
      }}>{label}</div>
      {children}
    </div>
  );
}

function ComposeBadge({ T, icon: Ico, color, text, onClear }) {
  return (
    <div style={{
      display: 'inline-flex', alignItems: 'center', gap: 4,
      padding: '3px 4px 3px 8px', borderRadius: 999,
      background: `color-mix(in srgb, ${color} 18%, transparent)`,
      color, fontSize: 10.5, fontWeight: 600, fontFamily: T.font.mono,
      border: `1px solid color-mix(in srgb, ${color} 35%, transparent)`,
    }}>
      <Ico size={10} />
      {text}
      {onClear && (
        <button onClick={onClear} style={{
          width: 14, height: 14, borderRadius: '50%', border: 'none',
          background: 'transparent', color, cursor: 'pointer',
          display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 0,
        }}><Icons.X size={9} /></button>
      )}
    </div>
  );
}

function ComposeToolBtn({ T, icon: Ico, hint, onClick, active, kbd }) {
  const [hover, setHover] = React.useState(false);
  return (
    <button onClick={onClick}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      title={hint + (kbd ? ` (${kbd})` : '')}
      style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        width: 28, height: 28, border: 'none', borderRadius: 6, cursor: 'pointer',
        background: active ? T.accent.coralSoft : (hover ? T.hoverBg : 'transparent'),
        color: active ? T.accent.coralDeep : T.fgMuted,
      }}>
      <Ico size={14} />
    </button>
  );
}

// ──────────────────────────────── Popovers

function Popover({ T, right = 8, bottom = 48, width = 260, onClose, children }) {
  React.useEffect(() => {
    const h = (e) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [onClose]);
  return (
    <div style={{
      position: 'absolute', bottom, right, width, zIndex: 20,
      background: T.bgReader, border: `1px solid ${T.border}`,
      borderRadius: 10, padding: 6,
      boxShadow: '0 14px 40px rgba(0,0,0,0.35), 0 0 0 1px rgba(255,255,255,0.03) inset',
      animation: 'ph-pop-in 0.14s ease-out',
    }}>
      {children}
      <style>{`
        @keyframes ph-pop-in {
          from { opacity: 0; transform: translateY(6px) scale(0.98) }
          to   { opacity: 1; transform: translateY(0) scale(1) }
        }
      `}</style>
    </div>
  );
}

function PopoverItem({ T, icon: Ico, label, sub, onClick, active, kbd }) {
  const [hover, setHover] = React.useState(false);
  return (
    <button onClick={onClick}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{
        width: '100%', display: 'flex', alignItems: 'center', gap: 10,
        padding: '7px 10px', border: 'none', borderRadius: 6,
        background: active ? T.accent.coralSoft : (hover ? T.hoverBg : 'transparent'),
        color: active ? T.accent.coralDeep : T.fg, cursor: 'pointer',
        fontFamily: 'inherit', fontSize: 12.5, textAlign: 'left',
      }}>
      {Ico && <Ico size={13} style={{ color: active ? T.accent.coralDeep : T.fgMuted }} />}
      <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>{label}</span>
      {sub && <span style={{ color: T.fgFaint, fontSize: 11, fontFamily: T.font.mono }}>{sub}</span>}
      {kbd && <Kbd T={T}>{kbd}</Kbd>}
    </button>
  );
}

function PopoverHeader({ T, children }) {
  return (
    <div style={{
      padding: '4px 10px 6px', fontSize: 10, fontFamily: T.font.mono,
      fontWeight: 600, color: T.fgFaint, textTransform: 'uppercase', letterSpacing: 0.6,
    }}>{children}</div>
  );
}

function SchedulePopover({ T, onPick, onClose }) {
  return (
    <Popover T={T} width={230} onClose={onClose}>
      <PopoverHeader T={T}>Send later</PopoverHeader>
      {PH_SCHEDULE_PRESETS.map((p) => (
        <PopoverItem key={p.id} T={T} icon={Icons.Calendar} label={p.label} sub={p.sub}
          onClick={() => onPick(p)} />
      ))}
    </Popover>
  );
}

function FollowPopover({ T, value, onPick, onClose }) {
  return (
    <Popover T={T} width={220} right={120} onClose={onClose}>
      <PopoverHeader T={T}>Follow up if no reply</PopoverHeader>
      {PH_FOLLOWUP_PRESETS.map((p) => (
        <PopoverItem key={p.id} T={T} icon={Icons.Snooze} label={p.label}
          onClick={() => onPick(p.id)} active={value === p.id} />
      ))}
      {value && (
        <div style={{ padding: 6 }}>
          <button onClick={() => onPick(null)} style={{
            width: '100%', padding: '6px 10px', borderRadius: 6,
            border: `1px solid ${T.borderSoft}`, background: 'transparent',
            color: T.fgMuted, fontSize: 11.5, cursor: 'pointer', fontFamily: 'inherit',
          }}>Cancel follow-up</button>
        </div>
      )}
    </Popover>
  );
}

function TemplatesPopover({ T, onPick, onClose }) {
  return (
    <Popover T={T} width={280} right={180} onClose={onClose}>
      <PopoverHeader T={T}>Templates</PopoverHeader>
      {PH_TEMPLATES.map((t) => (
        <PopoverItem key={t.id} T={T} icon={Icons.Edit} label={t.name}
          sub={t.body.split('\n')[0].slice(0, 24) + '…'}
          onClick={() => onPick(t)} />
      ))}
      <div style={{ height: 1, background: T.borderSoft, margin: '4px 6px' }} />
      <PopoverItem T={T} icon={Icons.Plus} label="Save current as template" onClick={onClose} />
    </Popover>
  );
}

function AIPopover({ T, onRun, onClose }) {
  return (
    <Popover T={T} width={220} right={220} onClose={onClose}>
      <PopoverHeader T={T}>Rewrite with AI</PopoverHeader>
      {PH_AI_ACTIONS.map((a) => {
        const Ico = Icons[a.icon] || Icons.Sparkle;
        return (
          <PopoverItem key={a.id} T={T} icon={Ico} label={a.label}
            onClick={() => onRun(a.id)} />
        );
      })}
      <div style={{ height: 1, background: T.borderSoft, margin: '4px 6px' }} />
      <div style={{ padding: '6px 10px', fontSize: 10, color: T.fgFaint, fontFamily: T.font.mono }}>
        On-device · MLX
      </div>
    </Popover>
  );
}

Object.assign(window, { Compose });
