// Keyboard shortcuts overlay — ? or cmd-/

const PH_SHORTCUTS = [
  { group: 'Navigation', items: [
    ['⌘K', 'Open command palette'],
    ['⌘,', 'Settings'],
    ['?', 'This cheatsheet'],
    ['J / K', 'Next / previous message'],
    ['G I', 'Go to Inbox'],
    ['G F', 'Go to Flagged'],
    ['G D', 'Go to Drafts'],
  ]},
  { group: 'Actions', items: [
    ['⌘N', 'Compose new message'],
    ['⌘R / ⇧⌘R', 'Reply / Reply all'],
    ['⇧⌘F', 'Forward'],
    ['E', 'Archive'],
    ['⌫', 'Delete'],
    ['⇧⌘L', 'Flag'],
    ['H', 'Snooze…'],
    ['L', 'Label / tag'],
    ['M', 'Mute thread'],
    ['U', 'Mark unread'],
  ]},
  { group: 'Compose', items: [
    ['⌘↵', 'Send'],
    ['⌘⇧↵', 'Schedule send'],
    ['⌘;', 'Insert template'],
    ['⌘\\', 'Toggle tracking'],
    ['⌘⇧A', 'Attach file'],
  ]},
  { group: 'View', items: [
    ['⌘1/2/3', 'Layout: 1, 2, or 3 panes'],
    ['⌘B', 'Toggle sidebar'],
    ['⌘⇧D', 'Toggle dark mode'],
    ['/', 'Focus search'],
  ]},
];

function ShortcutsOverlay({ T, onClose }) {
  return (
    <Modal T={T} onClose={onClose} width={760} height={600}>
      <ModalHeader T={T} icon={Icons.Keyboard}
        title="Keyboard shortcuts"
        subtitle="Posthaste is keyboard-first. Here's everything."
        onClose={onClose} />
      <div className="ph-scroll" style={{ flex: 1, overflow: 'auto', padding: 22,
        display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 28 }}>
        {PH_SHORTCUTS.map((grp) => (
          <div key={grp.group}>
            <SectionLabel T={T} style={{ marginBottom: 10 }}>{grp.group}</SectionLabel>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              {grp.items.map(([keys, desc]) => (
                <div key={keys} style={{ display: 'flex', alignItems: 'center', gap: 10,
                  padding: '4px 0', fontSize: T.type.body, color: T.fg }}>
                  <span style={{ display: 'flex', gap: 3, flexShrink: 0, minWidth: 90 }}>
                    {keys.split(' ').map((k, i) => <Kbd key={i} T={T}>{k}</Kbd>)}
                  </span>
                  <span style={{ color: T.fgMuted }}>{desc}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
      <ModalFooter T={T} hint="Press ? anywhere to re-open this sheet">
        <ModalButton T={T} variant="primary" onClick={onClose}>Got it</ModalButton>
      </ModalFooter>
    </Modal>
  );
}

Object.assign(window, { ShortcutsOverlay });
