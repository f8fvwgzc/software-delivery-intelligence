import { useQuery } from '@tanstack/react-query'
import { GitCommitHorizontalIcon } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { DisabledReason } from '@/components/shared/feedback/disabled-reason'
import { RefusalNote } from '@/components/shared/feedback/refusal-note'
import { InlineCode } from '@/components/system/code'
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'
import { count, plural } from '@/lib/intl'

import { useApplyFixes } from '../../api/mutations'
import { mergeQueries } from '../../api/queries'
import type { Review } from '../../api/types'
import { shortSha } from '../../lib/labels'
import { applyFixesGate } from '../../lib/permissions'
import { FixList } from './fix-list'

/**
 * "Apply N fixes": commits the suggestion of every accepted finding to the
 * pull request's branch, through an execution that waits for approval
 * (CONTRACT.md §23.2). Shown while the review has fixes ready; it says what
 * will happen first, and a refusal (the head moved, a fork) stays in the
 * dialog as the server's sentence.
 */
export function ApplyFixesDialog({ review }: { review: Review }) {
  const navigate = useNavigate()
  const { currentWorkspace } = useAuth()
  const apply = useApplyFixes(review.id)
  const [open, setOpen] = useState(false)
  // What the merge frame already asked GitHub: the branch's name and whether it lives in a fork. Never asked for here.
  const { data: pullRequest } = useQuery({ ...mergeQueries.readiness(review.projectId, review.prNumber), enabled: false })

  if (review.fixesReady <= 0) return null

  const disabledReason = applyFixesGate(currentWorkspace.myRole, pullRequest)
  const fixes = count(review.fixesReady, 'fix', 'fixes')

  const run = () => {
    apply.mutate(undefined, {
      onSuccess: (next) => {
        setOpen(false)
        const executionId = next.fixExecutionId
        toast.success('Fix commit requested', {
          description: 'It waits for approval on its execution.',
          action: executionId ? { label: 'Open execution', onClick: () => navigate(routes.execution(executionId)) } : undefined,
        })
      },
    })
  }

  return (
    <>
      <DisabledReason reason={disabledReason}>
        <Button variant="outline" onClick={() => setOpen(true)} disabled={disabledReason !== null}>
          <GitCommitHorizontalIcon data-icon="inline-start" />
          Apply {fixes}
        </Button>
      </DisabledReason>

      <AlertDialog
        open={open}
        onOpenChange={(next) => {
          if (apply.isPending) return
          // A refusal belongs to the attempt it answered, not to the next one.
          if (next) apply.reset()
          setOpen(next)
        }}
      >
        <AlertDialogContent className="sm:max-w-md">
          <AlertDialogHeader>
            <AlertDialogTitle>
              Apply {fixes} to #{review.prNumber}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              Creates an execution that commits the {plural(review.fixesReady, 'suggestion', 'suggestions')} below as one commit on top of{' '}
              <InlineCode>{shortSha(review.headSha)}</InlineCode> on{' '}
              {pullRequest?.headRef ? <InlineCode>{pullRequest.headRef}</InlineCode> : 'the pull request’s branch'}. The commit waits for an
              approval first. Nothing is force-pushed: if the branch moved since this review, the run stops and changes nothing.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <FixList review={review} />
          {apply.isError ? <RefusalNote error={apply.error} /> : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={apply.isPending}>Not yet</AlertDialogCancel>
            <Button onClick={run} loading={apply.isPending}>
              Apply fixes
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
