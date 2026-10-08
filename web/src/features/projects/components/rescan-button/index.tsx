import { RefreshCwIcon } from 'lucide-react'
import type { ComponentProps } from 'react'

import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import { useStartScan } from '../../api/mutations'
import { isProjectBusy, type Project } from '../../api/queries'

interface RescanButtonProps extends Pick<ComponentProps<typeof Button>, 'variant' | 'size' | 'className'> {
  project: Project
  /** Icon-only variant for table rows; the label moves to a tooltip. */
  iconOnly?: boolean
}

/** Queues a new scan. Blocked while one is already in flight. */
export function RescanButton({ project, iconOnly = false, variant = 'outline', size, className }: RescanButtonProps) {
  const startScan = useStartScan(project)
  const busy = isProjectBusy(project.status)
  const label = busy ? 'Scan in progress' : 'Rescan'

  if (iconOnly) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant={variant}
            size={size ?? 'icon-sm'}
            className={className}
            aria-label={`Rescan ${project.name}`}
            disabled={busy}
            loading={startScan.isPending}
            onClick={() => startScan.mutate()}
          >
            <RefreshCwIcon />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
    )
  }

  return (
    <Button
      variant={variant}
      size={size}
      className={className}
      disabled={busy}
      loading={startScan.isPending}
      onClick={() => startScan.mutate()}
    >
      <RefreshCwIcon data-icon="inline-start" />
      Rescan
    </Button>
  )
}
