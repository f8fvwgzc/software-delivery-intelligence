import {
  ActivityIcon,
  BookOpenIcon,
  BoxesIcon,
  Building2Icon,
  CheckSquareIcon,
  ContainerIcon,
  DatabaseIcon,
  GitPullRequestIcon,
  HistoryIcon,
  FolderGitIcon,
  KeyRoundIcon,
  LayoutDashboardIcon,
  LightbulbIcon,
  PlugIcon,
  ScaleIcon,
  ScrollTextIcon,
  SearchIcon,
  SettingsIcon,
  ShieldCheckIcon,
  UsersIcon,
  type LucideIcon,
} from 'lucide-react'

import { routes } from '@/app/router/routes'
import type { Role } from '@/features/auth/api/types'
import { hasRole } from '@/features/auth/lib/roles'

export interface NavItem {
  label: string
  to: string
  icon: LucideIcon
  /** Highlight when the URL starts with `to` (not only exact matches). */
  prefix?: boolean
  /** Hidden below this role; the gateway refuses the calls anyway (CONTRACT.md §18). */
  minRole?: Role
}

/**
 * Several pages folded under one row of the sidebar: closed until it is
 * opened or one of its pages is the current one. The command palette lists
 * its pages like any others.
 */
export interface NavSection {
  label: string
  icon: LucideIcon
  items: NavItem[]
  /** Hidden below this role, with everything in it. */
  minRole?: Role
}

export type NavEntry = NavItem | NavSection

export interface NavGroup {
  label: string
  items: NavEntry[]
}

export function isNavSection(entry: NavEntry): entry is NavSection {
  return 'items' in entry
}

export const navigation: NavGroup[] = [
  {
    label: 'Workspace',
    items: [
      { label: 'Overview', to: routes.overview(), icon: LayoutDashboardIcon },
      { label: 'Projects', to: routes.projects(), icon: FolderGitIcon, prefix: true },
      { label: 'Connect repository', to: routes.connect(), icon: PlugIcon },
    ],
  },
  {
    label: 'Delivery',
    items: [
      { label: 'Proposals', to: routes.proposals(), icon: LightbulbIcon, prefix: true },
      { label: 'Executions', to: routes.executions(), icon: ActivityIcon, prefix: true },
      { label: 'Approvals', to: routes.approvals(), icon: CheckSquareIcon },
      { label: 'Reviews', to: routes.reviews(), icon: GitPullRequestIcon, prefix: true },
      { label: 'Activity', to: routes.activity(), icon: HistoryIcon },
    ],
  },
  {
    label: 'Knowledge',
    items: [
      { label: 'Patterns', to: routes.patterns(), icon: BoxesIcon },
      { label: 'Decisions', to: routes.decisions(), icon: BookOpenIcon },
      { label: 'Search', to: routes.search(), icon: SearchIcon },
    ],
  },
  {
    label: 'Governance',
    items: [
      { label: 'Policies', to: routes.policies(), icon: ScaleIcon },
      { label: 'System', to: routes.system(), icon: ShieldCheckIcon },
      { label: 'Settings', to: routes.settings(), icon: SettingsIcon },
      {
        label: 'Workspace',
        icon: Building2Icon,
        minRole: 'admin',
        items: [
          { label: 'Members', to: routes.members(), icon: UsersIcon },
          { label: 'Audit', to: routes.audit(), icon: ScrollTextIcon },
          { label: 'Data & privacy', to: routes.dataPrivacy(), icon: DatabaseIcon },
          { label: 'Registries', to: routes.registries(), icon: ContainerIcon },
          { label: 'Secrets', to: routes.secrets(), icon: KeyRoundIcon },
        ],
      },
    ],
  },
]

const allowed = (role: Role, entry: { minRole?: Role }) => !entry.minRole || hasRole(role, entry.minRole)

/** The groups a user of `role` may see; a section or a group with nothing left is dropped. */
export function visibleNavigation(role: Role): NavGroup[] {
  return navigation
    .map((group) => ({
      ...group,
      items: group.items
        .filter((entry) => allowed(role, entry))
        .map((entry) => (isNavSection(entry) ? { ...entry, items: entry.items.filter((item) => allowed(role, item)) } : entry))
        .filter((entry) => !isNavSection(entry) || entry.items.length > 0),
    }))
    .filter((group) => group.items.length > 0)
}

/** Whether `item` is the page at `pathname`: exact, or its subtree for `prefix` items. */
export function isNavItemActive(item: NavItem, pathname: string) {
  if (item.to === '/') return pathname === '/'
  return item.prefix ? pathname.startsWith(item.to) : pathname === item.to
}
