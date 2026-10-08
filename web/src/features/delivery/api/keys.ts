/** Query key factory for delivery data. A refresh invalidates `deliveryKeys.project(id)`. */
export const deliveryKeys = {
  all: ['delivery'] as const,
  project: (projectId: string) => [...deliveryKeys.all, projectId] as const,
  pipelines: (projectId: string) => [...deliveryKeys.project(projectId), 'pipelines'] as const,
  runs: (projectId: string, limit: number) => [...deliveryKeys.project(projectId), 'runs', { limit }] as const,
  deployments: (projectId: string, limit: number) =>
    [...deliveryKeys.project(projectId), 'deployments', { limit }] as const,
}
