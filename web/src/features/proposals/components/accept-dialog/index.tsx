import { CheckIcon } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
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
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { components } from '@/lib/api/schema'

import { useAcceptProposal } from '../../api/mutations'
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
  const [optionId, setOptionId] = useState(initialOptionId)
  const [createExecution, setCreateExecution] = useState(true)

  const ranked = proposal.options.toSorted((a, b) => a.rank - b.rank)

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    accept.mutate(
      { optionId, createExecution },
      {
        onSuccess: (accepted) => {
          onDone()
          const executionId = accepted.executionId
          if (executionId) {
            toast.success('Proposal accepted', {
              description: 'An execution was created from the chosen plan.',
              action: { label: 'Open execution', onClick: () => navigate(routes.execution(executionId)) },
            })
          } else {
            toast.success('Proposal accepted')
          }
        },
      },
    )
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
          <Select value={optionId} onValueChange={setOptionId}>
            <SelectTrigger id="accept-option" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {ranked.map((option) => (
                  <SelectItem key={option.id} value={option.id}>
                    <span className="font-mono tabular-nums text-muted-foreground">{option.rank}</span>
                    <span className="truncate">{option.title}</span>
                    <span className="tabular-nums text-muted-foreground">{formatPercent(option.score?.overall ?? 0)}</span>
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
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
      </FieldGroup>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" loading={accept.isPending}>
          Accept option
        </Button>
      </DialogFooter>
    </form>
  )
}
