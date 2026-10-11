import { CheckIcon } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { ComboboxField } from '@/components/shared/form'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
  FieldTitle,
} from '@/components/ui/field'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import type { components } from '@/lib/api/schema'

import { useAcceptProposal, useOpenPullRequest } from '../../api/mutations'
import { ADAPTER_CHOICES, type Adapter } from '../../lib/adapters'
import { formatPercent } from '../../lib/format'

type Proposal = components['schemas']['Proposal']

interface AcceptDialogProps {
  proposal: Proposal
  /** The option in focus on the page; pre-selected here. */
  selectedOptionId: string
}

export function AcceptDialog({ proposal, selectedOptionId }: AcceptDialogProps) {
  const [open, setOpen] = useState(false)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="sm">
          <CheckIcon data-icon="inline-start" />
          Accept
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        {open ? (
          <AcceptForm proposal={proposal} initialOptionId={selectedOptionId} onDone={() => setOpen(false)} />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

interface AcceptFormProps {
  proposal: Proposal
  initialOptionId: string
  onDone: () => void
}

function AcceptForm({ proposal, initialOptionId, onDone }: AcceptFormProps) {
  const navigate = useNavigate()
  const accept = useAcceptProposal(proposal.id)
  const openPullRequest = useOpenPullRequest(proposal.id)
  const [optionId, setOptionId] = useState(initialOptionId)
  const [createExecution, setCreateExecution] = useState(true)
  const [adapter, setAdapter] = useState<Adapter>('simulated')

  const ranked = proposal.options.toSorted((a, b) => a.rank - b.rank)
  const chosen = proposal.options.find((option) => option.id === optionId)
  const pending = accept.isPending || openPullRequest.isPending
  const viaPullRequest = createExecution && adapter === 'github'
  const noFiles = viaPullRequest && (chosen?.artifactCount ?? 0) === 0

  const onAccepted = (accepted: Proposal) => {
    onDone()
    const executionId = accepted.executionId
    if (executionId) {
      toast.success('Proposal accepted', {
        description: viaPullRequest
          ? 'An execution was created that opens the pull request.'
          : 'An execution was created from the chosen plan.',
        action: { label: 'Open execution', onClick: () => navigate(routes.execution(executionId)) },
      })
    } else {
      toast.success('Proposal accepted')
    }
  }

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (pending || noFiles) return
    if (viaPullRequest) {
      openPullRequest.mutate({ optionId }, { onSuccess: onAccepted })
    } else {
      accept.mutate(
        { optionId, createExecution, adapter: createExecution ? adapter : null },
        { onSuccess: onAccepted },
      )
    }
  }

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-5">
      <DialogHeader>
        <DialogTitle>Accept proposal</DialogTitle>
        <DialogDescription>
          Accepting records a decision for this project. The plan runs only when you start the execution.
        </DialogDescription>
      </DialogHeader>
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="accept-option">Option</FieldLabel>
          <ComboboxField
            id="accept-option"
            value={optionId}
            onValueChange={setOptionId}
            options={ranked.map((option) => ({
              value: option.id,
              label: `${option.rank}. ${option.title}`,
              description: `${formatPercent(option.score?.overall ?? 0)} overall`,
            }))}
            searchPlaceholder="Search options"
            emptyText="No option matches."
          />
          {optionId === proposal.recommendedOptionId ? (
            <FieldDescription>This is the recommended option.</FieldDescription>
          ) : (
            <FieldDescription>You are choosing over the recommendation.</FieldDescription>
          )}
        </Field>
        <Field orientation="horizontal">
          <Checkbox
            id="accept-execute"
            checked={createExecution}
            onCheckedChange={(checked) => setCreateExecution(checked === true)}
          />
          <FieldLabel htmlFor="accept-execute" className="font-normal">
            Create execution now
          </FieldLabel>
        </Field>
        {createExecution ? (
          <FieldSet>
            <FieldLegend variant="label">Run with</FieldLegend>
            <RadioGroup value={adapter} onValueChange={(value) => setAdapter(value as Adapter)} className="gap-1.5">
              {ADAPTER_CHOICES.map((choice) => (
                <FieldLabel key={choice.value} htmlFor={`accept-adapter-${choice.value}`}>
                  <Field orientation="horizontal">
                    <RadioGroupItem id={`accept-adapter-${choice.value}`} value={choice.value} />
                    <FieldContent>
                      <FieldTitle>{choice.label}</FieldTitle>
                      <FieldDescription className="text-xs">{choice.description}</FieldDescription>
                    </FieldContent>
                  </Field>
                </FieldLabel>
              ))}
            </RadioGroup>
            {viaPullRequest ? (
              <FieldDescription className={noFiles ? 'text-warning' : undefined}>
                {noFiles
                  ? 'This option has no generated files, so there is nothing to commit. Pick another runtime.'
                  : `Commits ${chosen?.artifactCount ?? 0} generated ${chosen?.artifactCount === 1 ? 'file' : 'files'} to a new branch.`}
              </FieldDescription>
            ) : null}
          </FieldSet>
        ) : null}
      </FieldGroup>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={pending}>
          Cancel
        </Button>
        <Button type="submit" loading={pending} disabled={noFiles}>
          {viaPullRequest ? 'Accept and open pull request' : 'Accept option'}
        </Button>
      </DialogFooter>
    </form>
  )
}
