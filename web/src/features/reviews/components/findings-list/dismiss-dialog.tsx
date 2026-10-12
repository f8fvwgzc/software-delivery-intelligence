import { XIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'

import type { useDecideFinding } from '../../api/mutations'
import type { Finding } from '../../api/types'

interface DismissDialogProps {
  finding: Finding
  decide: ReturnType<typeof useDecideFinding>
}

/** Dismissing drops the finding from the published review, so it asks for a note. */
export function DismissDialog({ finding, decide }: DismissDialogProps) {
  const [open, setOpen] = useState(false)
  const [note, setNote] = useState('')
  const dismissing = decide.isPending && decide.variables?.decision === 'dismissed'
  const accepting = decide.isPending && decide.variables?.decision === 'accepted'

  const dismiss = () => {
    decide.mutate(
      { findingId: finding.id, decision: 'dismissed', note },
      {
        onSuccess: () => {
          toast.success('Finding dismissed')
          setOpen(false)
          setNote('')
        },
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="xs" variant="ghost" disabled={accepting}>
          <XIcon data-icon="inline-start" />
          Dismiss
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Dismiss this finding</DialogTitle>
          <DialogDescription>
            “{finding.title}” will not be posted to GitHub. The note stays in the activity log.
          </DialogDescription>
        </DialogHeader>
        <Field>
          <FieldLabel htmlFor={`dismiss-note-${finding.id}`}>Note</FieldLabel>
          <Textarea
            id={`dismiss-note-${finding.id}`}
            value={note}
            onChange={(event) => setNote(event.target.value)}
            placeholder="Why this does not apply here"
            rows={3}
            autoFocus
          />
          <FieldDescription>Optional, but a reason helps the next reviewer.</FieldDescription>
        </Field>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)} disabled={dismissing}>
            Keep
          </Button>
          <Button variant="destructive" onClick={dismiss} loading={dismissing}>
            Dismiss finding
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
