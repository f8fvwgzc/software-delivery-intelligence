import { PlusIcon } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { ComboboxField } from '@/components/shared/form'
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
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { InputGroup, InputGroupAddon, InputGroupInput, InputGroupText } from '@/components/ui/input-group'
import { Textarea } from '@/components/ui/textarea'

import { useGenerateProposal } from '../../api/mutations'
import { PROPOSAL_KIND_LABEL, PROPOSAL_KINDS, type ProposalKind } from '../../lib/labels'
import { ProjectSelect } from './project-select'

const KIND_OPTIONS = PROPOSAL_KINDS.map((kind) => ({ value: kind, label: PROPOSAL_KIND_LABEL[kind] }))

interface NewProposalDialogProps {
  /** When set, the project is fixed and the picker is hidden. */
  projectId?: string
  /** Kind preselected when the dialog opens, e.g. from `?kind=` on the project page. */
  initialKind?: ProposalKind
}

/** "New proposal": pick a project and a kind, optionally a budget and a description of the ask. */
export function NewProposalDialog({ projectId, initialKind }: NewProposalDialogProps) {
  const [open, setOpen] = useState(false)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="sm">
          <PlusIcon data-icon="inline-start" />
          New proposal
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        {/* Mounted per open so the form resets each time without an effect. */}
        {open ? (
          <NewProposalForm projectId={projectId} initialKind={initialKind} onDone={() => setOpen(false)} />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

interface NewProposalFormProps {
  projectId?: string
  initialKind?: ProposalKind
  onDone: () => void
}

/** Parses the budget input: empty is "no budget", anything else must be a non-negative amount. */
function parseBudget(raw: string): { value: number | null; error?: string } {
  const trimmed = raw.trim()
  if (trimmed === '') return { value: null }
  const value = Number(trimmed)
  if (!Number.isFinite(value) || value < 0) return { value: null, error: 'Enter a monthly amount in USD, or leave it empty.' }
  return { value: value === 0 ? null : value }
}

function NewProposalForm({ projectId: fixedProjectId, initialKind, onDone }: NewProposalFormProps) {
  const navigate = useNavigate()
  const generate = useGenerateProposal()
  const [projectId, setProjectId] = useState(fixedProjectId ?? '')
  const [kind, setKind] = useState<ProposalKind>(initialKind ?? 'deployment')
  const [budget, setBudget] = useState('')
  const [prompt, setPrompt] = useState('')

  const parsedBudget = parseBudget(budget)
  const canSubmit = projectId !== '' && !parsedBudget.error && !generate.isPending

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSubmit) return
    generate.mutate(
      { projectId, kind, prompt: prompt.trim() || null, budgetMonthlyUsd: parsedBudget.value },
      {
        onSuccess: (proposal) => {
          onDone()
          navigate(routes.proposal(proposal.id))
        },
      },
    )
  }

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-5">
      <DialogHeader>
        <DialogTitle>New proposal</DialogTitle>
        <DialogDescription>
          The platform ranks the catalog against what it knows about the project and cites its evidence.
        </DialogDescription>
      </DialogHeader>
      <FieldGroup>
        {fixedProjectId === undefined ? (
          <Field>
            <FieldLabel htmlFor="proposal-project">Project</FieldLabel>
            <ProjectSelect id="proposal-project" value={projectId} onChange={setProjectId} />
          </Field>
        ) : null}
        <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_10rem]">
          <Field>
            <FieldLabel htmlFor="proposal-kind">Kind</FieldLabel>
            <ComboboxField
              id="proposal-kind"
              value={kind}
              onValueChange={(value) => setKind(value as ProposalKind)}
              options={KIND_OPTIONS}
              searchPlaceholder="Search kinds"
              emptyText="No kind matches."
            />
          </Field>
          <Field data-invalid={parsedBudget.error ? true : undefined}>
            <FieldLabel htmlFor="proposal-budget">Budget (optional)</FieldLabel>
            <InputGroup>
              <InputGroupAddon>
                <InputGroupText>$</InputGroupText>
              </InputGroupAddon>
              <InputGroupInput
                id="proposal-budget"
                type="number"
                inputMode="decimal"
                min={0}
                step={1}
                value={budget}
                onChange={(event) => setBudget(event.target.value)}
                placeholder="None"
                className="tabular-nums"
                aria-invalid={parsedBudget.error ? true : undefined}
                aria-describedby={parsedBudget.error ? 'proposal-budget-error' : 'proposal-budget-hint'}
              />
              <InputGroupAddon align="inline-end">
                <InputGroupText className="text-xs">/mo</InputGroupText>
              </InputGroupAddon>
            </InputGroup>
            {parsedBudget.error ? (
              <FieldError id="proposal-budget-error">{parsedBudget.error}</FieldError>
            ) : (
              <FieldDescription id="proposal-budget-hint">Options above it are kept but marked.</FieldDescription>
            )}
          </Field>
        </div>
        <Field>
          <FieldLabel htmlFor="proposal-prompt">What do you need? (optional)</FieldLabel>
          <Textarea
            id="proposal-prompt"
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            placeholder="e.g. private networking only, keep the current region"
            rows={3}
          />
          <FieldDescription>Constraints stated here are applied before the first ranking.</FieldDescription>
        </Field>
      </FieldGroup>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" loading={generate.isPending} disabled={!canSubmit}>
          Generate proposal
        </Button>
      </DialogFooter>
    </form>
  )
}
