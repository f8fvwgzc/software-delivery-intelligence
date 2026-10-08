import { PlayIcon } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

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
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

import { useCreateExecution } from '../../api/mutations'
import { PLAN_TEMPLATE, toPlanStep, validatePlan, type DraftStep } from '../../lib/plan-template'
import { PlanEditor } from './plan-editor'
import { ProjectSelect } from './project-select'

interface NewDryRunDialogProps {
  /** Preselects and locks the project. */
  projectId?: string
  /** Button styling for the trigger. */
  variant?: 'default' | 'outline'
  size?: 'default' | 'sm'
}

const EMPTY_ERRORS = new Map<number, string>()

/** Creates an execution against the simulated adapter from an editable plan, then opens it. */
export function NewDryRunDialog({ projectId, variant = 'default', size = 'default' }: NewDryRunDialogProps) {
  const [open, setOpen] = useState(false)

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant={variant} size={size}>
          <PlayIcon data-icon="inline-start" />
          New dry run
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>New dry run</DialogTitle>
          <DialogDescription>
            Runs the plan with the simulated adapter. Every step still passes permission, policy and approval checks.
          </DialogDescription>
        </DialogHeader>
        {/* Mounted per open, so the form starts from the template every time. */}
        <DryRunForm projectId={projectId} onCreated={() => setOpen(false)} />
      </DialogContent>
    </Dialog>
  )
}

interface DryRunFormProps {
  projectId?: string
  onCreated: () => void
}

function DryRunForm({ projectId: fixedProjectId, onCreated }: DryRunFormProps) {
  const navigate = useNavigate()
  const create = useCreateExecution()
  const [projectId, setProjectId] = useState(fixedProjectId ?? '')
  const [title, setTitle] = useState('Dry run: build and run container')
  const [steps, setSteps] = useState<DraftStep[]>(PLAN_TEMPLATE)
  const [submitted, setSubmitted] = useState(false)

  const stepErrors = submitted ? validatePlan(steps) : EMPTY_ERRORS
  const projectError = submitted && !projectId ? 'Choose a project.' : undefined
  const titleError = submitted && !title.trim() ? 'Give the run a title.' : undefined

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault()
    setSubmitted(true)
    if (!projectId || !title.trim() || validatePlan(steps).size > 0) return

    create.mutate(
      { projectId, title: title.trim(), steps: steps.map(toPlanStep) },
      {
        onSuccess: (execution) => {
          toast.success(`Execution #${execution.number} created`)
          onCreated()
          navigate(routes.execution(execution.id))
        },
      },
    )
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-4" noValidate>
      <FieldGroup className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
        <Field data-invalid={projectError ? true : undefined}>
          <FieldLabel htmlFor="dry-run-project">Project</FieldLabel>
          <ProjectSelect
            id="dry-run-project"
            value={projectId}
            onChange={setProjectId}
            disabled={Boolean(fixedProjectId)}
            invalid={Boolean(projectError)}
          />
          {projectError && <FieldError>{projectError}</FieldError>}
        </Field>
        <Field data-invalid={titleError ? true : undefined}>
          <FieldLabel htmlFor="dry-run-title">Title</FieldLabel>
          <Input
            id="dry-run-title"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            aria-invalid={titleError ? true : undefined}
          />
          {titleError && <FieldError>{titleError}</FieldError>}
        </Field>
      </FieldGroup>

      <Field>
        <FieldLabel>Plan</FieldLabel>
        <PlanEditor steps={steps} errors={stepErrors} onChange={setSteps} onReset={() => setSteps(PLAN_TEMPLATE)} />
      </Field>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCreated}>
          Cancel
        </Button>
        <Button type="submit" loading={create.isPending}>
          Create execution
        </Button>
      </DialogFooter>
    </form>
  )
}
