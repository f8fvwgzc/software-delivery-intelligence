import { SendIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { ComboboxField } from '@/components/shared/form/combobox-field'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { usePublishReview } from '../../api/mutations'
import type { Review, ReviewEvent } from '../../api/types'
import { REVIEW_EVENT, canPublish } from '../../lib/labels'
import { count } from '@/lib/intl'

const EVENTS: ReviewEvent[] = ['comment', 'request_changes', 'approve']

/** Posts the review to GitHub. Approver only; the event is chosen first, then confirmed. */
export function PublishDialog({ review }: { review: Review }) {
  const { currentWorkspace } = useAuth()
  const publish = usePublishReview(review.id)
  const [event, setEvent] = useState<ReviewEvent>('comment')
  const [confirming, setConfirming] = useState(false)

  const approver = hasRole(currentWorkspace.myRole, 'approver')
  const allowed = approver && canPublish(review.status)
  if (!canPublish(review.status)) return null

  const run = () => {
    publish.mutate(
      { event },
      {
        onSuccess: (next) => {
          setConfirming(false)
          toast.success('Review published', {
            description: next.publishedUrl ? next.publishedUrl.replace(/^https?:\/\//, '') : `${next.findingsAccepted} comments posted.`,
          })
        },
      },
    )
  }

  const trigger = (
    <Button onClick={() => setConfirming(true)} disabled={!allowed} loading={publish.isPending}>
      <SendIcon data-icon="inline-start" />
      Publish to GitHub
    </Button>
  )

  return (
    <>
      <div className="flex items-center gap-2">
        <Field orientation="horizontal" className="gap-2">
          <FieldLabel htmlFor="review-event" className="sr-only">
            Review event
          </FieldLabel>
          <ComboboxField
            id="review-event"
            size="sm"
            align="end"
            className="w-40"
            value={event}
            onValueChange={(value) => setEvent(value as ReviewEvent)}
            options={EVENTS.map((value) => ({ value, label: REVIEW_EVENT[value].label, description: REVIEW_EVENT[value].description }))}
            disabled={!allowed}
            aria-label="Review event"
          />
        </Field>
        {approver ? (
          trigger
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <span tabIndex={0}>{trigger}</span>
            </TooltipTrigger>
            <TooltipContent>Publishing needs the approver role.</TooltipContent>
          </Tooltip>
        )}
      </div>

      <AlertDialog open={confirming} onOpenChange={setConfirming}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Publish this review to GitHub?</AlertDialogTitle>
            <AlertDialogDescription>
              Posts a “{REVIEW_EVENT[event].label}” review on #{review.prNumber} with the summary as its body and{' '}
              {count(review.findingsAccepted, 'comment', 'comments')} for the accepted findings. Dismissed findings stay here.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={publish.isPending}>Not yet</AlertDialogCancel>
            <AlertDialogAction asChild>
              <Button
                onClick={(event) => {
                  // Stay open while it posts; success closes it.
                  event.preventDefault()
                  run()
                }}
                loading={publish.isPending}
              >
                Publish
              </Button>
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <FieldDescription className="sr-only">Publishing needs the approver role and a review with findings ready.</FieldDescription>
    </>
  )
}
