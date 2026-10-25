import { PlusIcon } from 'lucide-react'
import { useState } from 'react'

import { Button } from '@/components/ui/button'

import { ApplicationEditor } from '../application-editor'

interface NewApplicationButtonProps {
  projectId: string
  variant?: 'default' | 'outline'
}

/** "New application" and the editor it opens (admin; callers render it for admins only). */
export function NewApplicationButton({ projectId, variant = 'default' }: NewApplicationButtonProps) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <Button variant={variant} size="sm" onClick={() => setOpen(true)}>
        <PlusIcon data-icon="inline-start" />
        New application
      </Button>
      <ApplicationEditor projectId={projectId} open={open} onOpenChange={setOpen} />
    </>
  )
}
