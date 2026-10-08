import { PlusIcon } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
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
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'

import { useGenerateProposal } from '../../api/mutations'
import { PROPOSAL_KIND_LABEL, PROPOSAL_KINDS, type ProposalKind } from '../../lib/labels'
import { ProjectSelect } from './project-select'

interface NewProposalDialogProps {
  /** When set, the project is fixed and the picker is hidden. */
  projectId?: string
}

/** "New proposal": pick a project and a kind, optionally describe the ask. */
export function NewProposalDialog({ projectId }: NewProposalDialogProps) {
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
        {/* Keyed on open so the form resets each time without an effect. */}
        {open ? <NewProposalForm key="form" projectId={projectId} onDone={() => setOpen(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

interface NewProposalFormProps {
  projectId?: string
  onDone: () => void
}

function NewProposalForm({ projectId: fixedProjectId, onDone }: NewProposalFormProps) {
  const navigate = useNavigate()
  const generate = useGenerateProposal()
  const [projectId, setProjectId] = useState(fixedProjectId ?? '')
  const [kind, setKind] = useState<ProposalKind>('deployment')
  const [prompt, setPrompt] = useState('')

  const canSubmit = projectId !== '' && !generate.isPending

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSubmit) return
    generate.mutate(
      { projectId, kind, prompt: prompt.trim() || null },
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
        <Field>
          <FieldLabel htmlFor="proposal-kind">Kind</FieldLabel>
          <Select value={kind} onValueChange={(value) => setKind(value as ProposalKind)}>
            <SelectTrigger id="proposal-kind" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {PROPOSAL_KINDS.map((option) => (
                  <SelectItem key={option} value={option}>
                    {PROPOSAL_KIND_LABEL[option]}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
        <Field>
          <FieldLabel htmlFor="proposal-prompt">What do you need? (optional)</FieldLabel>
          <Textarea
            id="proposal-prompt"
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            placeholder="e.g. keep it under $50 a month, private networking only"
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
