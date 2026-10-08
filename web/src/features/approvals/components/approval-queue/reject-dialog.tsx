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
import type { components } from '@/lib/api/schema'

import { useDecideApproval } from '../../api/mutations'

interface RejectDialogProps {
  approval: components['schemas']['Approval']
  decide: ReturnType<typeof useDecideApproval>
}

/** Rejecting ends the execution, so it asks for a note first. */
export function RejectDialog({ approval, decide }: RejectDialogProps) {
  const [open, setOpen] = useState(false)
  const [note, setNote] = useState('')
  const pendingReject = decide.isPending && decide.variables?.approve === false
  const pendingApprove = decide.isPending && decide.variables?.approve === true

  const reject = () => {
    decide.mutate(
      { approvalId: approval.id, approve: false, note },
      {
        onSuccess: () => {
          toast.success(`Rejected "${approval.stepTitle}"`)
          setOpen(false)
        },
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm" disabled={pendingApprove}>
          <XIcon data-icon="inline-start" />
          Reject
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Reject "{approval.stepTitle}"?</DialogTitle>
          <DialogDescription>
            Execution #{approval.executionNumber} is cancelled and nothing after this step runs.
          </DialogDescription>
        </DialogHeader>
        <Field>
          <FieldLabel htmlFor={`reject-note-${approval.id}`}>Note</FieldLabel>
          <Textarea
            id={`reject-note-${approval.id}`}
            value={note}
            onChange={(event) => setNote(event.target.value)}
            placeholder="Why this should not run"
            rows={3}
            autoFocus
          />
          <FieldDescription>Kept with the approval record and shown in the execution log.</FieldDescription>
        </Field>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)} disabled={pendingReject}>
            Keep pending
          </Button>
          <Button variant="destructive" onClick={reject} loading={pendingReject}>
            Reject step
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
