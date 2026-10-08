/** Query key factory for the architecture graph. Invalidate `graphKeys.project(id)` after a scan. */
export const graphKeys = {
  all: ['graph'] as const,
  project: (projectId: string) => [...graphKeys.all, projectId] as const,
  graph: (projectId: string, includeHistory: boolean) =>
    [...graphKeys.project(projectId), 'graph', { includeHistory }] as const,
  impact: (projectId: string, nodeId: string) => [...graphKeys.project(projectId), 'impact', nodeId] as const,
}
