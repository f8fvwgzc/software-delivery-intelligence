import { PencilLineIcon } from 'lucide-react'
import { toast } from 'sonner'

import { DisabledReason } from '@/components/shared/feedback/disabled-reason'
import { Button } from '@/components/ui/button'

import type { useDraftFix } from '../../api/mutations'
import type { Finding } from '../../api/types'

interface DraftFixButtonProps {
  finding: Finding
  draft: ReturnType<typeof useDraftFix>
  /** Why the viewer may not ask (their role); null when they may. */
  disabledReason: string | null
}

/**
 * Asks the workspace's model to write the replacement for the lines the
 * finding points at. Offered only where the server says a fix can be
 * drafted (`canDraftFix`); the draft never changes the finding's decision.
 */
export function DraftFixButton({ finding, draft, disabledReason }: DraftFixButtonProps) {
  const run = () => {
    draft.mutate(finding.id, {
      onSuccess: () => toast.success('Fix drafted', { description: 'The model wrote it. Read it before you accept the finding.' }),
    })
  }

  return (
    <DisabledReason reason={disabledReason}>
      <Button size="xs" variant="outline" onClick={run} loading={draft.isPending} disabled={disabledReason !== null}>
        <PencilLineIcon data-icon="inline-start" />
        Draft a fix
      </Button>
    </DisabledReason>
  )
}
