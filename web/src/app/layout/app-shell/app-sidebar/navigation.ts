import {
  ActivityIcon,
  BookOpenIcon,
  BoxesIcon,
  CheckSquareIcon,
  DatabaseIcon,
  FolderGitIcon,
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

export interface NavGroup {
  label: string
  items: NavItem[]
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
      { label: 'Members', to: routes.members(), icon: UsersIcon, minRole: 'admin' },
      { label: 'Audit', to: routes.audit(), icon: ScrollTextIcon, minRole: 'admin' },
      { label: 'Data & privacy', to: routes.dataPrivacy(), icon: DatabaseIcon, minRole: 'admin' },
    ],
  },
]

/** The groups a user of `role` may see; a group with nothing left is dropped. */
export function visibleNavigation(role: Role): NavGroup[] {
  return navigation
    .map((group) => ({ ...group, items: group.items.filter((item) => !item.minRole || hasRole(role, item.minRole)) }))
    .filter((group) => group.items.length > 0)
}
