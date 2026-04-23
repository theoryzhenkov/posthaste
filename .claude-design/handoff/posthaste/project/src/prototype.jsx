// Posthaste main prototype assembly
// Panes are resizable via <PaneSplitter> dragged boundaries.

function PaneSplitter({ T, onDrag, active }) {
  const [hover, setHover] = React.useState(false);
  const hot = hover || active;
  return (
    <div
      onPointerDown={onDrag}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative', width: 1, flexShrink: 0,
        background: T.border,
        cursor: 'col-resize', zIndex: 2,
      }}>
      {/* Wider invisible hit area */}
      <div style={{
        position: 'absolute', top: 0, bottom: 0, left: -4, right: -4,
        cursor: 'col-resize',
      }} />
      {hot && (
        <div style={{
          position: 'absolute', top: 0, bottom: 0, left: -1, width: 3,
          background: T.accent.coral, pointerEvents: 'none',
          transition: 'opacity 0.12s',
        }} />
      )}
    </div>
  );
}

function Prototype({ theme = 'dark', preset = 'neutral', density = 'standard', layout = 3, showAdvanced = true, onTheme, embedded = false }) {
  const T = resolveTheme(preset, theme);
  const backdrop = themeBackdrop(preset, T.mode);
  const [selectedMailbox, setSelectedMailbox] = React.useState('gmail/inbox');
  const [selectedMessage, setSelectedMessage] = React.useState('m1');
  const [composing, setComposing] = React.useState(false);
  const [currentLayout, setCurrentLayout] = React.useState(layout);

  // Overlays — settings, shortcuts, command palette, mailbox editor, onboarding
  const [overlay, setOverlay] = React.useState(null); // null | 'settings' | 'shortcuts' | 'cmdk' | 'onboarding'
  const [editor, setEditor] = React.useState(null);   // null | { mode, initial }

  // Global keyboard shortcuts
  React.useEffect(() => {
    const h = (e) => {
      const meta = e.metaKey || e.ctrlKey;
      const target = e.target;
      const typing = target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);
      // Cmd+K → palette
      if (meta && (e.key === 'k' || e.key === 'K')) { e.preventDefault(); setOverlay('cmdk'); return; }
      // Cmd+, → settings
      if (meta && e.key === ',') { e.preventDefault(); setOverlay('settings'); return; }
      // Cmd+N → compose
      if (meta && (e.key === 'n' || e.key === 'N')) { e.preventDefault(); setComposing(true); return; }
      if (typing) return;
      // ? → shortcuts
      if (e.key === '?' || (e.shiftKey && e.key === '/')) { e.preventDefault(); setOverlay('shortcuts'); return; }
    };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, []);

  // Command palette dispatch
  const onCommand = (item) => {
    if (item.kind !== 'command') return;
    switch (item.id) {
      case 'compose':   setComposing(true); break;
      case 'settings':  setOverlay('settings'); break;
      case 'shortcuts': setOverlay('shortcuts'); break;
      case 'newSmart':  setEditor({ mode: 'smart' }); break;
      case 'newRule':   setEditor({ mode: 'mailbox' }); break;
      case 'account':   setOverlay('onboarding'); break;
      default: break;
    }
  };

  // Pane widths — persisted. Reader always flexes to fill remaining space.
  const defaultWidths = () => ({
    sidebar: density === 'compact' ? 180 : 210,
    list:    currentLayout >= 3 ? (density === 'compact' ? 360 : 420) : 520,
  });
  const [widths, setWidths] = React.useState(() => {
    try {
      const saved = JSON.parse(localStorage.getItem('ph-panes') || 'null');
      if (saved) return { ...defaultWidths(), ...saved };
    } catch (e) {}
    return defaultWidths();
  });
  const [activeSplitter, setActiveSplitter] = React.useState(null);

  React.useEffect(() => setCurrentLayout(layout), [layout]);
  React.useEffect(() => {
    localStorage.setItem('ph-panes', JSON.stringify(widths));
  }, [widths]);

  const startDrag = (key, minW, maxW) => (e) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = widths[key];
    setActiveSplitter(key);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    const move = (ev) => {
      const dx = ev.clientX - startX;
      setWidths((w) => ({
        ...w,
        [key]: Math.max(minW, Math.min(maxW, startW + dx)),
      }));
    };
    const up = () => {
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', up);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      setActiveSplitter(null);
    };
    document.addEventListener('pointermove', move);
    document.addEventListener('pointerup', up);
  };

  const msg = PH_MESSAGES.find((m) => m.id === selectedMessage);
  const mailboxName = (() => {
    if (selectedMailbox === 'all') return 'All Inboxes';
    if (selectedMailbox === 'flagged') return 'Flagged';
    const smart = PH_SMART_MAILBOXES.find((s) => s.id === selectedMailbox);
    if (smart) return smart.name;
    for (const a of PH_ACCOUNTS) {
      const mb = a.mailboxes.find((m) => m.id === selectedMailbox);
      if (mb) return `${a.label} — ${mb.name}`;
    }
    return 'Inbox';
  })();

  const messages = PH_MESSAGES.filter((m) => {
    if (selectedMailbox === 'all') return m.mailbox.endsWith('/inbox');
    if (selectedMailbox === 'flagged') return m.flagged;
    if (selectedMailbox.startsWith('smart/')) return true;
    if (selectedMailbox.startsWith('tag:')) return m.tags.includes(selectedMailbox.slice(4));
    return m.mailbox === selectedMailbox;
  });

  return (
    <div style={{
      width: '100%', height: '100%',
      background: T.bg, color: T.fg,
      ...backdrop,
      display: 'flex', flexDirection: 'column',
      borderRadius: embedded ? 0 : 10, overflow: 'hidden',
      fontFamily: T.font.sans, position: 'relative',
      boxShadow: embedded ? 'none' : '0 0 0 1px rgba(0,0,0,0.2), 0 24px 72px rgba(0,0,0,0.35)',
    }}>
      <ActionBar T={T} density={density} onCompose={() => setComposing(true)}
        layout={currentLayout} onLayout={setCurrentLayout}
        theme={theme} onTheme={onTheme}
        onSettings={() => setOverlay('settings')}
        onShortcuts={() => setOverlay('shortcuts')}
        onCmdK={() => setOverlay('cmdk')} />
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        {currentLayout >= 2 && (
          <>
            <div style={{ width: widths.sidebar, flexShrink: 0, height: '100%' }}>
              <Sidebar T={T} selected={selectedMailbox} onSelect={setSelectedMailbox}
                density={density} tags={PH_TAGS}
                onAddSmart={() => setEditor({ mode: 'smart' })}
                onEditSmart={(sm) => setEditor({ mode: 'smart', initial: {
                  name: sm.name, icon: sm.icon, accent: sm.accent,
                  combinator: 'all',
                  conditions: [
                    { id: crypto.randomUUID(), field: 'from', op: 'contains', value: '' },
                    { id: crypto.randomUUID(), field: 'subject', op: 'contains', value: '' },
                  ],
                  actions: [{ id: crypto.randomUUID(), type: 'tag', value: sm.id.replace('smart/', '') }],
                }})}
              />
            </div>
            <PaneSplitter T={T} active={activeSplitter === 'sidebar'}
              onDrag={startDrag('sidebar', 160, 360)} />
          </>
        )}
        {currentLayout >= 3 && (
          <>
            <div style={{ width: widths.list, flexShrink: 0, height: '100%' }}>
              <MessageList T={T} messages={messages} selected={selectedMessage}
                onSelect={setSelectedMessage} density={density} tags={PH_TAGS} />
            </div>
            <PaneSplitter T={T} active={activeSplitter === 'list'}
              onDrag={startDrag('list', 280, 720)} />
          </>
        )}
        {currentLayout === 2 && (
          <div style={{ flex: 1, minWidth: 280, height: '100%' }}>
            <MessageList T={T} messages={messages} selected={selectedMessage}
              onSelect={setSelectedMessage} density={density} tags={PH_TAGS} />
          </div>
        )}
        {currentLayout >= 3 && (
          <div style={{ flex: 1, minWidth: 280, height: '100%' }}>
            <Reader T={T} msg={msg} tags={PH_TAGS} density={density} />
          </div>
        )}
      </div>
      {composing && <Compose T={T} onClose={() => setComposing(false)} />}
      {overlay === 'settings' && (
        <SettingsSheet T={T} onClose={() => setOverlay(null)}
          onOpenEditor={(mode, initial) => { setOverlay(null); setEditor({ mode, initial }); }} />
      )}
      {overlay === 'shortcuts' && (
        <ShortcutsOverlay T={T} onClose={() => setOverlay(null)} />
      )}
      {overlay === 'cmdk' && (
        <CommandPalette T={T} onClose={() => setOverlay(null)} onCommand={onCommand} />
      )}
      {overlay === 'onboarding' && (
        <Onboarding T={T} onClose={() => setOverlay(null)} />
      )}
      {editor && (
        <MailboxEditor T={T} mode={editor.mode} initial={editor.initial}
          onClose={() => setEditor(null)} />
      )}
    </div>
  );
}

Object.assign(window, { Prototype });
