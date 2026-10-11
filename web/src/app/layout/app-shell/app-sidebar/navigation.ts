import {
  ActivityIcon,
  BookOpenIcon,
  BoxesIcon,
  CheckSquareIcon,
  FolderGitIcon,
  LayoutDashboardIcon,
  LightbulbIcon,
  PlugIcon,
  ScaleIcon,
  SearchIcon,
  SettingsIcon,
  ShieldCheckIcon,
  type LucideIcon,
} from 'lucide-react'

import { routes } from '@/app/router/routes'

export interface NavItem {
  label: string
  to: string
  icon: LucideIcon
  /** Highlight when the URL starts with `to` (not only exact matches). */
  prefix?: boolean
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
    ],
  },
]
