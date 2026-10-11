import { NavLink } from 'react-router'

import { routes } from '@/app/router/routes'
import { cn } from '@/lib/utils'

import type { Project } from '../../api/queries'

interface ProjectTab {
  label: string
  to: string
  /** Only the index route matches exactly; the others match their subtree. */
  end?: boolean
  count?: number
}

function buildTabs(project: Project): ProjectTab[] {
  return [
    { label: 'Understanding', to: routes.project(project.id), end: true, count: project.openQuestions },
    { label: 'Architecture', to: routes.projectArchitecture(project.id) },
    { label: 'Code', to: routes.projectCode(project.id) },
    { label: 'Dependencies', to: routes.projectDependencies(project.id) },
    { label: 'Delivery', to: routes.projectDelivery(project.id) },
    { label: 'Proposals', to: routes.projectProposals(project.id) },
    { label: 'Documents', to: routes.projectDocuments(project.id) },
    { label: 'Diagrams', to: routes.projectDiagrams(project.id) },
    { label: 'Executions', to: routes.projectExecutions(project.id) },
  ]
}

const TAB_CLASS =
  'relative -mb-px inline-flex h-9 shrink-0 items-center gap-1.5 border-b-2 border-transparent px-1 text-sm whitespace-nowrap text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:rounded-sm focus-visible:ring-2 focus-visible:ring-ring/50'

/** Route-backed secondary navigation styled as a tab strip. */
export function ProjectTabs({ project }: { project: Project }) {
  return (
    <nav aria-label="Project sections" className="-mx-1 overflow-x-auto">
      <ul className="flex items-center gap-4 border-b px-1">
        {buildTabs(project).map((tab) => (
          <li key={tab.to}>
            <NavLink
              to={tab.to}
              end={tab.end}
              className={({ isActive }) =>
                cn(TAB_CLASS, isActive && 'border-foreground font-medium text-foreground')
              }
            >
              {tab.label}
              {tab.count !== undefined && tab.count > 0 ? (
                <span className="rounded-md bg-muted px-1.5 text-2xs tabular-nums text-muted-foreground">
                  {tab.count}
                </span>
              ) : null}
            </NavLink>
          </li>
        ))}
      </ul>
    </nav>
  )
}

export function ProjectTabsSkeleton() {
  return (
    <div className="-mx-1 border-b px-1">
      <div className="flex h-9 items-center gap-4">
        {Array.from({ length: 7 }, (_, index) => (
          <div key={index} className="h-4 w-20 animate-pulse rounded-md bg-muted" />
        ))}
      </div>
    </div>
  )
}
