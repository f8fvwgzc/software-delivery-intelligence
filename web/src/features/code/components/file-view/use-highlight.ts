import { useQuery } from '@tanstack/react-query'
import type { ThemedToken } from 'shiki/core'

import { codeKeys } from '../../api/keys'
import type { RepoFile } from '../../api/queries'
import { HIGHLIGHT_MAX_BYTES, shikiLanguage } from '../../lib/highlight-languages'

export type TokenLines = ThemedToken[][]

/** Whether this file gets syntax colours at all; everything else stays plain text. */
function highlightable(file: RepoFile) {
  return !file.binary && !file.truncated && file.content.length <= HIGHLIGHT_MAX_BYTES
}

/**
 * Token lines for a file, or `undefined` while plain text is the right thing
 * to show: before the highlighter chunk and grammar arrive, when the language
 * has no grammar, and when the file is binary, truncated or too large.
 * Cached per file at its commit, so returning to a file never re-tokenizes.
 */
export function useHighlight(projectId: string, file: RepoFile): TokenLines | undefined {
  const lang = highlightable(file) ? shikiLanguage(file.language) : null
  const { data } = useQuery({
    queryKey: codeKeys.tokens(projectId, file.path, file.commitSha, lang ?? ''),
    queryFn: async () => {
      const { tokenize } = await import('../../lib/highlighter')
      return tokenize(file.content, lang!)
    },
    enabled: lang !== null,
    staleTime: Infinity,
    retry: false,
  })
  return data
}
