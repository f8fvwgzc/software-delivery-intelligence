import { LockIcon } from 'lucide-react'
import { toast } from 'sonner'

import { ComboboxField } from '@/components/shared/form'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { components } from '@/lib/api/schema'
import { testIds } from '@/test-ids'

import { useUpdatePolicy } from '../../api/mutations'
import { EDITABLE_DECISIONS, POLICY_DECISION } from '../../lib/labels'

type Policy = components['schemas']['Policy']
type PolicyDecision = components['schemas']['PolicyDecision']

interface DecisionSelectProps {
  policy: Policy
}

const DECISION_OPTIONS = EDITABLE_DECISIONS.map((decision) => ({
  value: decision,
  label: POLICY_DECISION[decision].label,
  description: POLICY_DECISION[decision].description,
}))

/** The decision cell: a combobox for editable rows, a locked indicator otherwise. */
export function DecisionSelect({ policy }: DecisionSelectProps) {
  const update = useUpdatePolicy()

  if (!policy.editable) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            data-testid={testIds.policies.lockedDecision}
            className="inline-flex h-7 cursor-default items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground ring-1 ring-border/60"
            tabIndex={0}
          >
            <LockIcon className="size-3" />
            {POLICY_DECISION[policy.decision].label}
          </span>
        </TooltipTrigger>
        <TooltipContent>Cannot be relaxed</TooltipContent>
      </Tooltip>
    )
  }

  const change = (decision: string) => {
    if (decision === policy.decision) return
    update.mutate(
      { actionType: policy.actionType, decision: decision as PolicyDecision },
      { onSuccess: () => toast.success(`${policy.label}: ${POLICY_DECISION[decision as PolicyDecision].label.toLowerCase()}`) },
    )
  }

  return (
    <ComboboxField
      size="sm"
      className="w-32"
      aria-label={`Decision for ${policy.label}`}
      value={policy.decision}
      onValueChange={change}
      options={DECISION_OPTIONS}
      // The loader lives inside the control, like `<Button loading>`.
      loading={update.isPending}
      searchPlaceholder="Search decisions"
      emptyText="No decision matches."
    />
  )
}
