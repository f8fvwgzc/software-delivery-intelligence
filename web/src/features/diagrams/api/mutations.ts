import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { diagramKeys } from './keys'

type GenerateDiagramRequest = components['schemas']['GenerateDiagramRequest']
type UpdateDiagramRequest = components['schemas']['UpdateDiagramRequest']

/** POST /projects/{id}/diagrams — builds a diagram from the knowledge graph, tree and key files. */
export function useGenerateDiagram(projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: GenerateDiagramRequest) =>
      unwrap(api.POST('/api/v1/projects/{id}/diagrams', { params: { path: { id: projectId } }, body })),
    onSuccess: (diagram) => {
      // The create response carries spec and Mermaid; seed the detail so the
      // diagram page paints without a second round trip.
      queryClient.setQueryData(diagramKeys.detail(diagram.id), diagram)
      queryClient.invalidateQueries({ queryKey: diagramKeys.list(projectId) })
    },
  })
}

/** PATCH /diagrams/{id} — title, spec or Mermaid. The response replaces the cached detail. */
export function useUpdateDiagram(diagramId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: UpdateDiagramRequest) =>
      unwrap(api.PATCH('/api/v1/diagrams/{id}', { params: { path: { id: diagramId } }, body })),
    onSuccess: (diagram) => {
      queryClient.setQueryData(diagramKeys.detail(diagram.id), diagram)
      queryClient.invalidateQueries({ queryKey: diagramKeys.lists() })
    },
  })
}

/** DELETE /diagrams/{id}. */
export function useDeleteDiagram(projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (diagramId: string) =>
      unwrap(api.DELETE('/api/v1/diagrams/{id}', { params: { path: { id: diagramId } } })),
    onSuccess: (_, diagramId) => {
      queryClient.removeQueries({ queryKey: diagramKeys.detail(diagramId) })
      queryClient.invalidateQueries({ queryKey: diagramKeys.list(projectId) })
    },
  })
}
