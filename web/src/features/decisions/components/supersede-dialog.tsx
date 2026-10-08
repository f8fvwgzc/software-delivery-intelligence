import { useState } from 'react'
import { toast } from 'sonner'

import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import type { components } from '@/lib/api/schema'

import { useSupersedeDecision } from '../api/mutations'

type Decision = components['schemas']['Decision']

/** Retires a decision. Rankings stop honouring it immediately. */
export function SupersedeDialog({ decision }: { decision: Decision }) {
  const [open, setOpen] = useState(false)
  const supersede = useSupersedeDecision()

  const confirm = () => {
    supersede.mutate(
      { id: decision.id },
      {
        onSuccess: () => {
          setOpen(false)
          toast('Decision superseded', { description: decision.title })
        },
      },
    )
  }

  return (
    <AlertDialog open={open} onOpenChange={(next) => !supersede.isPending && setOpen(next)}>
      <AlertDialogTrigger asChild>
        <Button size="xs" variant="ghost" className="shrink-0 text-muted-foreground">
          Supersede
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Supersede “{decision.title}”?</AlertDialogTitle>
          <AlertDialogDescription>
            The decision stays in the record but stops steering rankings. Record a new one to replace it.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={supersede.isPending}>Keep decision</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={supersede.isPending}>
            Supersede decision
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
