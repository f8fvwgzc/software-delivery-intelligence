import { GitMergeIcon } from 'lucide-react'
import { useId, useState } from 'react'

import { ComboboxField } from '@/components/shared/form/combobox-field'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Switch } from '@/components/ui/switch'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import type { MergeMethod, MergeReadiness, Review } from '../../api/types'
import { allowedMethods, MERGE_METHOD } from '../../lib/merge'
import { mergeGate } from '../../lib/permissions'
import { MergeConfirmDialog } from './merge-confirm-dialog'

interface MergeControlsProps {
  review: Review
  readiness: MergeReadiness
}

/**
 * How to merge and the request itself. The method is one the repository
 * allows; the branch can be deleted afterwards unless it lives in a fork.
 * "Merge" is live only for an approver while nothing blocks; anyone else
 * reads why it is not.
 */
export function MergeControls({ review, readiness }: MergeControlsProps) {
  const id = useId()
  const { currentWorkspace } = useAuth()
  const approver = hasRole(currentWorkspace.myRole, 'approver')
  const methods = allowedMethods(readiness)
  const [picked, setPicked] = useState<MergeMethod | null>(null)
  const [deleteBranch, setDeleteBranch] = useState(false)
  const [confirming, setConfirming] = useState(false)

  // The repository's settings can change between two reads; a method it no longer allows falls back to its first.
  const method = picked && methods.includes(picked) ? picked : (methods[0] ?? null)
  const blocked = mergeGate(currentWorkspace.myRole, readiness)
  // The blockers are listed right above; only what they do not say is repeated here.
  const note = !approver ? blocked : methods.length === 0 ? 'The repository allows no merge method this page knows.' : null

  return (
    <div className="flex flex-col gap-2.5">
      <Field className="gap-1.5">
        <FieldLabel htmlFor={`${id}-method`} className="text-xs font-normal text-muted-foreground">
          Merge method
        </FieldLabel>
        <ComboboxField
          id={`${id}-method`}
          size="sm"
          className="w-full"
          value={method ?? ''}
          onValueChange={(value) => setPicked(value as MergeMethod)}
          options={methods.map((value) => ({ value, label: MERGE_METHOD[value].label, description: MERGE_METHOD[value].description }))}
          placeholder="None allowed"
          searchPlaceholder="Search methods"
          emptyText="No method matches."
          disabled={!approver || methods.length === 0}
        />
      </Field>

      {readiness.isFork ? null : (
        <Field orientation="horizontal" className="gap-2">
          <Switch id={`${id}-delete`} size="sm" checked={deleteBranch} onCheckedChange={setDeleteBranch} disabled={!approver} />
          <FieldLabel htmlFor={`${id}-delete`} className="text-xs font-normal">
            Delete branch after merge
          </FieldLabel>
        </Field>
      )}

      <Button
        size="sm"
        onClick={() => setConfirming(true)}
        disabled={blocked !== null || method === null}
        aria-describedby={note ? `${id}-note` : undefined}
      >
        <GitMergeIcon data-icon="inline-start" />
        Merge
      </Button>
      {note ? (
        <p id={`${id}-note`} className="text-xs leading-4 text-muted-foreground">
          {note}
        </p>
      ) : null}

      {method ? (
        <MergeConfirmDialog
          open={confirming}
          onOpenChange={setConfirming}
          review={review}
          readiness={readiness}
          method={method}
          deleteBranch={deleteBranch && !readiness.isFork}
        />
      ) : null}
    </div>
  )
}
