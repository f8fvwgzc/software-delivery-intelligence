import { PlusIcon } from 'lucide-react'
import { useState } from 'react'

import { Button } from '@/components/ui/button'

import { EnvironmentEditor } from './index'

interface AddEnvironmentButtonProps {
  projectId: string
  variant?: 'default' | 'outline'
  size?: 'default' | 'sm'
}

/** Opens the editor for a new environment; the service appends it after the last stage (admin). */
export function AddEnvironmentButton({ projectId, variant = 'default', size = 'default' }: AddEnvironmentButtonProps) {
  const [open, setOpen] = useState(false)

  return (
    <>
      <Button variant={variant} size={size} onClick={() => setOpen(true)}>
        <PlusIcon data-icon="inline-start" />
        Add environment
      </Button>
      <EnvironmentEditor projectId={projectId} open={open} onOpenChange={setOpen} />
    </>
  )
}
