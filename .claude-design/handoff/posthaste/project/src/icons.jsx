// Lucide icon adapter for Posthaste.
//
// Same API as before: `<Icons.Flag size={16} style={{...}} />`.
// Under the hood we look up the icon's path nodes from window.lucide and
// render them into a <svg>, so we keep full control over stroke weight + size.
//
// Lucide ships icons as [tag, attrs] tuples on a 24×24 grid; we wrap them in
// our own <svg> with currentColor stroke and a size-scaled stroke weight so
// the icon doesn't thin out at 12px.

const LUCIDE_STROKE = {
  12: 1.9,
  14: 1.85,
  16: 1.75,
  18: 1.75,
  20: 1.7,
  24: 1.6,
};

// Convert kebab-case SVG attrs to camelCase so React accepts them.
function kebabToCamelAttrs(attrs) {
  const out = {};
  for (const k in attrs) {
    // Skip attrs we override on the wrapper <svg>.
    if (k === 'stroke' || k === 'stroke-width' || k === 'stroke-linecap'
        || k === 'stroke-linejoin' || k === 'fill' || k === 'xmlns'
        || k === 'width' || k === 'height' || k === 'viewBox') continue;
    const camel = k.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    out[camel] = attrs[k];
  }
  return out;
}

let _lucideKey = 0;
function lucideNode([tag, attrs]) {
  return React.createElement(tag, { ...kebabToCamelAttrs(attrs), key: ++_lucideKey });
}

function makeLucideIcon(name) {
  return function LucideIcon(props = {}) {
    const size = props.size || 16;
    const stroke = LUCIDE_STROKE[size] ?? 1.75;
    const { style = {}, size: _s, ...rest } = props;

    const L = typeof window !== 'undefined' ? window.lucide : null;
    const def = L && L.icons && L.icons[name];

    // Lucide exports each icon as ["svg", svgAttrs, [[childTag, childAttrs], ...]]
    let children;
    if (def && Array.isArray(def) && Array.isArray(def[2])) {
      children = def[2].map(lucideNode);
    } else {
      children = <rect x="2" y="2" width="20" height="20" rx="3" />;
    }

    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
        stroke="currentColor" strokeWidth={stroke}
        strokeLinecap="round" strokeLinejoin="round"
        style={{ flexShrink: 0, ...style }} {...rest}>
        {children}
      </svg>
    );
  };
}

// Map our icon names → Lucide icon names.
const LUCIDE_MAP = {
  Inbox:       'Inbox',
  Archive:     'Archive',
  Drafts:      'FileText',
  Sent:        'Send',
  Junk:        'AlertTriangle',
  Trash:       'Trash2',
  All:         'Mails',
  Star:        'Star',
  Flag:        'Flag',
  Tag:         'Tag',
  Folder:      'Folder',
  Chevron:     'ChevronRight',
  ChevronDown: 'ChevronDown',
  ChevronDown2:'ChevronDown',
  Search:      'Search',
  Compose:     'PenLine',
  Reply:       'Reply',
  ReplyAll:    'ReplyAll',
  Forward:     'Forward',
  Attach:      'Paperclip',
  Download:    'Download',
  More:        'MoreHorizontal',
  Thread:      'MessageSquare',
  Snooze:      'Clock',
  Filter:      'Filter',
  Split:       'Columns2',
  Sidebar:     'PanelLeft',
  Layers:      'Layers',
  Sparkle:     'Sparkles',
  Dot:         'Circle',
  Bolt:        'Zap',
  Check:       'Check',
  Plus:        'Plus',
  X:           'X',
  At:          'AtSign',
  Settings:    'Settings',
  Keyboard:    'Keyboard',
  Command:     'Command',
  Shield:      'Shield',
  User:        'User',
  Users:       'Users',
  Bell:        'Bell',
  Moon:        'Moon',
  Sun:         'Sun',
  Calendar:    'Calendar',
  Globe:       'Globe',
  Key:         'KeyRound',
  Eye:         'Eye',
  EyeOff:      'EyeOff',
  Link:        'Link',
  Lock:        'Lock',
  Unlock:      'Unlock',
  Info:        'Info',
  Help:        'CircleHelp',
  Edit:        'SquarePen',
  Zap:         'Zap',
  Terminal:    'Terminal',
  Webhook:     'Webhook',
  Share:       'Share',
  Pin:         'Pin',
  VolumeX:     'VolumeX',
  EyeClosed:   'EyeOff',
  Trash:       'Trash2',
  Sliders:     'SlidersHorizontal',
  GitBranch:   'GitBranch',
  ArrowRight:  'ArrowRight',
  ArrowLeft:   'ArrowLeft',
  ArrowDown:   'ArrowDown',
  ArrowUp:     'ArrowUp',
  CornerDownLeft: 'CornerDownLeft',
};

const Icons = {};
for (const [ourName, lucideName] of Object.entries(LUCIDE_MAP)) {
  Icons[ourName] = makeLucideIcon(lucideName);
}

// Postmark stamp — kept as-is (not a Lucide icon; it's a brand mark).
function PostmarkStamp({ size = 40, color = 'currentColor', text = 'POST', date = 'HASTE', style = {} }) {
  if (size < 28) {
    return (
      <svg width={size} height={size} viewBox="0 0 40 40" style={style}>
        <circle cx="20" cy="20" r="17" fill="none" stroke={color} strokeWidth="1.6" strokeDasharray="2.5 2.5" opacity="0.85" />
        <circle cx="20" cy="20" r="11" fill="none" stroke={color} strokeWidth="1.6" />
        <path d="M 13 19.2 L 27 19.2 M 13 20.8 L 27 20.8" stroke={color} strokeWidth="1.2" />
      </svg>
    );
  }
  const uid = `${text}-${date}-${size}`;
  return (
    <svg width={size} height={size} viewBox="0 0 40 40" style={style}>
      <defs>
        <path id={`stamp-arc-top-${uid}`} d="M 6 20 A 14 14 0 0 1 34 20" fill="none" />
        <path id={`stamp-arc-bot-${uid}`} d="M 6 20 A 14 14 0 0 0 34 20" fill="none" />
      </defs>
      <circle cx="20" cy="20" r="18" fill="none" stroke={color} strokeWidth="1.3" strokeDasharray="2 2" opacity="0.9" />
      <circle cx="20" cy="20" r="14" fill="none" stroke={color} strokeWidth="1.1" opacity="0.9" />
      <text fontSize="4.2" fontWeight="700" fill={color} letterSpacing="1.2" style={{ fontFamily: "'Geist Mono', monospace" }}>
        <textPath href={`#stamp-arc-top-${uid}`} startOffset="50%" textAnchor="middle">{text}</textPath>
      </text>
      <text fontSize="4.2" fontWeight="700" fill={color} letterSpacing="1.2" style={{ fontFamily: "'Geist Mono', monospace" }}>
        <textPath href={`#stamp-arc-bot-${uid}`} startOffset="50%" textAnchor="middle">{date}</textPath>
      </text>
      <path d="M 13 19 L 27 19 M 13 21 L 27 21" stroke={color} strokeWidth="0.8" opacity="0.6" />
    </svg>
  );
}

Object.assign(window, { Icons, PostmarkStamp });
