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
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

import { useCreateExecution } from '../../api/mutations'
import { ADAPTER_CHOICES, unsupportedActions, type Adapter } from '../../lib/labels'
import { PLAN_TEMPLATE, toPlanStep, validatePlan, type DraftStep } from '../../lib/plan-template'
import { AdapterSelect } from './adapter-select'
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

/** Creates an execution from an editable plan on the chosen runtime, then opens it. */
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
            Runs the plan on the runtime you pick. Every step still passes permission, policy and approval checks.
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
  const [adapter, setAdapter] = useState<Adapter>('simulated')
  const [steps, setSteps] = useState<DraftStep[]>(PLAN_TEMPLATE)
  const [submitted, setSubmitted] = useState(false)

  const stepErrors = submitted ? validatePlan(steps) : EMPTY_ERRORS
  const projectError = submitted && !projectId ? 'Choose a project.' : undefined
  const titleError = submitted && !title.trim() ? 'Give the run a title.' : undefined

  // CONTRACT.md §5.4: the service refuses a run whose steps need a tool the
  // runtime cannot provide, so the same check gates Create here.
  const unsupported = unsupportedActions(adapter, steps)
  const adapterChoice = ADAPTER_CHOICES.find((choice) => choice.value === adapter)
  const runtimeError =
    unsupported.length > 0
      ? `${adapterChoice?.label ?? adapter} cannot run ${unsupported.join(', ')}. Change the runtime or remove those steps.`
      : undefined

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault()
    setSubmitted(true)
    // The first invalid control takes focus so the error that just appeared
    // next to it is read out, instead of a silent refusal of the submit.
    const firstInvalid = !projectId ? 'dry-run-project' : !title.trim() ? 'dry-run-title' : runtimeError ? 'dry-run-adapter' : null
    if (firstInvalid) {
      document.getElementById(firstInvalid)?.focus()
      return
    }
    if (validatePlan(steps).size > 0) return

    create.mutate(
      { projectId, title: title.trim(), adapter, steps: steps.map(toPlanStep) },
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
      <FieldGroup className="grid gap-3 sm:grid-cols-2">
        <Field data-invalid={projectError ? true : undefined}>
          <FieldLabel htmlFor="dry-run-project">Project</FieldLabel>
          <ProjectSelect
            id="dry-run-project"
            value={projectId}
            onChange={setProjectId}
            disabled={Boolean(fixedProjectId)}
            invalid={Boolean(projectError)}
            describedBy={projectError ? 'dry-run-project-error' : undefined}
          />
          {projectError && <FieldError id="dry-run-project-error">{projectError}</FieldError>}
        </Field>
        <Field data-invalid={titleError ? true : undefined}>
          <FieldLabel htmlFor="dry-run-title">Title</FieldLabel>
          <Input
            id="dry-run-title"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            aria-invalid={titleError ? true : undefined}
            aria-describedby={titleError ? 'dry-run-title-error' : undefined}
          />
          {titleError && <FieldError id="dry-run-title-error">{titleError}</FieldError>}
        </Field>
        <Field className="sm:col-span-2" data-invalid={runtimeError ? true : undefined}>
          <FieldLabel htmlFor="dry-run-adapter">Runtime</FieldLabel>
          <AdapterSelect
            id="dry-run-adapter"
            value={adapter}
            onChange={setAdapter}
            invalid={Boolean(runtimeError)}
            describedBy={runtimeError ? 'dry-run-adapter-error' : 'dry-run-adapter-hint'}
          />
          {runtimeError ? (
            <FieldError id="dry-run-adapter-error">{runtimeError}</FieldError>
          ) : (
            <FieldDescription id="dry-run-adapter-hint">{adapterChoice?.description}</FieldDescription>
          )}
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
        <Button type="submit" loading={create.isPending} disabled={Boolean(runtimeError)}>
          Create execution
        </Button>
      </DialogFooter>
    </form>
  )
}
