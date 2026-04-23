// Posthaste Tweaks panel

function TweaksPanel({ state, setState }) {
  const T = resolveTheme(state.preset || 'neutral', state.theme);
  const preset = PH_THEMES[state.preset || 'neutral'];
  const availableModes = preset.modes;
  return (
    <div style={{
      position: 'fixed', right: 16, bottom: 16, zIndex: 1000,
      width: 280, background: T.bgElev,
      border: `1px solid ${T.border}`, borderRadius: 10,
      boxShadow: '0 12px 40px rgba(0,0,0,0.35)',
      padding: 12, fontFamily: T.font.sans, color: T.fg,
      backdropFilter: 'blur(20px)', WebkitBackdropFilter: 'blur(20px)',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 7, marginBottom: 10 }}>
        <PostmarkStamp size={18} color={T.accent.coral} />
        <div style={{ fontSize: 13, fontWeight: 700, letterSpacing: -0.2 }}>Tweaks</div>
      </div>

      <TweakGroup T={T} label="Preset">
        {Object.values(PH_THEMES).map((p) => (
          <TweakChip key={p.id} T={T} active={(state.preset || 'neutral') === p.id}
            onClick={() => {
              const patch = { preset: p.id };
              if (!p.modes.includes(state.theme)) patch.theme = p.modes[0];
              setState(patch);
            }}>{p.label}</TweakChip>
        ))}
      </TweakGroup>

      <TweakGroup T={T} label="Mode">
        {['dark', 'light'].map((k) => (
          <TweakChip key={k} T={T} active={state.theme === k}
            disabled={!availableModes.includes(k)}
            onClick={() => availableModes.includes(k) && setState({ theme: k })}>{k}</TweakChip>
        ))}
      </TweakGroup>

      <TweakGroup T={T} label="Density">
        {['compact', 'standard', 'roomy'].map((k) => (
          <TweakChip key={k} T={T} active={state.density === k} onClick={() => setState({ density: k })}>{k}</TweakChip>
        ))}
      </TweakGroup>

      <TweakGroup T={T} label="Layout (panes)">
        {[2, 3].map((k) => (
          <TweakChip key={k} T={T} active={state.layout === k} onClick={() => setState({ layout: k })}>{k}</TweakChip>
        ))}
      </TweakGroup>

      <TweakGroup T={T} label="Advanced chrome">
        <TweakChip T={T} active={state.showAdvanced} onClick={() => setState({ showAdvanced: !state.showAdvanced })}>
          {state.showAdvanced ? 'shown' : 'hidden'}
        </TweakChip>
      </TweakGroup>
    </div>
  );
}

function TweakGroup({ T, label, children }) {
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{
        fontSize: 9.5, fontFamily: T.font.mono, fontWeight: 600,
        color: T.fgFaint, textTransform: 'uppercase', letterSpacing: 0.7,
        marginBottom: 5,
      }}>{label}</div>
      <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>{children}</div>
    </div>
  );
}

function TweakChip({ T, active, onClick, children, disabled }) {
  return (
    <button onClick={onClick} disabled={disabled} style={{
      border: `1px solid ${active ? T.accent.coral : T.borderSoft}`,
      background: active ? T.accent.coralSoft : T.bg,
      color: active ? T.accent.coralDeep : T.fg,
      padding: '4px 9px', borderRadius: (T.radius && T.radius.sm) || 5,
      fontSize: 11, fontWeight: 600, fontFamily: 'inherit',
      cursor: disabled ? 'not-allowed' : 'pointer', textTransform: 'capitalize',
      opacity: disabled ? 0.4 : 1,
    }}>{children}</button>
  );
}

Object.assign(window, { TweaksPanel });
