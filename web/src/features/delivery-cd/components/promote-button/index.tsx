import { ArrowUpRightIcon, ShieldCheckIcon } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import { usePromote } from '../../api/mutations'
import type { DeploymentRun, PipelineStage, Release } from '../../api/types'
import { isRunOpen, needsAttention } from '../../lib/labels'

interface PromoteButtonProps {
  projectId: string
  stage: PipelineStage
  /** The release that would go to the stage. */
  candidate: Release
  /** The viewer holds the approver role (CONTRACT.md §22). */
  canPromote: boolean
}

/**
 * Promotes the stage's candidate. A protected stage has production
 * semantics, so it asks first, says what the policy will do, and then
 * opens the run's execution, where the approval (and, the first time, the
 * request for access) waits. Any other stage starts the run at once and
 * stays on the board; the toast links to the execution.
 */
export function PromoteButton({ projectId, stage, candidate, canPromote }: PromoteButtonProps) {
  const navigate = useNavigate()
  const promote = usePromote(projectId)
  const [confirming, setConfirming] = useState(false)
  const { environment, lastRun } = stage

  const onStarted = (run: DeploymentRun) => {
    const execution = routes.execution(run.executionId)
    if (confirming) {
      // The confirmed path promised the next screen: the execution, with its approval and access requests.
      setConfirming(false)
      toast.success(`Promotion of ${candidate.version} to ${environment.name} requested`)
      navigate(execution)
      return
    }
    toast.success(`Promoting ${candidate.version} to ${environment.name}`, {
      description: 'Deploy permissions are granted per run: if it pauses for access or approval, open its execution.',
      action: { label: 'Open execution', onClick: () => navigate(execution) },
    })
  }

  const run = () => promote.mutate({ environmentId: environment.id, releaseId: candidate.id }, { onSuccess: onStarted })

  const disabledReason = !canPromote
    ? 'Promoting needs the approver role.'
    : lastRun && needsAttention(lastRun)
      ? 'The last deployment to this stage needs attention. Resolve it on its execution first.'
      : lastRun && isRunOpen(lastRun)
        ? 'A deployment to this stage is still in progress.'
        : null

  const button = (
    <Button
      size="sm"
      onClick={() => (environment.protected ? setConfirming(true) : run())}
      loading={promote.isPending && !confirming}
      disabled={disabledReason !== null}
      aria-label={`Promote ${candidate.version} to ${environment.name}`}
    >
      <ArrowUpRightIcon data-icon="inline-start" />
      Promote
    </Button>
  )

  return (
    <>
      {disabledReason ? (
        <Tooltip>
          {/* A disabled button fires no pointer events; the wrapper carries the tooltip. */}
          <TooltipTrigger asChild>
            <span tabIndex={0} className="inline-flex rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
              {button}
            </span>
          </TooltipTrigger>
          <TooltipContent>{disabledReason}</TooltipContent>
        </Tooltip>
      ) : (
        button
      )}

      <AlertDialog open={confirming} onOpenChange={(next) => (promote.isPending ? undefined : setConfirming(next))}>
        <AlertDialogContent className="sm:max-w-md">
          <AlertDialogHeader>
            <AlertDialogMedia className="bg-warning/10 text-warning">
              <ShieldCheckIcon />
            </AlertDialogMedia>
            <AlertDialogTitle>
              Promote <span className="font-mono">{candidate.version}</span> to {environment.name}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              {environment.name} is protected, so the policy for <span className="font-mono text-xs">deploy.production</span> applies:
              approval required. The run verifies the image, then stops until an approver decides. A failed deploy or health check
              pauses it with retry, rollback and cancel. Deploy permissions are granted per run; you may be asked to grant access
              on the next screen.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={promote.isPending}>Not now</AlertDialogCancel>
            <Button onClick={run} loading={promote.isPending}>
              Request promotion
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
