import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { settingsKeys } from './keys'

export { settingsKeys }

export type Settings = components['schemas']['Settings']
export type KnowledgeSettings = components['schemas']['KnowledgeSettings']
export type KnowledgeSettingsPatch = components['schemas']['KnowledgeSettingsPatch']
export type IntelligenceSettings = components['schemas']['IntelligenceSettings']
export type IntelligenceSettingsPatch = components['schemas']['IntelligenceSettingsPatch']
export type ScoreWeights = components['schemas']['ScoreWeights']
export type UpdateSettingsRequest = components['schemas']['UpdateSettingsRequest']

export const settingsQueries = {
  /** Scanner limits and ranking defaults, composed from both services. */
  settings: () =>
    queryOptions({
      queryKey: settingsKeys.settings(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/settings', { signal })),
      // Only this page changes them; a minute of staleness is fine.
      staleTime: 60_000,
    }),
}
