/** Query key factory for diagrams. The live-events provider invalidates `diagramKeys.all` on `entity: "diagram"`. */
export const diagramKeys = {
  all: ['diagrams'] as const,
  lists: () => [...diagramKeys.all, 'list'] as const,
  list: (projectId: string) => [...diagramKeys.lists(), projectId] as const,
  details: () => [...diagramKeys.all, 'detail'] as const,
  detail: (diagramId: string) => [...diagramKeys.details(), diagramId] as const,
}
