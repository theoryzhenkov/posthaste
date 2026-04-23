// Posthaste message list — v3 with resizable columns
// Column widths are state; each boundary has a visible, draggable separator.
// Columns use absolute widths so alignment between header and rows is exact.

const PH_DEFAULT_COLS = [
  { id: 'unread', width: 28, resizable: false, label: '', align: 'center' },
  { id: 'flag',   width: 28, resizable: false, label: '',  iconLabel: 'Flag', align: 'center' },
  { id: 'attach', width: 28, resizable: false, label: '',  iconLabel: 'Attach', align: 'center' },
  { id: 'subject',width: 320,resizable: true,  label: 'Subject', sortable: true, sorted: true, dir: 'desc', minWidth: 120 },
  { id: 'from',   width: 180,resizable: true,  label: 'From',    sortable: true, minWidth: 80 },
  { id: 'date',   width: 128,resizable: true,  label: 'Date Received', sortable: true, minWidth: 80 },
  { id: 'account',width: 72, resizable: true,  label: 'Account', sortable: true, align: 'right', minWidth: 54 },
  { id: 'tags',   width: 140,resizable: true,  label: 'Tags',    minWidth: 60 },
];

function UnreadDot({ T, selected }) {
  return (
    <div style={{
      width: 7, height: 7, borderRadius: '50%',
      background: selected ? T.selFg : T.signal.unread,
    }} />
  );
}

function ThreadBadge({ T, n, selected }) {
  return (
    <span style={{
      fontFamily: T.font.mono, fontSize: T.type.meta, fontWeight: 600,
      color: selected ? T.selFg : T.fgMuted,
      background: selected ? 'rgba(255,255,255,0.15)' : T.bgElev,
      border: `1px solid ${selected ? 'transparent' : T.borderSoft}`,
      padding: '0 5px', borderRadius: 3, lineHeight: '14px', minWidth: 18,
      textAlign: 'center',
    }}>{n}</span>
  );
}

// ─────────────────────────────────────────────────────────────
// Column header with visible dividers and drag-to-resize handles
// ─────────────────────────────────────────────────────────────
function ColumnDivider({ T, onDrag, active }) {
  const [hover, setHover] = React.useState(false);
  const hot = hover || active;
  return (
    <div
      onPointerDown={onDrag}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative', width: 1,
        alignSelf: 'center', height: '60%',
        background: T.borderSoft,
        cursor: 'col-resize', flexShrink: 0,
        zIndex: 2,
      }}>
      {/* Wider invisible hit area */}
      <div style={{
        position: 'absolute', top: '-50%', bottom: '-50%', left: -4, right: -4,
        cursor: 'col-resize',
      }} />
      {/* Visible hover/active highlight */}
      {hot && (
        <div style={{
          position: 'absolute', top: '-20%', bottom: '-20%', left: -1, width: 3,
          background: T.accent.coral, pointerEvents: 'none',
        }} />
      )}
    </div>
  );
}

function ColumnHeaderCell({ T, col, onSort }) {
  const [hover, setHover] = React.useState(false);
  return (
    <div
      onClick={col.sortable ? onSort : undefined}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        width: col.width, flexShrink: 0,
        padding: '0 10px', textAlign: col.align || 'left',
        fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 600,
        color: col.sorted ? T.fg : (hover && col.sortable ? T.fgMuted : T.fgFaint),
        textTransform: 'uppercase', letterSpacing: 0.5,
        display: 'flex', alignItems: 'center', gap: 4,
        cursor: col.sortable ? 'pointer' : 'default',
        justifyContent: col.align === 'right' ? 'flex-end' : col.align === 'center' ? 'center' : 'flex-start',
        userSelect: 'none',
        height: '100%',
        background: hover && col.sortable ? T.hoverBg : 'transparent',
        transition: 'background 0.08s, color 0.08s',
        overflow: 'hidden', whiteSpace: 'nowrap',
      }}>
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>
        {col.iconLabel === 'Flag' ? <Icons.Flag size={T.icon.xs} style={{ color: 'currentColor' }} />
         : col.iconLabel === 'Attach' ? <Icons.Attach size={T.icon.xs} style={{ color: 'currentColor' }} />
         : col.label}
      </span>
      {col.sorted && <span style={{ opacity: 0.8 }}>{col.dir === 'asc' ? '↑' : '↓'}</span>}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// Row cell — width-matched to header column
// ─────────────────────────────────────────────────────────────
function RowCell({ T, col, children, selected }) {
  return (
    <div style={{
      width: col.width, flexShrink: 0,
      padding: col.id === 'unread' || col.id === 'flag' || col.id === 'attach'
        ? '0' : '0 10px',
      textAlign: col.align || 'left',
      display: 'flex', alignItems: 'center',
      justifyContent: col.align === 'right' ? 'flex-end' : col.align === 'center' ? 'center' : 'flex-start',
      gap: 6,
      whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
      height: '100%',
      boxSizing: 'border-box',
    }}>
      {children}
    </div>
  );
}

function MessageRow({ T, msg, selected, onClick, density, tags, cols, totalWidth, zebra }) {
  const [hover, setHover] = React.useState(false);
  const [focus, setFocus] = React.useState(false);
  const h = density === 'compact' ? 24 : density === 'roomy' ? 48 : 30;
  const zebraBg = zebra ? T.bgListAlt || T.bgElev : T.bgList;
  const bg = selected ? T.selBg : (hover ? T.hoverBg : zebraBg);
  const fg = selected ? T.selFg : (msg.unread ? T.fg : T.fgMuted);
  const fw = msg.unread ? 600 : 400;

  // Roomy: two-line card (no columns)
  if (density === 'roomy') {
    return (
      <div onClick={onClick} tabIndex={0} role="option" aria-selected={selected}
        onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}
        onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
        style={{
          display: 'flex', padding: '10px 12px', gap: 10,
          borderBottom: `1px solid ${T.borderSoft}`,
          background: selected ? T.selBg : (hover ? T.hoverBg : 'transparent'),
          color: fg, cursor: 'pointer', alignItems: 'flex-start',
          outline: 'none',
          boxShadow: focus ? `inset 0 0 0 2px ${T.focusRing}` : 'none',
        }}>
        <div style={{ width: 8, display: 'flex', justifyContent: 'center', paddingTop: 6 }}>
          {msg.unread && <UnreadDot T={T} selected={selected} />}
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 2 }}>
            <span style={{ fontWeight: fw, fontSize: T.type.body, color: selected ? T.selFg : (msg.unread ? T.fg : T.fgMuted), flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {msg.from.name}
            </span>
            {msg.flagged && <Icons.Flag size={T.icon.xs} style={{ color: selected ? T.selFg : T.signal.flag }} />}
            {msg.hasAttachment && <Icons.Attach size={T.icon.xs} style={{ color: selected ? T.selFg : T.fgFaint }} />}
            <span style={{ fontSize: T.type.meta, color: selected ? T.selFg : T.fgFaint, fontFamily: T.font.mono }}>{msg.dateShort}</span>
          </div>
          <div style={{ fontSize: T.type.body, fontWeight: fw, color: selected ? T.selFg : (msg.unread ? T.fg : T.fgMuted), marginBottom: 2, display: 'flex', alignItems: 'center', gap: 6 }}>
            {msg.threadCount > 1 && <ThreadBadge T={T} n={msg.threadCount} selected={selected} />}
            <span style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{msg.subject}</span>
          </div>
          <div style={{ fontSize: T.type.ui, color: selected ? T.selFg : T.fgSubtle, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
            {msg.preview}
          </div>
          {msg.tags.length > 0 && (
            <div style={{ display: 'flex', gap: 4, marginTop: 4 }}>
              {msg.tags.map((t) => {
                const tag = tags.find((x) => x.id === t);
                if (!tag) return null;
                return (
                  <span key={t} style={{
                    fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 600,
                    color: tag.color, background: `color-mix(in oklab, ${tag.color} 15%, transparent)`,
                    padding: '1px 5px', borderRadius: 3,
                  }}>{tag.name}</span>
                );
              })}
            </div>
          )}
        </div>
      </div>
    );
  }

  // Tabular: column-synced with header
  const rowFont = density === 'compact' ? T.type.ui : T.type.body;
  const cellContent = {
    unread: msg.unread && <UnreadDot T={T} selected={selected} />,
    flag:   msg.flagged && <Icons.Flag size={T.icon.xs} style={{ color: selected ? T.selFg : T.signal.flag }} />,
    attach: msg.hasAttachment && <Icons.Attach size={T.icon.xs} style={{ color: selected ? T.selFg : T.fgFaint }} />,
    subject: (
      <>
        {msg.threadCount > 1 && <ThreadBadge T={T} n={msg.threadCount} selected={selected} />}
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{msg.subject}</span>
      </>
    ),
    from: <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{msg.from.name}</span>,
    date: <span style={{ fontFamily: T.font.mono, fontSize: T.type.meta, color: selected ? T.selFg : T.fgSubtle }}>{msg.date}</span>,
    account: <span style={{ fontFamily: T.font.mono, fontSize: T.type.meta, color: selected ? T.selFg : T.fgFaint }}>{msg.account === 'gmail' ? 'Gmail' : msg.account === 'work' ? 'Work' : 'Univ.'}</span>,
    tags: (
      <div style={{ display: 'flex', gap: 4, alignItems: 'center', overflow: 'hidden' }}>
        {msg.tags.slice(0, 3).map((t) => {
          const tag = tags.find((x) => x.id === t);
          if (!tag) return null;
          return (
            <span key={t} title={tag.name} style={{
              fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 600,
              color: selected ? T.selFg : tag.color,
              background: selected ? 'rgba(255,255,255,0.12)' : `color-mix(in oklab, ${tag.color} 14%, transparent)`,
              padding: '0 5px', borderRadius: 3, lineHeight: '14px', flexShrink: 0,
            }}>{tag.name.slice(0, 4)}</span>
          );
        })}
      </div>
    ),
  };

  return (
    <div onClick={onClick} tabIndex={0} role="option" aria-selected={selected}
      onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex', alignItems: 'stretch',
        height: h, width: totalWidth, background: bg, color: fg,
        cursor: 'pointer', fontSize: rowFont,
        fontWeight: fw,
        outline: 'none',
        boxShadow: focus ? `inset 0 0 0 2px ${T.focusRing}` : 'none',
      }}>
      {cols.map((c) => (
        <RowCell key={c.id} T={T} col={c} selected={selected}>
          {cellContent[c.id]}
        </RowCell>
      ))}
    </div>
  );
}

function MessageList({ T, messages, selected, onSelect, density, tags }) {
  const [cols, setCols] = React.useState(() => {
    try {
      const saved = JSON.parse(localStorage.getItem('ph-cols') || 'null');
      if (saved && Array.isArray(saved)) {
        return PH_DEFAULT_COLS.map((c) => {
          const s = saved.find((x) => x.id === c.id);
          return s ? { ...c, width: s.width } : c;
        });
      }
    } catch (e) {}
    return PH_DEFAULT_COLS;
  });
  const [activeDivider, setActiveDivider] = React.useState(null);

  React.useEffect(() => {
    localStorage.setItem('ph-cols', JSON.stringify(cols.map((c) => ({ id: c.id, width: c.width }))));
  }, [cols]);

  const startDrag = (idx) => (e) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = cols[idx].width;
    const minW = cols[idx].minWidth || 40;
    setActiveDivider(idx);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';

    const move = (ev) => {
      const dx = ev.clientX - startX;
      setCols((cs) => {
        const next = cs.slice();
        next[idx] = { ...next[idx], width: Math.max(minW, startW + dx) };
        return next;
      });
    };
    const up = () => {
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', up);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      setActiveDivider(null);
    };
    document.addEventListener('pointermove', move);
    document.addEventListener('pointerup', up);
  };

  const headerH = 26;
  const isTabular = density !== 'roomy';

  // Measure the list container width so the last column (tags) can flex to
  // fill remaining space when the pane is wider than the sum of fixed columns.
  const containerRef = React.useRef(null);
  const [paneW, setPaneW] = React.useState(0);
  React.useEffect(() => {
    if (!containerRef.current) return;
    const el = containerRef.current;
    const ro = new ResizeObserver(() => setPaneW(el.clientWidth));
    ro.observe(el);
    setPaneW(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  // Effective columns: stretch the last column to absorb extra pane width.
  const effectiveCols = React.useMemo(() => {
    const dividerPx = cols.length - 1;
    const rawTotal = cols.reduce((s, c) => s + c.width, 0) + dividerPx;
    if (!paneW || paneW <= rawTotal) return cols;
    const last = cols[cols.length - 1];
    const extra = paneW - rawTotal;
    return cols.map((c, i) =>
      i === cols.length - 1 ? { ...c, width: c.width + extra } : c
    );
  }, [cols, paneW]);

  const totalWidth = effectiveCols.reduce((s, c) => s + c.width, 0) + (effectiveCols.length - 1);

  const sortBy = (id) => {
    setCols((cs) => cs.map((c) => {
      if (!c.sortable) return c;
      if (c.id === id) return { ...c, sorted: true, dir: c.sorted && c.dir === 'desc' ? 'asc' : 'desc' };
      return { ...c, sorted: false };
    }));
  };

  return (
    <div role="listbox" aria-label="Messages" ref={containerRef} style={{
      display: 'flex', flexDirection: 'column',
      height: '100%', background: T.bgList,
      borderRight: `1px solid ${T.border}`,
      overflow: 'hidden',
    }}>
      {!isTabular ? (
        <div className="ph-scroll" style={{ flex: 1, overflow: 'auto' }}>
          {messages.map((m) => (
            <MessageRow key={m.id} T={T} msg={m} selected={selected === m.id}
              onClick={() => onSelect(m.id)} density={density} tags={tags} />
          ))}
        </div>
      ) : (
        // Single horizontal scroll container wraps header + rows so they
        // scroll together left/right. Rows have their own vertical scroll.
        <div className="ph-scroll" style={{ flex: 1, overflow: 'auto hidden', display: 'flex', flexDirection: 'column', minHeight: 0 }}>
          <div style={{ width: totalWidth, display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
            <div style={{
              height: headerH, display: 'flex', alignItems: 'stretch',
              background: T.bgTitlebar,
              width: totalWidth, flexShrink: 0,
              borderBottom: `1px solid ${T.borderStrong}`,
              position: 'sticky', top: 0, zIndex: 3,
            }}>
              {effectiveCols.map((c, i) => (
                <React.Fragment key={c.id}>
                  <ColumnHeaderCell T={T} col={c} onSort={() => sortBy(c.id)} />
                  {i < effectiveCols.length - 1 && (
                    c.resizable || effectiveCols[i+1].resizable ? (
                      <ColumnDivider T={T} onDrag={startDrag(i)} active={activeDivider === i} />
                    ) : (
                      <div style={{ width: 1, alignSelf: 'center', height: '60%', background: T.borderSoft, flexShrink: 0 }} />
                    )
                  )}
                </React.Fragment>
              ))}
            </div>
            <div className="ph-scroll" style={{ flex: 1, overflow: 'hidden auto', width: totalWidth }}>
              {messages.map((m, idx) => (
                <MessageRow key={m.id} T={T} msg={m} selected={selected === m.id}
                  onClick={() => onSelect(m.id)} density={density} tags={tags}
                  cols={effectiveCols} totalWidth={totalWidth} zebra={idx % 2 === 1} />
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// Row with visible thin dividers aligned with header, to anchor resize lines.
function MessageRowWithDividers({ T, msg, selected, onClick, density, tags, cols, totalWidth }) {
  const [hover, setHover] = React.useState(false);
  const [focus, setFocus] = React.useState(false);
  const h = density === 'compact' ? 24 : 30;
  const bg = selected ? T.selBg : (hover ? T.hoverBg : T.bgList);
  const fg = selected ? T.selFg : (msg.unread ? T.fg : T.fgMuted);
  const fw = msg.unread ? 600 : 400;
  const rowFont = density === 'compact' ? T.type.ui : T.type.body;

  const cellContent = {
    unread: msg.unread && <UnreadDot T={T} selected={selected} />,
    flag:   msg.flagged && <Icons.Flag size={T.icon.xs} style={{ color: selected ? T.selFg : T.signal.flag }} />,
    attach: msg.hasAttachment && <Icons.Attach size={T.icon.xs} style={{ color: selected ? T.selFg : T.fgFaint }} />,
    subject: (
      <>
        {msg.threadCount > 1 && <ThreadBadge T={T} n={msg.threadCount} selected={selected} />}
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{msg.subject}</span>
      </>
    ),
    from: <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{msg.from.name}</span>,
    date: <span style={{ fontFamily: T.font.mono, fontSize: T.type.meta, color: selected ? T.selFg : T.fgSubtle, overflow: 'hidden', textOverflow: 'ellipsis' }}>{msg.date}</span>,
    account: <span style={{ fontFamily: T.font.mono, fontSize: T.type.meta, color: selected ? T.selFg : T.fgFaint }}>{msg.account === 'gmail' ? 'Gmail' : msg.account === 'work' ? 'Work' : 'Univ.'}</span>,
    tags: (
      <div style={{ display: 'flex', gap: 4, alignItems: 'center', overflow: 'hidden' }}>
        {msg.tags.slice(0, 3).map((t) => {
          const tag = tags.find((x) => x.id === t);
          if (!tag) return null;
          return (
            <span key={t} title={tag.name} style={{
              fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 600,
              color: selected ? T.selFg : tag.color,
              background: selected ? 'rgba(255,255,255,0.12)' : `color-mix(in oklab, ${tag.color} 14%, transparent)`,
              padding: '0 5px', borderRadius: 3, lineHeight: '14px', flexShrink: 0,
            }}>{tag.name.slice(0, 4)}</span>
          );
        })}
      </div>
    ),
  };

  return (
    <div onClick={onClick} tabIndex={0} role="option" aria-selected={selected}
      onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex', alignItems: 'stretch',
        height: h, width: totalWidth, background: bg, color: fg,
        cursor: 'pointer', fontSize: rowFont,
        fontWeight: fw,
        outline: 'none',
        boxShadow: focus ? `inset 0 0 0 2px ${T.focusRing}` : 'none',
      }}>
      {cols.map((c, i) => (
        <React.Fragment key={c.id}>
          <div style={{
            width: c.width, flexShrink: 0,
            padding: (c.id === 'unread' || c.id === 'flag' || c.id === 'attach') ? 0 : '0 10px',
            display: 'flex', alignItems: 'center',
            justifyContent: c.align === 'right' ? 'flex-end' : c.align === 'center' ? 'center' : 'flex-start',
            gap: 6, whiteSpace: 'nowrap', overflow: 'hidden',
          }}>
            {cellContent[c.id]}
          </div>
          {i < cols.length - 1 && (
            <div style={{
              width: 1, flexShrink: 0,
              background: selected ? 'transparent' : T.borderSoft,
            }} />
          )}
        </React.Fragment>
      ))}
    </div>
  );
}

function ThreadPane({ T, thread, selected, onSelect }) {
  return (
    <div className="ph-scroll" style={{
      width: '100%', height: '100%',
      background: T.bgList, borderRight: `1px solid ${T.border}`,
      overflow: 'auto',
    }}>
      <div style={{
        padding: '8px 12px', fontSize: T.type.meta, fontFamily: T.font.mono, fontWeight: 600,
        color: T.fgFaint, textTransform: 'uppercase', letterSpacing: 0.6,
        borderBottom: `1px solid ${T.borderSoft}`, background: T.bgTitlebar,
      }}>Thread · {thread.length} messages</div>
      {thread.map((t, i) => (
        <div key={t.id} onClick={() => onSelect(t.id)}
          style={{
            padding: '10px 12px', cursor: 'pointer',
            background: selected === t.id ? T.selBg : 'transparent',
            color: selected === t.id ? T.selFg : T.fg,
            borderBottom: `1px solid ${T.borderSoft}`,
            position: 'relative',
          }}>
          <div style={{ position: 'absolute', left: 6, top: 0, bottom: 0, width: 1, background: T.borderSoft }} />
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 3, position: 'relative' }}>
            <div style={{ width: 6, height: 6, borderRadius: '50%', background: i === 0 ? T.signal.unread : T.fgFaint, marginLeft: -3 }} />
            <span style={{ fontSize: T.type.ui, fontWeight: 600 }}>{t.from}</span>
            <div style={{ flex: 1 }} />
            <span style={{ fontSize: T.type.meta, fontFamily: T.font.mono, color: selected === t.id ? T.selFg : T.fgFaint }}>{t.date}</span>
          </div>
          <div style={{ fontSize: T.type.ui, color: selected === t.id ? T.selFg : T.fgSubtle, marginLeft: 6 }}>{t.excerpt}</div>
        </div>
      ))}
    </div>
  );
}

Object.assign(window, { MessageList, MessageRow, ThreadPane });
