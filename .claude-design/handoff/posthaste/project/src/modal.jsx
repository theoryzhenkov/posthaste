// Shared glass modal primitive — Arc-style blurred backdrop + frosted sheet.
// Every editor/palette/settings sheet composes from these.

function Modal({ T, onClose, width = 720, height = 560, padding = 0, children, noBg = false, tone = 'auto' }) {
  // Esc to close
  React.useEffect(() => {
    if (!onClose) return;
    const h = (e) => { if (e.key === 'Escape') { e.preventDefault(); onClose(); } };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [onClose]);

  // Determine whether the base theme is dark — glass tones adapt
  const isDark = (T.mode || 'dark') === 'dark';
  const sheetBg = noBg ? 'transparent' : (isDark
    ? 'rgba(22,20,28,0.82)'
    : 'rgba(255,255,254,0.85)');
  const sheetBorder = isDark ? 'rgba(255,255,255,0.09)' : 'rgba(20,18,28,0.08)';
  const sheetShadow = isDark
    ? '0 32px 80px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.04) inset'
    : '0 32px 80px rgba(40,30,60,0.22), 0 0 0 1px rgba(255,255,255,0.6) inset';

  return (
    <div
      onMouseDown={(e) => { if (e.target === e.currentTarget && onClose) onClose(); }}
      style={{
        position: 'absolute', inset: 0, zIndex: 2000,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        background: isDark ? 'rgba(6,4,12,0.55)' : 'rgba(40,30,60,0.35)',
        backdropFilter: 'blur(18px) saturate(140%)',
        WebkitBackdropFilter: 'blur(18px) saturate(140%)',
        animation: 'ph-modal-in 0.18s ease-out',
      }}>
      <div style={{
        width: Math.min(width, 10000), maxWidth: 'calc(100% - 48px)',
        height: Math.min(height, 10000), maxHeight: 'calc(100% - 48px)',
        background: sheetBg,
        backdropFilter: 'blur(24px) saturate(180%)',
        WebkitBackdropFilter: 'blur(24px) saturate(180%)',
        border: `1px solid ${sheetBorder}`,
        borderRadius: 16,
        boxShadow: sheetShadow,
        color: T.fg,
        fontFamily: T.font.sans,
        display: 'flex', flexDirection: 'column',
        overflow: 'hidden', padding,
        animation: 'ph-sheet-in 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.0)',
      }}>
        {children}
      </div>
      <style>{`
        @keyframes ph-modal-in { from { opacity: 0 } to { opacity: 1 } }
        @keyframes ph-sheet-in {
          from { opacity: 0; transform: translateY(12px) scale(0.985) }
          to   { opacity: 1; transform: translateY(0) scale(1) }
        }
      `}</style>
    </div>
  );
}

// Header bar inside a modal: title + subtitle on the left, close button on the right.
function ModalHeader({ T, title, subtitle, icon: Icon, onClose, actions }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 12,
      padding: '18px 22px 16px',
      borderBottom: `1px solid ${T.borderSoft}`,
      flexShrink: 0,
    }}>
      {Icon && (
        <div style={{
          width: 36, height: 36, borderRadius: 10,
          background: T.accent.coralSoft, color: T.accent.coralDeep,
          display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0,
        }}><Icon size={18} /></div>
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: T.type.head, fontWeight: 700, letterSpacing: -0.3, color: T.fg }}>
          {title}
        </div>
        {subtitle && (
          <div style={{ fontSize: T.type.ui, color: T.fgMuted, marginTop: 2 }}>
            {subtitle}
          </div>
        )}
      </div>
      {actions}
      {onClose && (
        <button onClick={onClose}
          title="Close (Esc)"
          style={{
            width: 30, height: 30, borderRadius: 8,
            border: 'none', background: 'transparent', color: T.fgMuted,
            cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center',
            fontFamily: 'inherit',
          }}>
          <Icons.X size={16} />
        </button>
      )}
    </div>
  );
}

// Footer: right-aligned actions by default
function ModalFooter({ T, children, hint }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
      padding: '14px 22px', borderTop: `1px solid ${T.borderSoft}`,
      flexShrink: 0, background: 'color-mix(in srgb, currentColor 2%, transparent)',
    }}>
      {hint && <div style={{ fontSize: T.type.meta, color: T.fgFaint, fontFamily: T.font.mono }}>{hint}</div>}
      <div style={{ flex: 1 }} />
      {children}
    </div>
  );
}

// Generic button used in modals/toolbars. Variants: primary, secondary, ghost, danger
function ModalButton({ T, variant = 'secondary', onClick, children, icon: Icon, disabled, kbd }) {
  const base = {
    display: 'inline-flex', alignItems: 'center', gap: 6,
    height: 32, padding: '0 14px', borderRadius: 8,
    fontSize: T.type.ui, fontWeight: 600, fontFamily: 'inherit',
    cursor: disabled ? 'not-allowed' : 'pointer', border: '1px solid transparent',
    transition: 'transform 0.06s ease, background 0.1s ease',
    userSelect: 'none', whiteSpace: 'nowrap', opacity: disabled ? 0.5 : 1,
  };
  const styles = {
    primary: {
      background: T.accent.coral, color: '#fff',
      borderColor: 'color-mix(in srgb, black 12%, transparent)',
      boxShadow: '0 1px 0 rgba(255,255,255,0.2) inset, 0 2px 6px rgba(0,0,0,0.12)',
    },
    secondary: {
      background: T.bgElev, color: T.fg, borderColor: T.border,
    },
    ghost: {
      background: 'transparent', color: T.fg,
    },
    danger: {
      background: 'transparent', color: T.accent.rose, borderColor: T.accent.rose,
    },
  };
  return (
    <button onClick={onClick} disabled={disabled} style={{ ...base, ...styles[variant] }}>
      {Icon && <Icon size={14} />}
      <span>{children}</span>
      {kbd && (
        <span style={{
          fontFamily: T.font.mono, fontSize: T.type.meta, opacity: 0.75,
          padding: '1px 5px', borderRadius: 4, marginLeft: 4,
          background: variant === 'primary' ? 'rgba(255,255,255,0.18)' : T.bg,
          border: variant === 'primary' ? 'none' : `1px solid ${T.borderSoft}`,
        }}>{kbd}</span>
      )}
    </button>
  );
}

// Keyboard shortcut pill
function Kbd({ T, children }) {
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      minWidth: 18, height: 18, padding: '0 5px',
      fontFamily: T.font.mono, fontSize: T.type.meta, fontWeight: 600,
      color: T.fgMuted, background: T.bg,
      border: `1px solid ${T.borderSoft}`, borderRadius: 4,
    }}>{children}</span>
  );
}

// Section label inside a panel — small mono caps
function SectionLabel({ T, children, style = {} }) {
  return (
    <div style={{
      fontFamily: T.font.mono, fontSize: T.type.meta, fontWeight: 600,
      color: T.fgFaint, letterSpacing: 0.7, textTransform: 'uppercase',
      ...style,
    }}>{children}</div>
  );
}

// Form row: label on the left, control on the right
function FormRow({ T, label, hint, children, stack = false }) {
  return (
    <div style={{
      display: 'flex', flexDirection: stack ? 'column' : 'row', gap: stack ? 6 : 16,
      alignItems: stack ? 'stretch' : 'flex-start',
      padding: '12px 0', borderBottom: `1px solid ${T.borderSoft}`,
    }}>
      <div style={{ width: stack ? 'auto' : 180, flexShrink: 0, paddingTop: stack ? 0 : 7 }}>
        <div style={{ fontSize: T.type.body, fontWeight: 500, color: T.fg }}>{label}</div>
        {hint && (
          <div style={{ fontSize: T.type.meta, color: T.fgMuted, marginTop: 3, lineHeight: 1.4 }}>
            {hint}
          </div>
        )}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>{children}</div>
    </div>
  );
}

// Generic text input
function PhInput({ T, value, onChange, placeholder, mono = false, style = {}, ...rest }) {
  return (
    <input
      value={value ?? ''}
      onChange={(e) => onChange && onChange(e.target.value)}
      placeholder={placeholder}
      style={{
        width: '100%', height: 32, padding: '0 10px',
        border: `1px solid ${T.border}`, borderRadius: 8,
        background: T.bg, color: T.fg,
        fontFamily: mono ? T.font.mono : T.font.sans,
        fontSize: T.type.body, outline: 'none',
        ...style,
      }}
      onFocus={(e) => { e.target.style.borderColor = T.focusRing; e.target.style.boxShadow = `0 0 0 3px color-mix(in srgb, ${T.focusRing} 20%, transparent)`; }}
      onBlur={(e) => { e.target.style.borderColor = T.border; e.target.style.boxShadow = 'none'; }}
      {...rest}
    />
  );
}

// Simple toggle switch
function PhSwitch({ T, checked, onChange, label }) {
  return (
    <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, cursor: 'pointer', userSelect: 'none' }}>
      <span style={{
        width: 34, height: 20, borderRadius: 999,
        background: checked ? T.accent.coral : T.border,
        position: 'relative', transition: 'background 0.15s',
        flexShrink: 0,
      }}>
        <span style={{
          position: 'absolute', top: 2, left: checked ? 16 : 2,
          width: 16, height: 16, borderRadius: '50%', background: '#fff',
          transition: 'left 0.15s', boxShadow: '0 1px 3px rgba(0,0,0,0.2)',
        }} />
      </span>
      {label && <span style={{ fontSize: T.type.body, color: T.fg }}>{label}</span>}
    </label>
  );
}

// Select (native styled)
function PhSelect({ T, value, onChange, children, mono = false, style = {} }) {
  return (
    <select value={value} onChange={(e) => onChange && onChange(e.target.value)}
      style={{
        height: 32, padding: '0 28px 0 10px',
        border: `1px solid ${T.border}`, borderRadius: 8,
        background: `${T.bg} url("data:image/svg+xml,%3Csvg width='12' height='12' viewBox='0 0 12 12' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M3 5l3 3 3-3' fill='none' stroke='%23888' stroke-width='1.4' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E") no-repeat right 8px center`,
        color: T.fg,
        fontFamily: mono ? T.font.mono : T.font.sans,
        fontSize: T.type.body, outline: 'none', appearance: 'none',
        cursor: 'pointer',
        ...style,
      }}>
      {children}
    </select>
  );
}

Object.assign(window, { Modal, ModalHeader, ModalFooter, ModalButton, Kbd, SectionLabel, FormRow, PhInput, PhSwitch, PhSelect });
