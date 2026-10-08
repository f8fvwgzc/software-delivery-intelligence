import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { documentKeys } from './keys'

type DocumentKind = components['schemas']['DocumentKind']

/** POST /projects/{id}/documents — generates one document from the knowledge model. */
export function useGenerateDocument(projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (kind: DocumentKind) =>
      unwrap(
        api.POST('/api/v1/projects/{id}/documents', {
          params: { path: { id: projectId } },
          body: { kind },
        }),
      ),
    onSuccess: (document) => {
      // The create response carries the body; seed the detail so the viewer
      // paints without a second round trip.
      queryClient.setQueryData(documentKeys.detail(document.id), document)
      queryClient.invalidateQueries({ queryKey: documentKeys.list(projectId) })
    },
  })
}
