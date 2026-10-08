import { useId, useState } from 'react'

import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'

import { useSetProjectMode } from '../../api/mutations'
import type { Project } from '../../api/queries'
import { PROJECT_MODE } from '../../lib/labels'
import { ExecuteModeDialog } from './execute-mode-dialog'

interface ModeSwitchProps {
  project: Project
  size?: 'sm' | 'default'
  className?: string
}

/**
 * Observe ⇄ execute toggle. Switching execute on asks first; switching it
 * off is immediate, since observe is the safe default.
 */
export function ModeSwitch({ project, size = 'default', className }: ModeSwitchProps) {
  const id = useId()
  const [confirming, setConfirming] = useState(false)
  const setMode = useSetProjectMode(project)

  const isExecute = project.mode === 'execute'

  const handleCheckedChange = (checked: boolean) => {
    if (checked) {
      setConfirming(true)
      return
    }
    setMode.mutate('observe')
  }

  const handleConfirm = () => {
    setMode.mutate('execute', { onSuccess: () => setConfirming(false) })
  }

  return (
    <div className={cn('inline-flex items-center gap-2', className)}>
      <Switch
        id={id}
        size={size}
        checked={isExecute}
        onCheckedChange={handleCheckedChange}
        disabled={setMode.isPending}
        aria-label={`Execute mode for ${project.name}`}
      />
      <Label
        htmlFor={id}
        className={cn('font-normal', size === 'sm' ? 'text-xs' : 'text-sm', !isExecute && 'text-muted-foreground')}
      >
        {PROJECT_MODE[project.mode].label}
      </Label>
      <ExecuteModeDialog
        projectName={project.name}
        open={confirming}
        onOpenChange={setConfirming}
        onConfirm={handleConfirm}
        pending={setMode.isPending}
      />
    </div>
  )
}
