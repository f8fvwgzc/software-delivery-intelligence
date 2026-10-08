import { RefreshCwIcon } from 'lucide-react'
import type { ComponentProps } from 'react'

import { Button } from '@/components/ui/button'

import { useRefreshDeployments } from '../../api/mutations'

interface RefreshDeploymentsButtonProps extends Pick<ComponentProps<typeof Button>, 'variant' | 'size'> {
  projectId: string
}

/** Re-syncs deployments from the provider; the loader lives inside the button. */
export function RefreshDeploymentsButton({ projectId, variant = 'outline', size = 'sm' }: RefreshDeploymentsButtonProps) {
  const refresh = useRefreshDeployments(projectId)

  return (
    <Button variant={variant} size={size} loading={refresh.isPending} onClick={() => refresh.mutate()}>
      <RefreshCwIcon data-icon="inline-start" />
      Refresh
    </Button>
  )
}
