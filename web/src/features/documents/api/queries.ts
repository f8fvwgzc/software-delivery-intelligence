import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { documentKeys } from './keys'

/** Documents of a project, without their markdown bodies. */
export const documentsQuery = (projectId: string) =>
  queryOptions({
    queryKey: documentKeys.list(projectId),
    queryFn: () =>
      unwrap(api.GET('/api/v1/projects/{id}/documents', { params: { path: { id: projectId } } })),
  })

/** One document with its markdown. Generated text does not change, so keep it. */
export const documentQuery = (documentId: string) =>
  queryOptions({
    queryKey: documentKeys.detail(documentId),
    queryFn: () => unwrap(api.GET('/api/v1/documents/{id}', { params: { path: { id: documentId } } })),
    staleTime: Infinity,
  })
