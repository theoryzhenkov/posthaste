// Posthaste window chrome — v2, locked type/icon ramp

function TrafficLights() {
  const dot = (bg) => (
    <div style={{
      width: 12, height: 12, borderRadius: '50%', background: bg,
      boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.2)',
    }} />
  );
  return (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
      {dot('#ff5f57')}{dot('#febc2e')}{dot('#28c940')}
    </div>
  );
}

function Titlebar({ T, title, subtitle, density = 'standard', onTheme, theme }) {
  const h = density === 'compact' ? 36 : density === 'roomy' ? 48 : 42;
  return (
    <div style={{
      height: h, display: 'flex', alignItems: 'center',
      background: T.bgTitlebar, borderBottom: `1px solid ${T.border}`,
      padding: '0 12px', gap: 12, flexShrink: 0, position: 'relative',
    }}>
      <TrafficLights />
      <div style={{ flex: 1 }} />
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <button onClick={onTheme} title="Toggle theme" style={{
          border: 'none', background: 'transparent', width: 28, height: 28,
          borderRadius: 5, cursor: 'pointer', display: 'flex', alignItems: 'center',
          justifyContent: 'center', color: T.fgMuted,
        }}>
          {theme === 'dark'
            ? <svg width={T.icon.sm} height={T.icon.sm} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={T.stroke[T.icon.sm]}><circle cx="8" cy="8" r="3" /><path d="M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.5 1.5M11.5 11.5L13 13M3 13l1.5-1.5M11.5 4.5L13 3" strokeLinecap="round" /></svg>
            : <svg width={T.icon.sm} height={T.icon.sm} viewBox="0 0 16 16" fill="currentColor"><path d="M6 2a6 6 0 1 0 8 8 5 5 0 0 1-8-8z" /></svg>}
        </button>
      </div>
    </div>
  );
}

function ToolbarChip({ T, icon: Icon, label, active, onClick, hint }) {
  const [hover, setHover] = React.useState(false);
  const bg = active ? T.accent.coralSoft : (hover ? T.hoverBg : 'transparent');
  const fg = active ? T.accent.coralDeep : T.fgMuted;
  return (
    <button onClick={onClick}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      title={hint}
      style={{
        display: 'flex', alignItems: 'center', gap: 5,
        height: 28, padding: label ? '0 9px 0 8px' : '0 6px',
        border: 'none', background: bg, color: fg,
        borderRadius: 6, cursor: 'pointer',
        fontSize: T.type.ui, fontWeight: 500, fontFamily: 'inherit',
        transition: 'background 0.1s',
      }}>
      {Icon && <Icon size={T.icon.sm} />}
      {label && <span>{label}</span>}
      {hint && label && (
        <span style={{
          fontFamily: T.font.mono, fontSize: T.type.meta, opacity: 0.6,
          padding: '1px 4px', borderRadius: 3, background: 'rgba(128,128,128,0.12)',
          marginLeft: 2,
        }}>{hint}</span>
      )}
    </button>
  );
}

function ActionBar({ T, density = 'standard', onCompose, layout, onLayout, theme, onTheme, onSettings, onCmdK, onShortcuts }) {
  const h = density === 'compact' ? 38 : density === 'roomy' ? 46 : 42;
  return (
    <div style={{
      height: h, display: 'flex', alignItems: 'center',
      borderBottom: `1px solid ${T.borderSoft}`, background: T.bgTitlebar,
      padding: '0 12px', gap: 4, flexShrink: 0,
    }}>
      <TrafficLights />
      <div style={{ width: 8 }} />
      <ToolbarChip T={T} icon={Icons.Compose} label="Compose" onClick={onCompose} hint="⌘N" />
      <div style={{ width: 1, height: 18, background: T.borderSoft, margin: '0 6px' }} />
      <ToolbarChip T={T} icon={Icons.Reply} hint="⌘R" />
      <ToolbarChip T={T} icon={Icons.ReplyAll} hint="⇧⌘R" />
      <ToolbarChip T={T} icon={Icons.Forward} hint="⇧⌘F" />
      <div style={{ width: 1, height: 18, background: T.borderSoft, margin: '0 6px' }} />
      <ToolbarChip T={T} icon={Icons.Archive} hint="E" />
      <ToolbarChip T={T} icon={Icons.Trash} hint="⌫" />
      <ToolbarChip T={T} icon={Icons.Flag} hint="⇧⌘L" />
      <ToolbarChip T={T} icon={Icons.Snooze} hint="H" />
      <ToolbarChip T={T} icon={Icons.Tag} hint="L" />
      <div style={{ flex: 1 }} />
      <QuerySearch T={T} onFocus={onCmdK} />
      {onShortcuts && (
        <button onClick={onShortcuts} title="Keyboard shortcuts (?)" style={{
          border: 'none', background: 'transparent', width: 28, height: 28,
          borderRadius: 5, cursor: 'pointer', display: 'flex', alignItems: 'center',
          justifyContent: 'center', color: T.fgMuted, marginLeft: 4,
          fontFamily: T.font.mono, fontSize: 13, fontWeight: 700,
        }}>?</button>
      )}
      {onSettings && (
        <button onClick={onSettings} title="Settings (⌘,)" style={{
          border: 'none', background: 'transparent', width: 28, height: 28,
          borderRadius: 5, cursor: 'pointer', display: 'flex', alignItems: 'center',
          justifyContent: 'center', color: T.fgMuted,
        }}><Icons.Settings size={T.icon.sm} /></button>
      )}
      {onTheme && (
        <button onClick={onTheme} title="Toggle theme" style={{
          border: 'none', background: 'transparent', width: 28, height: 28,
          borderRadius: 5, cursor: 'pointer', display: 'flex', alignItems: 'center',
          justifyContent: 'center', color: T.fgMuted, marginLeft: 4,
        }}>
          {theme === 'dark'
            ? <svg width={T.icon.sm} height={T.icon.sm} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={T.stroke[T.icon.sm]}><circle cx="8" cy="8" r="3" /><path d="M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.5 1.5M11.5 11.5L13 13M3 13l1.5-1.5M11.5 4.5L13 3" strokeLinecap="round" /></svg>
            : <svg width={T.icon.sm} height={T.icon.sm} viewBox="0 0 16 16" fill="currentColor"><path d="M6 2a6 6 0 1 0 8 8 5 5 0 0 1-8-8z" /></svg>}
        </button>
      )}
    </div>
  );
}

function QuerySearch({ T, onFocus: onActivate }) {
  const [val, setVal] = React.useState('');
  const [focus, setFocus] = React.useState(false);
  return (
    <div onClick={() => onActivate && onActivate()} style={{
      display: 'flex', alignItems: 'center', gap: 6,
      height: 26, width: focus || val ? 340 : 220,
      transition: 'width 0.2s',
      background: T.bgElev,
      border: `1px solid ${focus ? T.focusRing : T.borderSoft}`,
      boxShadow: focus ? `0 0 0 2px color-mix(in oklab, ${T.focusRing} 30%, transparent)` : 'none',
      borderRadius: 6, padding: '0 8px', cursor: onActivate ? 'text' : 'default',
    }}>
      <Icons.Search size={T.icon.xs} style={{ color: T.fgFaint }} />
      {!val && !focus ? (
        <>
          <span style={{ fontSize: T.type.ui, color: T.fgFaint }}>Search mail</span>
          <div style={{ flex: 1 }} />
          <span style={{
            fontFamily: T.font.mono, fontSize: T.type.meta, color: T.fgFaint,
            padding: '1px 5px', borderRadius: 3, background: T.bg,
            border: `1px solid ${T.borderSoft}`,
          }}>⌘K</span>
        </>
      ) : (
        <input autoFocus={focus} value={val} onChange={(e) => setVal(e.target.value)}
          onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}
          placeholder="from:maya tag:work date:>2026-04-01"
          style={{
            flex: 1, border: 'none', background: 'transparent', outline: 'none',
            fontFamily: T.font.mono, fontSize: T.type.ui, color: T.fg,
          }} />
      )}
    </div>
  );
}

function StatusBar({ T, count, unread, account }) {
  return (
    <div style={{
      height: 24, display: 'flex', alignItems: 'center',
      padding: '0 12px', gap: 16,
      borderTop: `1px solid ${T.borderSoft}`, background: T.bgSidebar,
      fontSize: T.type.meta, color: T.fgMuted, fontFamily: T.font.mono,
    }}>
      <span>{count} messages · {unread} unread</span>
      <div style={{ flex: 1 }} />
      <span style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: T.accent.sage }} />
        JMAP · Fastmail · sync ok
      </span>
      <span>{account}</span>
    </div>
  );
}

Object.assign(window, { TrafficLights, Titlebar, ToolbarChip, ActionBar, QuerySearch, StatusBar });
