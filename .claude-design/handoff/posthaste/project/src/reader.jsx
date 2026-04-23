// Posthaste reader pane — v2, locked ramp + icon grid

function Reader({ T, msg, tags }) {
  if (!msg) {
    return (
      <div style={{
        width: '100%', height: '100%', background: T.bgReader,
        display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
        gap: 14, color: T.fgFaint,
      }}>
        <PostmarkStamp size={72} color={T.fgFaint} text="NO MSG" date="SELECTED" />
        <div style={{ fontSize: T.type.body, fontWeight: 500 }}>Select a message to read</div>
        <div style={{ fontSize: T.type.meta, fontFamily: T.font.mono }}>
          <kbd style={{ padding: '1px 5px', borderRadius: 3, border: `1px solid ${T.borderSoft}`, background: T.bgElev }}>J</kbd>
          {' / '}
          <kbd style={{ padding: '1px 5px', borderRadius: 3, border: `1px solid ${T.borderSoft}`, background: T.bgElev }}>K</kbd>
          {' to navigate'}
        </div>
      </div>
    );
  }

  return (
    <div className="ph-scroll" role="article" style={{
      width: '100%', height: '100%', background: T.bgReader,
      overflow: 'auto', color: T.fg,
    }}>
      {/* Header */}
      <div style={{
        padding: '16px 20px 12px',
        borderBottom: `1px solid ${T.borderSoft}`,
        position: 'relative',
      }}>
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12 }}>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: T.type.head, fontWeight: 600, color: T.fg, letterSpacing: -0.2, marginBottom: 8, lineHeight: 1.25 }}>
              {msg.subject}
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
              <div style={{
                width: 28, height: 28, borderRadius: '50%',
                background: `color-mix(in oklab, ${T.accent.coral} 40%, ${T.bg})`,
                color: '#fff', fontWeight: 700, fontSize: T.type.ui,
                display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0,
              }}>{msg.from.name.split(' ').map(x => x[0]).slice(0, 2).join('')}</div>
              <div style={{ fontSize: T.type.ui, minWidth: 0 }}>
                <div style={{ display: 'flex', gap: 6, alignItems: 'baseline' }}>
                  <span style={{ fontWeight: 600, color: T.fg, fontSize: T.type.body }}>{msg.from.name}</span>
                  <span style={{ color: T.fgMuted, fontFamily: T.font.mono, fontSize: T.type.meta }}>&lt;{msg.from.email}&gt;</span>
                </div>
                <div style={{ color: T.fgMuted, fontSize: T.type.meta, marginTop: 2, fontFamily: T.font.mono }}>
                  to {msg.to || 'theor@gmail.com'} · {msg.date}
                </div>
              </div>
            </div>
          </div>
          <div style={{ display: 'flex', gap: 4, flexShrink: 0 }}>
            <IconBtn T={T} icon={Icons.Reply} hint="Reply" />
            <IconBtn T={T} icon={Icons.Forward} hint="Forward" />
            <IconBtn T={T} icon={Icons.Archive} hint="Archive" />
            <IconBtn T={T} icon={Icons.More} hint="More" />
          </div>
        </div>

        {msg.tags && msg.tags.length > 0 && (
          <div style={{ display: 'flex', gap: 6, marginTop: 10 }}>
            {msg.tags.map((t) => {
              const tag = tags.find((x) => x.id === t);
              if (!tag) return null;
              return (
                <span key={t} style={{
                  fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 600,
                  color: tag.color, background: `color-mix(in oklab, ${tag.color} 14%, transparent)`,
                  padding: '2px 7px', borderRadius: 4,
                  display: 'flex', alignItems: 'center', gap: 4,
                }}>
                  <span style={{ width: 5, height: 5, borderRadius: '50%', background: tag.color }} />
                  {tag.name}
                </span>
              );
            })}
          </div>
        )}
      </div>

      {msg.attachments && msg.attachments.length > 0 && (
        <div style={{ padding: '10px 20px', borderBottom: `1px solid ${T.borderSoft}`, background: T.bgElev }}>
          {msg.attachments.map((a, i) => (
            <div key={i} style={{
              display: 'flex', alignItems: 'center', gap: 10, padding: '8px 10px',
              background: T.bg, border: `1px solid ${T.border}`, borderRadius: 6,
              marginBottom: i < msg.attachments.length - 1 ? 6 : 0,
            }}>
              <div style={{
                width: 32, height: 32, borderRadius: 4,
                background: a.type === 'pdf' ? T.accent.rose : a.type === 'image' ? T.accent.violet : T.accent.blue,
                color: '#fff', fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 700,
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                textTransform: 'uppercase',
              }}>{a.type === 'ai' ? 'ai' : a.type === 'image' ? 'img' : a.type}</div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: T.type.ui, fontWeight: 500, color: T.fg, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{a.name}</div>
                <div style={{ fontSize: T.type.meta, color: T.fgFaint, fontFamily: T.font.mono }}>{a.size}</div>
              </div>
              <IconBtn T={T} icon={Icons.Download} hint="Save" />
              <IconBtn T={T} icon={Icons.More} />
            </div>
          ))}
        </div>
      )}

      <div style={{
        padding: '18px 22px 28px',
        fontSize: T.type.body, lineHeight: 1.6, color: T.fg,
        fontFamily: T.font.sans,
        whiteSpace: 'pre-wrap', maxWidth: 720,
      }}>
        {msg.body}
        {msg.id === 'm1' && (
          <div style={{ marginTop: 14 }}>
            <a style={{ color: T.accent.coralDeep, textDecoration: 'none', borderBottom: `1px dotted ${T.accent.coral}` }}>
              https://account.venmo.com/u/LeoCancelmoPhD
            </a>
          </div>
        )}
      </div>
    </div>
  );
}

function IconBtn({ T, icon: Icon, hint, onClick }) {
  const [hover, setHover] = React.useState(false);
  const [focus, setFocus] = React.useState(false);
  return (
    <button onClick={onClick} title={hint}
      onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{
        width: 28, height: 28, border: 'none',
        background: hover ? T.hoverBg : 'transparent',
        color: T.fgMuted, borderRadius: 5, cursor: 'pointer',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        outline: 'none',
        boxShadow: focus ? `0 0 0 2px ${T.focusRing}` : 'none',
      }}>
      <Icon size={T.icon.sm} />
    </button>
  );
}

Object.assign(window, { Reader, IconBtn });
