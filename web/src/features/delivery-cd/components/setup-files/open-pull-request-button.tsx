import { useQueryClient } from '@tanstack/react-query'
import { GitPullRequestArrowIcon } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'
import { useCreateExecution } from '@/features/executions/api/mutations'

import { cdQueries } from '../../api/queries'
import type { DeliveryArtifacts } from '../../api/types'
import { setupPullRequest } from '../../lib/setup-pull-request'

interface OpenPullRequestButtonProps {
  projectId: string
  files: DeliveryArtifacts
}

/**
 * Offers the setup files as a pull request: creates a `github` execution
 * that carries them (the plan the proposals page uses) and opens it. Nothing
 * reaches the repository until that execution is started and passes its
 * gates. GitHub executions need the approver role.
 */
export function OpenPullRequestButton({ projectId, files }: OpenPullRequestButtonProps) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { currentWorkspace } = useAuth()
  const create = useCreateExecution()
  const [preparing, setPreparing] = useState(false)
  const allowed = hasRole(currentWorkspace.myRole, 'approver')

  const open = async () => {
    setPreparing(true)
    try {
      // The repository and its default branch: cached by the project layout.
      const project = await queryClient.ensureQueryData(cdQueries.project(projectId))
      create.mutate(setupPullRequest(project, files), {
        onSuccess: (execution) => {
          toast.success(`Execution #${execution.number} created`, { description: 'Start it to open the pull request.' })
          navigate(routes.execution(execution.id))
        },
        // A refused create is reported by the mutation cache (lib/query-client.ts).
        onSettled: () => setPreparing(false),
      })
    } catch {
      setPreparing(false)
      toast.error('Could not read the project', { description: 'Its repository and default branch are needed to open a pull request.' })
    }
  }

  const button = (
    <Button size="sm" onClick={open} loading={preparing} disabled={!allowed || files.artifacts.length === 0}>
      <GitPullRequestArrowIcon data-icon="inline-start" />
      Open pull request
    </Button>
  )

  if (allowed) return button
  return (
    <Tooltip>
      {/* A disabled button fires no pointer events; the wrapper carries the tooltip. */}
      <TooltipTrigger asChild>
        <span tabIndex={0} className="inline-flex rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
          {button}
        </span>
      </TooltipTrigger>
      <TooltipContent>Opening a pull request needs the approver role.</TooltipContent>
    </Tooltip>
  )
}
