import { XIcon } from 'lucide-react'
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
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'

import { useRejectProposal } from '../../api/mutations'

export function RejectDialog({ proposalId }: { proposalId: string }) {
  const [open, setOpen] = useState(false)
  const [reason, setReason] = useState('')
  const reject = useRejectProposal(proposalId)
  const trimmed = reason.trim()

  const onOpenChange = (next: boolean) => {
    if (reject.isPending) return
    setOpen(next)
    if (!next) setReason('')
  }

  const confirm = () => {
    if (trimmed.length === 0) return
    reject.mutate(trimmed, {
      onSuccess: () => {
        setOpen(false)
        setReason('')
        toast('Proposal rejected')
      },
    })
  }

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="outline">
          <XIcon data-icon="inline-start" />
          Reject
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Reject this proposal?</AlertDialogTitle>
          <AlertDialogDescription>
            The thread stays readable but no option can be accepted afterwards. The reason is kept with the proposal.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="reject-reason">Reason</FieldLabel>
            <Textarea
              id="reject-reason"
              value={reason}
              onChange={(event) => setReason(event.target.value)}
              placeholder="e.g. we are not deploying this quarter"
              rows={3}
              autoFocus
            />
          </Field>
        </FieldGroup>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={reject.isPending}>Keep proposal</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={reject.isPending} disabled={trimmed.length === 0}>
            Reject proposal
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
