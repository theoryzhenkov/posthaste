// Posthaste sidebar — v2, locked ramp + decoupled signals

function SidebarHeader({ T, children, style = {} }) {
  return (
    <div style={{
      padding: '14px 14px 6px', fontFamily: T.font.mono,
      fontSize: T.type.meta, fontWeight: 600, color: T.fgFaint,
      letterSpacing: 0.6, textTransform: 'uppercase',
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      ...style,
    }}>{children}</div>
  );
}

function SidebarRow({ T, icon: Icon, label, count, depth = 0, selected, onClick, accent, density = 'standard', onEdit }) {
  const [hover, setHover] = React.useState(false);
  const [focus, setFocus] = React.useState(false);
  const h = density === 'compact' ? 24 : density === 'roomy' ? 32 : 28;
  const bg = selected ? T.selBg : (hover ? T.hoverBg : 'transparent');
  const fg = selected ? T.selFg : T.fg;
  return (
    <div
      role="treeitem" tabIndex={0}
      onClick={onClick}
      onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex', alignItems: 'center',
        height: h, padding: `0 8px 0 ${8 + depth * 14}px`, margin: '0 6px',
        gap: 8, borderRadius: 5,
        background: bg, color: fg,
        cursor: 'pointer', userSelect: 'none', outline: 'none',
        fontSize: T.type.body, fontWeight: selected ? 600 : 500,
        boxShadow: focus ? `0 0 0 2px ${T.focusRing}` : 'none',
      }}>
      <div style={{ width: 14 }} />
      {Icon && <Icon size={T.icon.sm} style={{ color: selected ? T.selFg : (accent || T.fgMuted) }} />}
      <span style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{label}</span>
      {hover && onEdit ? (
        <button onClick={(e) => { e.stopPropagation(); onEdit(); }}
          title="Edit rules"
          style={{
            width: 18, height: 18, border: 'none', background: 'transparent',
            color: selected ? T.selFg : T.fgFaint, cursor: 'pointer',
            display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 0, borderRadius: 3,
          }}><Icons.Sliders size={11} /></button>
      ) : count > 0 ? (
        <span style={{
          fontFamily: T.font.mono, fontSize: T.type.meta,
          color: selected ? T.selFg : T.fgFaint, fontWeight: 600,
        }}>{count}</span>
      ) : null}
    </div>
  );
}

function AccountHeader({ T, account, expanded, onExpand, density }) {
  const h = density === 'compact' ? 26 : 30;
  return (
    <div onClick={onExpand} style={{
      display: 'flex', alignItems: 'center',
      height: h, padding: '0 10px', margin: '6px 6px 2px',
      gap: 8, borderRadius: 5, cursor: 'pointer',
    }}>
      <button style={{
        width: 14, height: 14, border: 'none', background: 'transparent',
        padding: 0, color: T.fgMuted, opacity: 0.7,
        transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
        transition: 'transform 0.1s', display: 'flex', alignItems: 'center',
      }}><Icons.Chevron size={T.icon.xs} /></button>
      <div style={{
        width: 18, height: 18, borderRadius: 4, background: account.color,
        color: '#fff', fontSize: T.type.meta, fontWeight: 700, fontFamily: T.font.mono,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}>{account.stamp}</div>
      <span style={{ fontSize: T.type.ui, fontWeight: 700, color: T.fg, flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
        {account.label}
      </span>
      {account.mailboxes.reduce((s, m) => s + m.unread, 0) > 0 && (
        <span style={{
          background: T.signal.unread, color: '#fff',
          fontFamily: T.font.mono, fontSize: T.type.meta, fontWeight: 700,
          padding: '1px 6px', borderRadius: 6, minWidth: 18, textAlign: 'center',
        }}>{account.mailboxes.reduce((s, m) => s + m.unread, 0)}</span>
      )}
    </div>
  );
}

function Sidebar({ T, selected, onSelect, density, tags, onAddSmart, onEditSmart }) {
  const [expanded, setExpanded] = React.useState({ gmail: true, work: true, uni: true });
  const toggle = (id) => setExpanded((e) => ({ ...e, [id]: !e[id] }));
  return (
    <div className="ph-scroll" role="tree" style={{
      background: T.bgSidebar, borderRight: `1px solid ${T.border}`,
      width: '100%', height: '100%', overflow: 'auto',
      display: 'flex', flexDirection: 'column', paddingBottom: 12,
    }}>
      <div style={{ padding: '10px 0 2px' }}>
        <SidebarRow T={T} icon={Icons.All} label="All Inboxes" count={13}
          selected={selected === 'all'} onClick={() => onSelect('all')}
          accent={T.accent.coral} density={density} />
        <SidebarRow T={T} icon={Icons.Flag} label="Flagged" count={3}
          selected={selected === 'flagged'} onClick={() => onSelect('flagged')}
          accent={T.signal.flag} density={density} />
      </div>
      <SidebarHeader T={T}>
        <span>Smart</span>
        {onAddSmart && (
          <button onClick={onAddSmart} title="New smart mailbox"
            style={{
              width: 16, height: 16, border: 'none', background: 'transparent',
              color: T.fgFaint, cursor: 'pointer', padding: 0, borderRadius: 3,
              display: 'flex', alignItems: 'center', justifyContent: 'center',
            }}
            onMouseEnter={(e) => { e.currentTarget.style.color = T.accent.coral; e.currentTarget.style.background = T.hoverBg; }}
            onMouseLeave={(e) => { e.currentTarget.style.color = T.fgFaint; e.currentTarget.style.background = 'transparent'; }}
          ><Icons.Plus size={12} /></button>
        )}
      </SidebarHeader>
      {PH_SMART_MAILBOXES.map((sm) => (
        <SidebarRow key={sm.id} T={T} icon={Icons[sm.icon]} label={sm.name} count={sm.unread}
          accent={T.accent[sm.accent]} selected={selected === sm.id}
          onClick={() => onSelect(sm.id)}
          onEdit={onEditSmart ? () => onEditSmart(sm) : null}
          density={density} />
      ))}
      <SidebarHeader T={T}><span>Tags</span></SidebarHeader>
      {tags.map((t) => (
        <SidebarRow key={t.id} T={T} icon={Icons.Tag} label={t.name} accent={t.color}
          selected={selected === `tag:${t.id}`} onClick={() => onSelect(`tag:${t.id}`)} density={density} />
      ))}
      <SidebarHeader T={T} style={{ marginTop: 8 }}>Accounts</SidebarHeader>
      {PH_ACCOUNTS.map((acc) => (
        <React.Fragment key={acc.id}>
          <AccountHeader T={T} account={acc} expanded={expanded[acc.id]} onExpand={() => toggle(acc.id)} density={density} />
          {expanded[acc.id] && acc.mailboxes.map((mb) => (
            <SidebarRow key={mb.id} T={T} icon={Icons[mb.icon]} label={mb.name} count={mb.unread}
              depth={1} selected={selected === mb.id}
              onClick={() => onSelect(mb.id)} density={density} />
          ))}
        </React.Fragment>
      ))}
    </div>
  );
}

Object.assign(window, { Sidebar, SidebarRow, SidebarHeader, AccountHeader });
