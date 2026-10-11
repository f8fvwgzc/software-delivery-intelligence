import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { diagramKeys } from './keys'

export { diagramKeys }

export type Diagram = components['schemas']['Diagram']
export type DiagramKind = components['schemas']['DiagramKind']
export type DiagramSpec = components['schemas']['DiagramSpec']
export type DiagramNode = components['schemas']['DiagramNode']
export type DiagramEdge = components['schemas']['DiagramEdge']
export type DiagramGroup = components['schemas']['DiagramGroup']

/** Diagrams of a project, newest first, without spec and Mermaid. */
export const diagramsQuery = (projectId: string) =>
  queryOptions({
    queryKey: diagramKeys.list(projectId),
    queryFn: () => unwrap(api.GET('/api/v1/projects/{id}/diagrams', { params: { path: { id: projectId } } })),
  })

/** One diagram with its spec, Mermaid and explanation. Edits go through the mutations, which update this entry. */
export const diagramQuery = (diagramId: string) =>
  queryOptions({
    queryKey: diagramKeys.detail(diagramId),
    queryFn: () => unwrap(api.GET('/api/v1/diagrams/{id}', { params: { path: { id: diagramId } } })),
    staleTime: 30_000,
  })
