import { AlertCircleIcon, RotateCcwIcon, SaveIcon } from 'lucide-react'
import { useState, type KeyboardEvent } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'
import { cn } from '@/lib/utils'

import { CANVAS_HEIGHT_CLASS } from '../canvas-skeleton'
import { MermaidPreview } from './mermaid-preview'
import { useDebouncedValue } from './use-debounced-value'

export interface MermaidEditorProps {
  /** The stored source. Remount (key) when it changes so the draft follows. */
  source: string
  /** `source === "mermaid"`: the Mermaid is authoritative and the spec is stale. */
  authoritative: boolean
  onSave: (mermaid: string) => void
  saving: boolean
}

const PREVIEW_DELAY_MS = 300

/**
 * Two panes: the source on the left, the live preview on the right,
 * re-rendered 300 ms after typing stops. Parse errors appear under the
 * source. This module is a lazy chunk; it is the only one that loads
 * `mermaid`.
 */
export default function MermaidEditor({ source, authoritative, onSave, saving }: MermaidEditorProps) {
  const [draft, setDraft] = useState(source)
  const [error, setError] = useState<string | null>(null)
  const debounced = useDebouncedValue(draft, PREVIEW_DELAY_MS)
  const dirty = draft !== source
  const canSave = dirty && error === null && draft.trim().length > 0 && !saving

  const save = () => {
    if (canSave) onSave(draft)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 's') {
      event.preventDefault()
      save()
    }
  }

  return (
    <div className="flex flex-col gap-3">
      {authoritative ? (
        <Alert>
          <AlertCircleIcon />
          <AlertTitle>Mermaid source is authoritative</AlertTitle>
          <AlertDescription>
            This diagram was last edited here, so the editorial view shows the last generated spec. Regenerate to
            rebuild both from the repository.
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          {dirty ? 'Unsaved changes. Saving makes this source authoritative.' : 'Edits preview as you type.'}
        </p>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" disabled={!dirty || saving} onClick={() => setDraft(source)}>
            <RotateCcwIcon data-icon="inline-start" />
            Reset
          </Button>
          <Button size="sm" loading={saving} disabled={!canSave} onClick={save}>
            <SaveIcon data-icon="inline-start" />
            Save
          </Button>
        </div>
      </div>

      <div className={cn('grid gap-3 lg:grid-cols-2', CANVAS_HEIGHT_CLASS)}>
        <Field className="min-h-0 gap-1.5" data-invalid={error !== null || undefined}>
          <FieldLabel htmlFor="mermaid-source" className="text-xs text-muted-foreground">
            Source
          </FieldLabel>
          <Textarea
            id="mermaid-source"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onKeyDown}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            aria-invalid={error !== null || undefined}
            aria-describedby={error ? 'mermaid-source-error' : 'mermaid-source-hint'}
            className="min-h-0 flex-1 field-sizing-fixed resize-none font-mono text-xs leading-5 whitespace-pre"
            wrap="off"
          />
          {error ? (
            <FieldError id="mermaid-source-error" className="font-mono text-2xs whitespace-pre-wrap">
              {error}
            </FieldError>
          ) : (
            <FieldDescription id="mermaid-source-hint" className="text-2xs">
              Flowchart, sequence, class and ER syntax are supported. ⌘S saves.
            </FieldDescription>
          )}
        </Field>
        <div className="flex min-h-0 flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">Preview</span>
          <MermaidPreview
            source={debounced}
            onError={setError}
            className={cn('min-h-0 flex-1 rounded-lg bg-card ring-1 ring-border', error && 'ring-destructive/40')}
          />
        </div>
      </div>
    </div>
  )
}
