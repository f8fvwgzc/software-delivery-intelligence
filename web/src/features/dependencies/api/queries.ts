import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { dependencyKeys } from './keys'

export { dependencyKeys }

export type DependenciesResponse = components['schemas']['DependenciesResponse']
export type Dependency = components['schemas']['Dependency']
export type EcosystemCount = components['schemas']['EcosystemCount']
export type DependabotStatus = components['schemas']['DependabotStatus']

export const dependencyQueries = {
  list: (projectId: string) =>
    queryOptions({
      queryKey: dependencyKeys.list(projectId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/projects/{id}/dependencies', { params: { path: { id: projectId } }, signal })),
      // Read at the scanned commit; a rescan invalidates the project family.
      staleTime: 10 * 60_000,
    }),
}
