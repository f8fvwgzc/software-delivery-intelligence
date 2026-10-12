export interface DeploymentRunFilters {
  environmentId?: string
}

/*
 * Three roots, so the live stream and the mutations can target them:
 *
 * - `delivery-cd`: everything a `deployment`, `release` or `environment`
 *   event can change (the board, releases, runs).
 * - `registries` and `workspace-secrets`: workspace configuration, changed
 *   only by an admin's own mutations.
 * - `delivery-artifacts`: generated from the project's facts; it moves with
 *   the environments, not with every deployment.
 */
export const cdKeys = {
  all: ['delivery-cd'] as const,
  pipelines: () => [...cdKeys.all, 'pipeline'] as const,
  pipeline: (projectId: string) => [...cdKeys.pipelines(), projectId] as const,
  releases: () => [...cdKeys.all, 'releases'] as const,
  releasePages: (projectId: string) => [...cdKeys.releases(), projectId, 'pages'] as const,
  release: (releaseId: string) => [...cdKeys.releases(), 'detail', releaseId] as const,
  runs: () => [...cdKeys.all, 'runs'] as const,
  runPages: (projectId: string, filters: DeploymentRunFilters) => [...cdKeys.runs(), projectId, 'pages', filters] as const,
}

export const artifactKeys = {
  all: ['delivery-artifacts'] as const,
  project: (projectId: string) => [...artifactKeys.all, projectId] as const,
}

export const registryKeys = {
  all: ['registries'] as const,
  list: () => [...registryKeys.all, 'list'] as const,
}

export const secretKeys = {
  all: ['workspace-secrets'] as const,
  list: (workspaceId: string) => [...secretKeys.all, workspaceId] as const,
}
