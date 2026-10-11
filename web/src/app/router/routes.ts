/** Every in-app URL is built here, so a path changes in exactly one place. */
export const routes = {
  overview: () => '/',
  connect: () => '/connect',

  projects: () => '/projects',
  project: (projectId: string) => `/projects/${projectId}`,
  projectArchitecture: (projectId: string) => `/projects/${projectId}/architecture`,
  projectDependencies: (projectId: string) => `/projects/${projectId}/dependencies`,
  projectDelivery: (projectId: string) => `/projects/${projectId}/delivery`,
  projectProposals: (projectId: string) => `/projects/${projectId}/proposals`,
  projectDocuments: (projectId: string) => `/projects/${projectId}/documents`,
  projectExecutions: (projectId: string) => `/projects/${projectId}/executions`,

  proposals: () => '/proposals',
  proposal: (proposalId: string) => `/proposals/${proposalId}`,

  executions: () => '/executions',
  execution: (executionId: string) => `/executions/${executionId}`,
  approvals: () => '/approvals',

  patterns: () => '/knowledge/patterns',
  decisions: () => '/knowledge/decisions',
  search: () => '/knowledge/search',

  policies: () => '/policies',
  system: () => '/system',
  settings: () => '/settings',

  apiDocs: () => '/api/docs',
} as const
