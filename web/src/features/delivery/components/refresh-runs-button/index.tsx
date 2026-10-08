import { RefreshCwIcon } from 'lucide-react'
import type { ComponentProps } from 'react'

import { Button } from '@/components/ui/button'

import { useRefreshPipelineRuns } from '../../api/mutations'

interface RefreshRunsButtonProps extends Pick<ComponentProps<typeof Button>, 'variant' | 'size'> {
  projectId: string
}

/** Re-syncs runs from the provider; the loader lives inside the button. */
export function RefreshRunsButton({ projectId, variant = 'outline', size = 'sm' }: RefreshRunsButtonProps) {
  const refresh = useRefreshPipelineRuns(projectId)

  return (
    <Button variant={variant} size={size} loading={refresh.isPending} onClick={() => refresh.mutate()}>
      <RefreshCwIcon data-icon="inline-start" />
      Refresh
    </Button>
  )
}
