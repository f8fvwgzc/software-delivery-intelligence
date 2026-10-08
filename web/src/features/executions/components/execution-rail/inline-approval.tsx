import { CheckIcon, XIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { useDecideApproval } from '@/features/approvals/api/mutations'
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
import { Field, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'

interface InlineApprovalProps {
  approvalId: string
  stepTitle: string
}

/** Approve/reject pair shown on a step that is waiting for a human. */
export function InlineApproval({ approvalId, stepTitle }: InlineApprovalProps) {
  const decide = useDecideApproval()
  const [rejectOpen, setRejectOpen] = useState(false)
  const [note, setNote] = useState('')
  const pendingApprove = decide.isPending && decide.variables?.approve === true
  const pendingReject = decide.isPending && decide.variables?.approve === false

  const approve = () => {
    decide.mutate({ approvalId, approve: true }, { onSuccess: () => toast.success(`Approved "${stepTitle}"`) })
  }

  const reject = () => {
    decide.mutate(
      { approvalId, approve: false, note },
      {
        onSuccess: () => {
          toast.success(`Rejected "${stepTitle}"`)
          setRejectOpen(false)
        },
      },
    )
  }

  return (
    <div className="flex flex-wrap items-center gap-2 rounded-md bg-warning/[0.06] px-3 py-2 ring-1 ring-warning/20">
      <span className="mr-auto text-xs text-foreground">This step is waiting for your decision.</span>
      <AlertDialog open={rejectOpen} onOpenChange={setRejectOpen}>
        <AlertDialogTrigger asChild>
          <Button variant="outline" size="sm" disabled={pendingApprove}>
            <XIcon data-icon="inline-start" />
            Reject
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Reject "{stepTitle}"?</AlertDialogTitle>
            <AlertDialogDescription>The execution is cancelled and nothing after this step runs.</AlertDialogDescription>
          </AlertDialogHeader>
          <Field>
            <FieldLabel htmlFor={`reject-note-${approvalId}`}>Note</FieldLabel>
            <Textarea
              id={`reject-note-${approvalId}`}
              value={note}
              onChange={(event) => setNote(event.target.value)}
              placeholder="Why this should not run (optional)"
              rows={2}
            />
          </Field>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={pendingReject}>Keep waiting</AlertDialogCancel>
            <Button variant="destructive" onClick={reject} loading={pendingReject}>
              Reject step
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <Button size="sm" onClick={approve} loading={pendingApprove} disabled={pendingReject}>
        <CheckIcon data-icon="inline-start" />
        Approve
      </Button>
    </div>
  )
}
