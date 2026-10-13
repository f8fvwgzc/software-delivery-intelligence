import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
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

import { useMergePullRequest } from '../../api/mutations'
import type { MergeMethod, MergeReadiness, Review } from '../../api/types'
import { shortSha } from '../../lib/labels'
import { MERGE_METHOD } from '../../lib/merge'
import { RefusalNote } from '@/components/shared/feedback/refusal-note'

interface MergeConfirmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  review: Review
  readiness: MergeReadiness
  method: MergeMethod
  deleteBranch: boolean
}

/**
 * Says what the merge will do before it is asked for: an execution that
 * reads the pull request again, merges exactly the commit GitHub was read
 * at, and waits for an approval unless the workspace's policy is automatic.
 * A refusal stays here as the server's sentence.
 */
export function MergeConfirmDialog({ open, onOpenChange, review, readiness, method, deleteBranch }: MergeConfirmDialogProps) {
  const navigate = useNavigate()
  const merge = useMergePullRequest({ projectId: review.projectId, prNumber: review.prNumber, reviewId: review.id })

  const run = () => {
    merge.mutate(
      { method, deleteBranch },
      {
        onSuccess: (result) => {
          onOpenChange(false)
          toast.success(`Merge of #${review.prNumber} requested`, {
            description: 'Its approval and its outcome are on the execution.',
            action: { label: 'Open execution', onClick: () => navigate(routes.execution(result.executionId)) },
          })
        },
      },
    )
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (merge.isPending) return
        // A refusal belongs to the attempt it answered, not to the next one.
        if (next) merge.reset()
        onOpenChange(next)
      }}
    >
      <AlertDialogContent className="sm:max-w-md">
        <AlertDialogHeader>
          <AlertDialogTitle>
            Merge #{review.prNumber} into <span className="font-mono">{readiness.baseRef}</span>?
          </AlertDialogTitle>
          <AlertDialogDescription>
            Creates an execution that reads the pull request again and merges exactly commit{' '}
            <InlineCode>{shortSha(readiness.headSha)}</InlineCode> with “{MERGE_METHOD[method].label}”
            {deleteBranch ? (
              <>
                , then deletes the branch <InlineCode>{readiness.headRef}</InlineCode>
              </>
            ) : null}
            . The merge waits for an approval unless this workspace’s policy for <InlineCode>repo.merge_pull_request</InlineCode> is
            automatic. If the head moved, a check stopped passing or a conflict appeared in the meantime, the run stops and merges
            nothing.
          </AlertDialogDescription>
        </AlertDialogHeader>
        {merge.isError ? <RefusalNote error={merge.error} /> : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={merge.isPending}>Not now</AlertDialogCancel>
          <Button onClick={run} loading={merge.isPending}>
            Request merge
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
