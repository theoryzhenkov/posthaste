// Posthaste root app — mounts design canvas with hero + variations, plus Tweaks

function App() {
  const initial = JSON.parse(localStorage.getItem('ph-tweaks') || '{}');
  const [state, setRawState] = React.useState({
    theme: 'dark', preset: 'neutral', density: 'standard', layout: 3, showAdvanced: true,
    ...initial,
  });
  const setState = (patch) => {
    setRawState((s) => {
      const n = { ...s, ...patch };
      localStorage.setItem('ph-tweaks', JSON.stringify(n));
      return n;
    });
  };
  const onTheme = () => setState({ theme: state.theme === 'dark' ? 'light' : 'dark' });

  return (
    <>
      <DesignCanvas>
        <DCSection id="hero" title="Posthaste" subtitle="A modern JMAP client — power tools with a friendly skin">
          <DCArtboard id="main" label="Main prototype · interactive" width={1280} height={820}>
            <Prototype
              theme={state.theme}
              preset={state.preset}
              density={state.density}
              layout={state.layout}
              showAdvanced={state.showAdvanced}
              onTheme={onTheme}
              embedded
            />
          </DCArtboard>
        </DCSection>

        <DCSection id="directions" title="Visual directions" subtitle="Seven aesthetic presets — switch any at runtime from the Tweaks panel">
          {Object.values(PH_THEMES).map((p) => (
            <DCArtboard key={p.id} id={'preset-' + p.id}
              label={`${p.label} · ${p.description}`}
              width={960} height={600}>
              <Prototype preset={p.id}
                theme={p.modes[0]}
                density={state.density}
                layout={3}
                showAdvanced={false}
                embedded />
            </DCArtboard>
          ))}
        </DCSection>

        <DCSection id="states" title="Interactions" subtitle="Compose modal with the postmark send animation">
          <DCArtboard id="compose" label="Compose · hit Send to see the stamp drop" width={1280} height={820}>
            <ComposeDemo state={state} onTheme={onTheme} />
          </DCArtboard>
        </DCSection>
      </DesignCanvas>
      <TweaksPanel state={state} setState={setState} />
    </>
  );
}

function ComposeDemo({ state, onTheme }) {
  const [show, setShow] = React.useState(true);
  return (
    <div style={{ position: 'relative', width: '100%', height: '100%' }}>
      <Prototype
        theme={state.theme}
        preset={state.preset}
        density={state.density}
        layout={state.layout}
        showAdvanced={state.showAdvanced}
        onTheme={onTheme}
        embedded
      />
      {show && (
        <Compose T={resolveTheme(state.preset || 'neutral', state.theme)} onClose={() => setShow(false)} />
      )}
      {!show && (
        <button onClick={() => setShow(true)} style={{
          position: 'absolute', top: 20, right: 20, zIndex: 50,
          padding: '8px 14px', background: resolveTheme(state.preset || 'neutral', state.theme).accent.coral,
          color: '#fff', border: 'none', borderRadius: 6, cursor: 'pointer',
          fontWeight: 600, fontSize: 12,
        }}>Reopen compose</button>
      )}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App />);
