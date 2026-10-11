/** Everything scoped to one workspace id: members, audit, data controls. */
export const workspaceKeys = {
  all: ['workspaces'] as const,
  workspace: (id: string) => [...workspaceKeys.all, id] as const,
  members: (id: string) => [...workspaceKeys.workspace(id), 'members'] as const,
  audits: (id: string) => [...workspaceKeys.workspace(id), 'audit'] as const,
  audit: (id: string, actionPrefix: string) => [...workspaceKeys.audits(id), { actionPrefix }] as const,
  dataControls: (id: string) => [...workspaceKeys.workspace(id), 'data-controls'] as const,
}
