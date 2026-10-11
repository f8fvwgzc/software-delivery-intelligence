import { PlusIcon } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

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
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'

import { useCreateDecision } from '../../api/mutations'
import { DECISION_SCOPE_DESCRIPTION, DECISION_SCOPE_LABEL, DECISION_SCOPES, type DecisionScope } from '../../lib/labels'
import { ProjectField } from './project-field'

const SCOPE_OPTIONS = DECISION_SCOPES.map((scope) => ({
  value: scope,
  label: DECISION_SCOPE_LABEL[scope],
  description: DECISION_SCOPE_DESCRIPTION[scope],
}))

/** "Record decision": a rule the ranker must honour from now on. */
export function RecordDecisionDialog() {
  const [open, setOpen] = useState(false)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="sm">
          <PlusIcon data-icon="inline-start" />
          Record decision
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        {open ? <RecordDecisionForm onDone={() => setOpen(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

function splitLines(value: string) {
  return value
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

function splitTags(value: string) {
  return value
    .split(',')
    .map((tag) => tag.trim().toLowerCase())
    .filter((tag) => tag.length > 0)
}

function RecordDecisionForm({ onDone }: { onDone: () => void }) {
  const create = useCreateDecision()
  const [scope, setScope] = useState<DecisionScope>('project')
  const [projectId, setProjectId] = useState('')
  const [title, setTitle] = useState('')
  const [statement, setStatement] = useState('')
  const [reasons, setReasons] = useState('')
  const [tags, setTags] = useState('')
  const [approvedBy, setApprovedBy] = useState('')

  const canSubmit = title.trim().length > 0 && statement.trim().length > 0 && !create.isPending

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSubmit) return
    create.mutate(
      {
        scope,
        projectId: scope === 'project' && projectId ? projectId : null,
        title: title.trim(),
        statement: statement.trim(),
        reasons: splitLines(reasons),
        tags: splitTags(tags),
        approvedBy: approvedBy.trim() || null,
      },
      {
        onSuccess: (decision) => {
          onDone()
          toast.success('Decision recorded', { description: decision.title })
        },
      },
    )
  }

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-5">
      <DialogHeader>
        <DialogTitle>Record decision</DialogTitle>
        <DialogDescription>
          Tags like <span className="font-mono">cloud:azure</span> or <span className="font-mono">iac:terraform</span>{' '}
          are what the ranker matches on.
        </DialogDescription>
      </DialogHeader>
      <FieldGroup>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="decision-scope">Scope</FieldLabel>
            <ComboboxField
              id="decision-scope"
              value={scope}
              onValueChange={(value) => setScope(value as DecisionScope)}
              options={SCOPE_OPTIONS}
              searchPlaceholder="Search scopes"
              emptyText="No scope matches."
            />
          </Field>
          {scope === 'project' ? (
            <ProjectField value={projectId} onChange={setProjectId} />
          ) : (
            <Field>
              <FieldLabel>Applies to</FieldLabel>
              <FieldDescription>Every project in this {DECISION_SCOPE_LABEL[scope].toLowerCase()}.</FieldDescription>
            </Field>
          )}
        </div>
        <Field>
          <FieldLabel htmlFor="decision-title">Title</FieldLabel>
          <Input
            id="decision-title"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="e.g. Standardise on Azure"
            required
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="decision-statement">Statement</FieldLabel>
          <Textarea
            id="decision-statement"
            value={statement}
            onChange={(event) => setStatement(event.target.value)}
            placeholder="The decision itself, in one or two sentences."
            rows={2}
            required
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="decision-reasons">Reasons</FieldLabel>
          <Textarea
            id="decision-reasons"
            value={reasons}
            onChange={(event) => setReasons(event.target.value)}
            placeholder={'One reason per line'}
            rows={3}
          />
        </Field>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="decision-tags">Tags</FieldLabel>
            <Input
              id="decision-tags"
              value={tags}
              onChange={(event) => setTags(event.target.value)}
              placeholder="cloud:azure, iac:terraform"
              className="font-mono"
            />
            <FieldDescription>Comma separated.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="decision-approved-by">Approved by</FieldLabel>
            <Input
              id="decision-approved-by"
              value={approvedBy}
              onChange={(event) => setApprovedBy(event.target.value)}
              placeholder="name or handle"
            />
          </Field>
        </div>
      </FieldGroup>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" loading={create.isPending} disabled={!canSubmit}>
          Record decision
        </Button>
      </DialogFooter>
    </form>
  )
}
