import { useQuery } from '@tanstack/react-query'
import type { ThemedToken } from 'shiki/core'

import { HIGHLIGHT_MAX_BYTES } from '@/features/code/lib/highlight-languages'

/** Its own root: tokens follow the text they were made from, never the live stream. */
const TOKENS_ROOT = 'gitops-yaml-tokens'

/**
 * YAML tokens for the rendered manifests, from the same lazily loaded shiki
 * highlighter (and yaml grammar) as the code browser. Undefined while the
 * text should stay plain: before the highlighter chunk lands, and above
 * the size where tokenizing would stall the main thread.
 */
export function useYamlTokens(applicationId: string, revision: string, text: string): ThemedToken[][] | undefined {
  const enabled = text.length > 0 && text.length <= HIGHLIGHT_MAX_BYTES
  const { data } = useQuery({
    queryKey: [TOKENS_ROOT, applicationId, revision, text.length],
    queryFn: async () => {
      const { tokenize } = await import('@/features/code/lib/highlighter')
      return tokenize(text, 'yaml')
    },
    enabled,
    staleTime: Infinity,
    gcTime: 60_000,
    retry: false,
  })
  return data
}
