import { useMutation } from '@tanstack/react-query'
import { PlugZapIcon } from 'lucide-react'

import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'
import { api, unwrap } from '@/lib/api/client'
import { isNetworkError } from '@/lib/api/errors'

const HINT =
  'Sends one tiny request to the model the saved settings point at and says what came back, or which setting is in the way. It spends a few tokens.'

/**
 * `POST /settings/reasoner/test` (CONTRACT.md §19). A misconfiguration is an
 * answer, not an error: the server names the setting and where it lives, so
 * the sentence is shown as it comes. Tests what is saved, not what is typed.
 */
export function ReasonerTest() {
  const test = useMutation({
    mutationFn: () => unwrap(api.POST('/api/v1/settings/reasoner/test')),
    // The outcome is shown next to the button; a toast would say it twice.
    meta: { silent: true },
  })
  const result = test.data
  const connected = result?.status === 'ok'

  const sentence = test.isError
    ? isNetworkError(test.error)
      ? 'The gateway did not answer, so nothing was tested. Try again in a moment.'
      : test.error.message
    : (result?.message ?? HINT)

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-2">
        <Button type="button" variant="outline" size="sm" onClick={() => test.mutate()} loading={test.isPending}>
          <PlugZapIcon data-icon="inline-start" />
          Test connection
        </Button>
        {result ? (
          <Tag tone={connected ? 'success' : 'warning'} dot>
            {connected ? 'Connected' : 'Not connected'}
          </Tag>
        ) : null}
        {result?.model ? (
          <span className="truncate font-mono text-xs text-muted-foreground" title={result.endpoint ?? undefined}>
            {result.provider} · {result.model}
          </span>
        ) : null}
      </div>
      <p className="text-xs text-muted-foreground" role="status" aria-live="polite">
        {sentence}
      </p>
    </div>
  )
}
