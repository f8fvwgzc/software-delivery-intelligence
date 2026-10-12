import { SparklesIcon } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { ComboboxField, type ComboboxOption } from '@/components/shared/form'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'

import { useGenerateDiagram } from '../../api/mutations'
import type { Diagram } from '../../api/queries'
import { DIAGRAM_KIND, DIAGRAM_KINDS, type DiagramKind } from '../../lib/labels'

interface GenerateFormProps {
  projectId: string
  onGenerated: (diagram: Diagram) => void
}

const KIND_OPTIONS: readonly ComboboxOption[] = DIAGRAM_KINDS.map((kind) => ({
  value: kind,
  label: DIAGRAM_KIND[kind].label,
  description: DIAGRAM_KIND[kind].description,
}))

/** Kind, an optional focus (required for custom, where pasted Mermaid is imported), and the one solid action. */
export function GenerateForm({ projectId, onGenerated }: GenerateFormProps) {
  const id = useId()
  const [kind, setKind] = useState<DiagramKind>('architecture')
  const [prompt, setPrompt] = useState('')
  const [touched, setTouched] = useState(false)
  const generate = useGenerateDiagram(projectId)

  const isCustom = kind === 'custom'
  const trimmed = prompt.trim()
  const missingPrompt = isCustom && trimmed.length === 0
  const meta = DIAGRAM_KIND[kind]

  const submit = (event: FormEvent) => {
    event.preventDefault()
    setTouched(true)
    if (missingPrompt) return
    generate.mutate(
      { kind, prompt: trimmed.length > 0 ? trimmed : null },
      {
        onSuccess: (diagram) => {
          toast.success(`${meta.label} diagram generated`, {
            description: `${diagram.spec?.nodes?.length ?? 0} nodes, ${diagram.spec?.edges?.length ?? 0} edges.`,
          })
          onGenerated(diagram)
        },
        onError: (error) => {
          toast.error('Could not generate the diagram', { description: error.message })
        },
      },
    )
  }

  return (
    <form
      onSubmit={submit}
      className="grid items-start gap-3 rounded-lg bg-card p-3 ring-1 ring-border sm:grid-cols-[13rem_minmax(0,1fr)_auto]"
      aria-label="Generate a diagram"
    >
      <Field className="gap-1.5">
        <FieldLabel htmlFor={`${id}-kind`} className="text-xs text-muted-foreground">
          Kind
        </FieldLabel>
        <ComboboxField
          id={`${id}-kind`}
          value={kind}
          onValueChange={(value) => setKind(value as DiagramKind)}
          options={KIND_OPTIONS}
          searchPlaceholder="Search kinds"
          disabled={generate.isPending}
          className="w-full"
        />
        <FieldDescription className="text-2xs">{meta.description}</FieldDescription>
      </Field>

      <Field className="gap-1.5" data-invalid={touched && missingPrompt ? true : undefined}>
        <FieldLabel htmlFor={`${id}-prompt`} className="text-xs text-muted-foreground">
          {isCustom ? 'Prompt or Mermaid' : 'Focus'}
          {isCustom ? null : <span className="font-normal text-muted-foreground">optional</span>}
        </FieldLabel>
        <Textarea
          id={`${id}-prompt`}
          value={prompt}
          onChange={(event) => setPrompt(event.target.value)}
          onBlur={() => setTouched(true)}
          rows={1}
          disabled={generate.isPending}
          aria-invalid={touched && missingPrompt ? true : undefined}
          aria-describedby={touched && missingPrompt ? `${id}-prompt-error` : `${id}-prompt-hint`}
          placeholder={
            isCustom
              ? 'Describe what to draw, or paste Mermaid source to import it'
              : 'A subsystem, directory or question to focus on'
          }
          className="max-h-40 min-h-8 py-1.5 text-sm"
        />
        {touched && missingPrompt ? (
          <FieldError id={`${id}-prompt-error`} className="text-2xs">
            A custom diagram needs a prompt or Mermaid source.
          </FieldError>
        ) : (
          <FieldDescription id={`${id}-prompt-hint`} className="text-2xs">
            {isCustom
              ? 'Pasted Mermaid is stored as is and becomes the authoritative source.'
              : 'Narrows what the generator reads; leave empty for the whole repository.'}
          </FieldDescription>
        )}
      </Field>

      <Button type="submit" loading={generate.isPending} className="sm:mt-[1.4rem]">
        <SparklesIcon data-icon="inline-start" />
        {generate.isPending ? 'Generating' : 'Generate'}
      </Button>
    </form>
  )
}
