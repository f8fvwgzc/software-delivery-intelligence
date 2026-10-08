import { LockIcon } from 'lucide-react'
import { toast } from 'sonner'

import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { components } from '@/lib/api/schema'

import { useUpdatePolicy } from '../../api/mutations'
import { EDITABLE_DECISIONS, POLICY_DECISION } from '../../lib/labels'

type Policy = components['schemas']['Policy']
type PolicyDecision = components['schemas']['PolicyDecision']

interface DecisionSelectProps {
  policy: Policy
}

/** The decision cell: a select for editable rows, a locked indicator otherwise. */
export function DecisionSelect({ policy }: DecisionSelectProps) {
  const update = useUpdatePolicy()

  if (!policy.editable) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex h-7 cursor-default items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground ring-1 ring-border/60" tabIndex={0}>
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
    <Select value={policy.decision} onValueChange={change} disabled={update.isPending}>
      {/* The loader lives inside the control, like `<Button loading>`. */}
      <SelectTrigger
        size="sm"
        className="w-32 text-xs data-loading:opacity-80"
        aria-label={`Decision for ${policy.label}`}
        aria-busy={update.isPending || undefined}
        data-loading={update.isPending ? '' : undefined}
      >
        {update.isPending && <Spinner className="size-3 text-muted-foreground" />}
        <SelectValue />
      </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {EDITABLE_DECISIONS.map((decision) => (
              <SelectItem key={decision} value={decision}>
                <span className="flex flex-col">
                  <span>{POLICY_DECISION[decision].label}</span>
                  <span className="text-2xs text-muted-foreground">{POLICY_DECISION[decision].description}</span>
                </span>
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
    </Select>
  )
}
