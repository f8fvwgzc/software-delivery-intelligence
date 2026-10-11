import { lazy, type ComponentType } from 'react'
import { createBrowserRouter, type RouteObject } from 'react-router'

import { AppShell } from '@/app/layout/app-shell'
import { RootLayout } from '@/app/layout/root-layout'
import { RedirectIfSignedIn, RequireSession } from '@/app/router/guards'
import { NotFoundPage } from '@/app/router/not-found-page'
import { RouteError } from '@/app/router/route-error'

/*
 * Every page is its own chunk. The Suspense boundary that shows while a
 * chunk loads lives in <AppShell />, around the <Outlet />.
 */
const LoginPage = lazy(() => import('@/features/auth/pages/login-page'))

const OverviewPage = lazy(() => import('@/features/overview/pages/overview-page'))
const ConnectPage = lazy(() => import('@/features/github/pages/connect-page'))

const ProjectsPage = lazy(() => import('@/features/projects/pages/projects-page'))
const ProjectLayout = lazy(() => import('@/features/projects/pages/project-layout'))
const UnderstandingPage = lazy(() => import('@/features/understanding/pages/understanding-page'))
const ArchitecturePage = lazy(() => import('@/features/graph/pages/architecture-page'))
const CodePage = lazy(() => import('@/features/code/pages/code-page'))
const DependenciesPage = lazy(() => import('@/features/dependencies/pages/dependencies-page'))
const DeliveryPage = lazy(() => import('@/features/delivery/pages/delivery-page'))
const ProjectProposalsPage = lazy(() => import('@/features/proposals/pages/project-proposals-page'))
const DocumentsPage = lazy(() => import('@/features/documents/pages/documents-page'))
const DiagramsPage = lazy(() => import('@/features/diagrams/pages/diagrams-page'))
const ProjectExecutionsPage = lazy(() => import('@/features/executions/pages/project-executions-page'))

const ProposalsPage = lazy(() => import('@/features/proposals/pages/proposals-page'))
const ProposalPage = lazy(() => import('@/features/proposals/pages/proposal-page'))

const ExecutionsPage = lazy(() => import('@/features/executions/pages/executions-page'))
const ExecutionPage = lazy(() => import('@/features/executions/pages/execution-page'))
const ApprovalsPage = lazy(() => import('@/features/approvals/pages/approvals-page'))
const DiagramPage = lazy(() => import('@/features/diagrams/pages/diagram-page'))

const PatternsPage = lazy(() => import('@/features/patterns/pages/patterns-page'))
const DecisionsPage = lazy(() => import('@/features/decisions/pages/decisions-page'))
const SearchPage = lazy(() => import('@/features/search/pages/search-page'))

const PoliciesPage = lazy(() => import('@/features/policies/pages/policies-page'))
const SystemPage = lazy(() => import('@/features/system/pages/system-page'))
const SettingsPage = lazy(() => import('@/features/settings/pages/settings-page'))
const MembersPage = lazy(() => import('@/features/workspaces/pages/members-page'))
const AuditPage = lazy(() => import('@/features/workspaces/pages/audit-page'))
const DataPrivacyPage = lazy(() => import('@/features/workspaces/pages/data-privacy-page'))

// Breadcrumbs that need data (a project name, an execution number) are
// components, loaded lazily like everything else.
const ProjectCrumb = lazy(() => import('@/features/projects/components/project-crumb'))
const ProposalCrumb = lazy(() => import('@/features/proposals/components/proposal-crumb'))
const ExecutionCrumb = lazy(() => import('@/features/executions/components/execution-crumb'))
const DiagramCrumb = lazy(() => import('@/features/diagrams/components/diagram-crumb'))

/** Route metadata read by the header breadcrumbs. */
export interface RouteHandle {
  crumb: string | ComponentType
}

const crumb = (value: RouteHandle['crumb']): RouteHandle => ({ crumb: value })

/** Everything inside the authenticated shell, relative to `/`. */
const shellRoutes: RouteObject[] = [
  { index: true, Component: OverviewPage, handle: crumb('Overview') },
  { path: 'connect', Component: ConnectPage, handle: crumb('Connect') },

  {
    path: 'projects',
    handle: crumb('Projects'),
    children: [
      { index: true, Component: ProjectsPage },
      {
        path: ':projectId',
        Component: ProjectLayout,
        handle: crumb(ProjectCrumb),
        children: [
          { index: true, Component: UnderstandingPage },
          { path: 'architecture', Component: ArchitecturePage, handle: crumb('Architecture') },
          { path: 'code', Component: CodePage, handle: crumb('Code') },
          { path: 'code/*', Component: CodePage, handle: crumb('Code') },
          { path: 'dependencies', Component: DependenciesPage, handle: crumb('Dependencies') },
          { path: 'delivery', Component: DeliveryPage, handle: crumb('Delivery') },
          { path: 'proposals', Component: ProjectProposalsPage, handle: crumb('Proposals') },
          { path: 'documents', Component: DocumentsPage, handle: crumb('Documents') },
          { path: 'diagrams', Component: DiagramsPage, handle: crumb('Diagrams') },
          { path: 'executions', Component: ProjectExecutionsPage, handle: crumb('Executions') },
        ],
      },
    ],
  },

  {
    path: 'proposals',
    handle: crumb('Proposals'),
    children: [
      { index: true, Component: ProposalsPage },
      { path: ':proposalId', Component: ProposalPage, handle: crumb(ProposalCrumb) },
    ],
  },

  {
    path: 'executions',
    handle: crumb('Executions'),
    children: [
      { index: true, Component: ExecutionsPage },
      { path: ':executionId', Component: ExecutionPage, handle: crumb(ExecutionCrumb) },
    ],
  },
  { path: 'approvals', Component: ApprovalsPage, handle: crumb('Approvals') },
  { path: 'diagrams/:diagramId', Component: DiagramPage, handle: crumb(DiagramCrumb) },

  {
    path: 'knowledge',
    handle: crumb('Knowledge'),
    children: [
      { path: 'patterns', Component: PatternsPage, handle: crumb('Patterns') },
      { path: 'decisions', Component: DecisionsPage, handle: crumb('Decisions') },
      { path: 'search', Component: SearchPage, handle: crumb('Search') },
    ],
  },

  {
    path: 'workspace',
    handle: crumb('Workspace'),
    children: [
      { path: 'members', Component: MembersPage, handle: crumb('Members') },
      { path: 'audit', Component: AuditPage, handle: crumb('Audit') },
      { path: 'data', Component: DataPrivacyPage, handle: crumb('Data & privacy') },
    ],
  },

  { path: 'policies', Component: PoliciesPage, handle: crumb('Policies') },
  { path: 'system', Component: SystemPage, handle: crumb('System') },
  { path: 'settings', Component: SettingsPage, handle: crumb('Settings') },

  { path: '*', Component: NotFoundPage, handle: crumb('Not found') },
]

export const router = createBrowserRouter([
  {
    Component: RootLayout,
    ErrorBoundary: RouteError,
    children: [
      // Outside the shell: the login page, which sends signed-in users back in.
      {
        Component: RedirectIfSignedIn,
        children: [{ path: 'login', Component: LoginPage }],
      },
      // The shell: needs a session and a workspace; opens the event stream.
      {
        Component: RequireSession,
        children: [{ path: '/', Component: AppShell, children: shellRoutes }],
      },
    ],
  },
])
