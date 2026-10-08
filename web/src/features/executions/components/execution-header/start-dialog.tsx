import { PlayIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

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
import { Switch } from '@/components/ui/switch'
import type { components } from '@/lib/api/schema'

import { useStartExecution } from '../../api/mutations'
import { FAILURE_CODES } from '../../lib/labels'

interface StartDialogProps {
  execution: components['schemas']['Execution']
}

/** Starts the run, optionally with one injected fault so the failure path can be rehearsed. */
export function StartDialog({ execution }: StartDialogProps) {
  const [open, setOpen] = useState(false)

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <PlayIcon data-icon="inline-start" />
          Start
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Start execution #{execution.number}</DialogTitle>
          <DialogDescription>
            Runs the policy check first. Steps that need approval wait for you; the rest run one at a time.
          </DialogDescription>
        </DialogHeader>
        <StartForm execution={execution} onStarted={() => setOpen(false)} />
      </DialogContent>
    </Dialog>
  )
}

interface StartFormProps {
  execution: components['schemas']['Execution']
  onStarted: () => void
}

function StartForm({ execution, onStarted }: StartFormProps) {
  const start = useStartExecution(execution.id)
  const [inject, setInject] = useState(false)
  const [stepKey, setStepKey] = useState(execution.steps[execution.steps.length - 1]?.key ?? '')
  const [code, setCode] = useState<string>('PERMISSION_DENIED')
  const isDryRun = execution.adapter === 'simulated'

  const submit = () => {
    start.mutate(inject && stepKey ? { stepKey, code } : null, {
      onSuccess: () => {
        toast.success(`Execution #${execution.number} started`)
        onStarted()
      },
    })
  }

  return (
    <div className="flex flex-col gap-4">
      {isDryRun && (
        <FieldGroup className="gap-3">
          <Field orientation="horizontal">
            <Switch id="inject-fault" checked={inject} onCheckedChange={setInject} />
            <div className="flex flex-col gap-0.5">
              <FieldLabel htmlFor="inject-fault">Inject a fault</FieldLabel>
              <FieldDescription>Make one step fail on purpose to rehearse diagnosis and remediation.</FieldDescription>
            </div>
          </Field>
          {inject && (
            <div className="grid gap-3 sm:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="fault-step">Step</FieldLabel>
                <Select value={stepKey} onValueChange={setStepKey}>
                  <SelectTrigger id="fault-step" className="w-full">
                    <SelectValue placeholder="Choose a step" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {execution.steps.map((step) => (
                        <SelectItem key={step.key} value={step.key}>
                          <span className="font-mono text-xs text-muted-foreground">{step.key}</span>
                          <span className="truncate">{step.title}</span>
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
              <Field>
                <FieldLabel htmlFor="fault-code">Failure code</FieldLabel>
                <Select value={code} onValueChange={setCode}>
                  <SelectTrigger id="fault-code" className="w-full font-mono text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {FAILURE_CODES.map((failureCode) => (
                        <SelectItem key={failureCode} value={failureCode} className="font-mono text-xs">
                          {failureCode}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
            </div>
          )}
        </FieldGroup>
      )}
      <DialogFooter>
        <Button variant="outline" onClick={onStarted} disabled={start.isPending}>
          Not now
        </Button>
        <Button onClick={submit} loading={start.isPending}>
          <PlayIcon data-icon="inline-start" />
          {inject ? 'Start with fault' : 'Start'}
        </Button>
      </DialogFooter>
    </div>
  )
}
