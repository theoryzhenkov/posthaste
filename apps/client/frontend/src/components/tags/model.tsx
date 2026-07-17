/**
 * Shared tag presentation: the curated color palette + lucide icon set, and the
 * resolver that turns a tag name plus its optional {@link TagAppearance}
 * override into concrete chip styles. Used by the settings pane (to pick) and
 * the chip (to render), so picker and render agree.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
import {
  Bell,
  Book,
  Bookmark,
  Briefcase,
  Bug,
  Calendar,
  Camera,
  Car,
  Check,
  CircleAlert,
  Clock,
  Cloud,
  Code,
  Coffee,
  CreditCard,
  DollarSign,
  FileText,
  Flag,
  Flame,
  Folder,
  Gift,
  GraduationCap,
  Heart,
  House,
  Image,
  Leaf,
  Lightbulb,
  Link,
  Mail,
  MapPin,
  MessageCircle,
  Music,
  Plane,
  ShoppingCart,
  Star,
  Sun,
  Tag,
  Target,
  Trophy,
  User,
  Users,
  Zap,
  type LucideIcon,
} from 'lucide-react'

import type { TagAppearance } from '@/api/types'
import { smartMailboxAccent } from '@/mailboxRoles'

/** A curated foreground/background pair. Backgrounds use alpha so the chip
 *  tints over either light or dark surfaces; foregrounds read on both. */
export interface TagColorSwatch {
  id: string
  fg: string
  bg: string
}

export const TAG_COLOR_SWATCHES: readonly TagColorSwatch[] = [
  {
    id: 'slate',
    fg: 'oklch(0.55 0.02 255)',
    bg: 'oklch(0.62 0.02 255 / 0.18)',
  },
  { id: 'red', fg: 'oklch(0.58 0.18 22)', bg: 'oklch(0.66 0.18 22 / 0.16)' },
  { id: 'orange', fg: 'oklch(0.62 0.16 55)', bg: 'oklch(0.7 0.16 55 / 0.16)' },
  { id: 'amber', fg: 'oklch(0.62 0.13 85)', bg: 'oklch(0.74 0.14 85 / 0.18)' },
  {
    id: 'green',
    fg: 'oklch(0.57 0.14 150)',
    bg: 'oklch(0.68 0.14 150 / 0.16)',
  },
  { id: 'teal', fg: 'oklch(0.57 0.1 195)', bg: 'oklch(0.68 0.1 195 / 0.16)' },
  { id: 'blue', fg: 'oklch(0.57 0.14 245)', bg: 'oklch(0.66 0.14 245 / 0.16)' },
  {
    id: 'violet',
    fg: 'oklch(0.57 0.16 295)',
    bg: 'oklch(0.66 0.16 295 / 0.16)',
  },
  { id: 'pink', fg: 'oklch(0.6 0.18 350)', bg: 'oklch(0.68 0.18 350 / 0.16)' },
]

/** Curated lucide icons assignable to a tag, keyed by stable lucide name. */
export const TAG_ICONS: Record<string, LucideIcon> = {
  tag: Tag,
  star: Star,
  flag: Flag,
  bookmark: Bookmark,
  heart: Heart,
  bell: Bell,
  briefcase: Briefcase,
  house: House,
  user: User,
  users: Users,
  mail: Mail,
  folder: Folder,
  'file-text': FileText,
  calendar: Calendar,
  clock: Clock,
  check: Check,
  'circle-alert': CircleAlert,
  'dollar-sign': DollarSign,
  'credit-card': CreditCard,
  'shopping-cart': ShoppingCart,
  gift: Gift,
  plane: Plane,
  car: Car,
  coffee: Coffee,
  book: Book,
  code: Code,
  bug: Bug,
  zap: Zap,
  flame: Flame,
  leaf: Leaf,
  sun: Sun,
  cloud: Cloud,
  music: Music,
  camera: Camera,
  image: Image,
  link: Link,
  'map-pin': MapPin,
  'message-circle': MessageCircle,
  lightbulb: Lightbulb,
  target: Target,
  trophy: Trophy,
  'graduation-cap': GraduationCap,
}

export const TAG_ICON_NAMES: readonly string[] = Object.keys(TAG_ICONS)

/** Resolved chip styling for a tag. */
export interface TagStyle {
  fg: string
  bg: string
  Icon: LucideIcon
}

/**
 * Resolve a tag's chip styling from its name and optional override. Absent
 * override fields fall back to the name-derived accent (a subtle tint) and the
 * generic tag icon, so unconfigured tags still render as colored chips.
 */
export function resolveTagStyle(
  name: string,
  override?: TagAppearance | null,
): TagStyle {
  const accent = smartMailboxAccent(null, name)
  return {
    fg: override?.fg ?? accent,
    bg: override?.bg ?? `color-mix(in oklab, ${accent} 16%, transparent)`,
    Icon: (override?.icon && TAG_ICONS[override.icon]) || Tag,
  }
}
