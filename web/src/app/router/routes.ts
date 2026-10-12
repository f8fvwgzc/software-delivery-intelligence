/** Every in-app URL is built here, so a path changes in exactly one place. */
export const routes = {
  login: () => '/login',
  overview: () => '/',
  connect: () => '/connect',

  projects: () => '/projects',
  project: (projectId: string) => `/projects/${projectId}`,
  projectArchitecture: (projectId: string) => `/projects/${projectId}/architecture`,
  /** The code browser; `path` is repository-relative (`web/src/App.tsx`) and lands in the splat. */
  projectCode: (projectId: string, path = '') =>
    path
      ? `/projects/${projectId}/code/${path.split('/').map(encodeURIComponent).join('/')}`
      : `/projects/${projectId}/code`,
  projectDependencies: (projectId: string) => `/projects/${projectId}/dependencies`,
  projectDelivery: (projectId: string) => `/projects/${projectId}/delivery`,
  projectProposals: (projectId: string) => `/projects/${projectId}/proposals`,
  projectDocuments: (projectId: string) => `/projects/${projectId}/documents`,
  projectExecutions: (projectId: string) => `/projects/${projectId}/executions`,
  projectDiagrams: (projectId: string) => `/projects/${projectId}/diagrams`,
  projectPullRequests: (projectId: string) => `/projects/${projectId}/pull-requests`,

  diagram: (diagramId: string) => `/diagrams/${diagramId}`,

  proposals: () => '/proposals',
  proposal: (proposalId: string) => `/proposals/${proposalId}`,

  executions: () => '/executions',
  execution: (executionId: string) => `/executions/${executionId}`,
  approvals: () => '/approvals',

  reviews: () => '/reviews',
  review: (reviewId: string) => `/reviews/${reviewId}`,
  activity: () => '/activity',

  patterns: () => '/knowledge/patterns',
  decisions: () => '/knowledge/decisions',
  search: () => '/knowledge/search',

  members: () => '/workspace/members',
  audit: () => '/workspace/audit',
  dataPrivacy: () => '/workspace/data',

  policies: () => '/policies',
  system: () => '/system',
  settings: () => '/settings',

  apiDocs: () => '/api/docs',
  /** The trust documents (TRUST.md, PRIVACY.md), served by the app itself. */
  trustDoc: (doc: 'trust' | 'privacy') => `/docs/${doc}`,
} as const
