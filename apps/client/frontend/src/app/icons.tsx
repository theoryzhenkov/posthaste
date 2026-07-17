// Tiny inline SVG icons, stroked with currentColor so they follow the text
// color of whatever control they sit in. No icon font, no dependency.

function Icon({ children, size = 15 }: { children: React.ReactNode; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  )
}

export function ArchiveIcon() {
  return (
    <Icon>
      <rect x="1.5" y="2.5" width="13" height="3.5" rx="0.8" />
      <path d="M2.8 6v6.2a1.3 1.3 0 0 0 1.3 1.3h7.8a1.3 1.3 0 0 0 1.3-1.3V6" />
      <path d="M6.2 9h3.6" />
    </Icon>
  )
}

export function TrashIcon() {
  return (
    <Icon>
      <path d="M2.5 4.2h11" />
      <path d="M5.5 4V2.8a1 1 0 0 1 1-1h3a1 1 0 0 1 1 1V4" />
      <path d="M4 4.4l.7 8.6a1.2 1.2 0 0 0 1.2 1.1h4.2a1.2 1.2 0 0 0 1.2-1.1l.7-8.6" />
    </Icon>
  )
}

export function PaperclipIcon() {
  return (
    <Icon size={13}>
      <path d="M13 7.4 8 12.4a3.2 3.2 0 0 1-4.5-4.5l5.3-5.3a2.1 2.1 0 0 1 3 3l-5.3 5.2a1 1 0 0 1-1.5-1.4l4.9-4.9" />
    </Icon>
  )
}

export function ReplyIcon() {
  return (
    <Icon>
      <path d="M6.5 3 2 7.2l4.5 4.2" />
      <path d="M2.3 7.2h6.5a5 5 0 0 1 5 5v.8" />
    </Icon>
  )
}

export function ComposeIcon() {
  return (
    <Icon>
      <path d="M7 3H3.6A1.6 1.6 0 0 0 2 4.6v7.8A1.6 1.6 0 0 0 3.6 14h7.8a1.6 1.6 0 0 0 1.6-1.6V9" />
      <path d="m11.8 2.3 1.9 1.9L8 10l-2.6.7L6 8.1Z" />
    </Icon>
  )
}

export function CloseIcon() {
  return (
    <Icon>
      <path d="m3.5 3.5 9 9M12.5 3.5l-9 9" />
    </Icon>
  )
}
