import { ComboboxField } from '@/components/shared/form'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { formatNumber } from '@/lib/intl'

import {
  baseUrlError,
  OPENAI_COMPATIBLE_EXAMPLES,
  parseTokenCap,
  REASONER_PROVIDER_DESCRIPTION,
  REASONER_PROVIDER_LABEL,
  REASONER_PROVIDERS,
} from '../../lib/settings'

const PROVIDER_OPTIONS = REASONER_PROVIDERS.map((provider) => ({
  value: provider,
  label: REASONER_PROVIDER_LABEL[provider],
  description: REASONER_PROVIDER_DESCRIPTION[provider],
}))

export interface ReasonerDraft {
  reasonerProvider: string
  reasonerBaseUrl: string
  reasonerModel: string
  reasonerDailyTokenCap: string
}

interface ReasonerFieldsProps {
  id: string
  draft: ReasonerDraft
  onChange: (patch: Partial<ReasonerDraft>) => void
  /** Tokens spent today, when the stats query answered; shown next to the cap. */
  tokensToday?: number
}

/**
 * Where the reasoner's model comes from: the provider, its base URL (only an
 * OpenAI-compatible server needs one), the model name and the daily token cap.
 */
export function ReasonerFields({ id, draft, onChange, tokensToday }: ReasonerFieldsProps) {
  const urlError = baseUrlError(draft.reasonerProvider, draft.reasonerBaseUrl)
  const modelError = draft.reasonerModel.trim() === '' ? 'Enter the model name the provider expects.' : null
  const cap = parseTokenCap(draft.reasonerDailyTokenCap)
  const capError = cap === null ? 'A whole number of tokens, or empty for no cap.' : null
  const compatible = draft.reasonerProvider === 'openai_compatible'

  return (
    <FieldSet>
      <FieldLegend variant="label">Reasoner model</FieldLegend>
      <FieldDescription className="text-xs">Which server answers negotiation and review prompts, and how much it may spend per day.</FieldDescription>
      <FieldGroup className="gap-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor={`${id}-provider`}>Provider</FieldLabel>
            <ComboboxField
              id={`${id}-provider`}
              value={draft.reasonerProvider}
              onValueChange={(value) => onChange({ reasonerProvider: value })}
              options={PROVIDER_OPTIONS}
              searchPlaceholder="Search providers"
              emptyText="No provider matches."
            />
            <FieldDescription className="text-xs">
              {REASONER_PROVIDER_DESCRIPTION[compatible ? 'openai_compatible' : 'anthropic']}
            </FieldDescription>
          </Field>
          <Field data-invalid={modelError ? true : undefined}>
            <FieldLabel htmlFor={`${id}-reasoner-model`}>Model</FieldLabel>
            <Input
              id={`${id}-reasoner-model`}
              value={draft.reasonerModel}
              onChange={(event) => onChange({ reasonerModel: event.target.value })}
              spellCheck={false}
              autoComplete="off"
              placeholder={compatible ? 'gpt-4o-mini, llama3.1, …' : 'claude-sonnet-4-5'}
              aria-invalid={modelError ? true : undefined}
              aria-describedby={modelError ? `${id}-reasoner-model-error` : undefined}
              className="font-mono"
            />
            {modelError ? <FieldError id={`${id}-reasoner-model-error`}>{modelError}</FieldError> : <FieldDescription className="text-xs">Sent to the provider as is.</FieldDescription>}
          </Field>
        </div>

        {compatible ? (
          <Field data-invalid={urlError ? true : undefined}>
            <FieldLabel htmlFor={`${id}-base-url`}>Base URL</FieldLabel>
            <Input
              id={`${id}-base-url`}
              type="url"
              value={draft.reasonerBaseUrl}
              onChange={(event) => onChange({ reasonerBaseUrl: event.target.value })}
              spellCheck={false}
              autoComplete="off"
              inputMode="url"
              placeholder={OPENAI_COMPATIBLE_EXAMPLES[0].url}
              aria-invalid={urlError ? true : undefined}
              aria-describedby={urlError ? `${id}-base-url-error` : `${id}-base-url-hint`}
              className="font-mono"
            />
            {urlError ? <FieldError id={`${id}-base-url-error`}>{urlError}</FieldError> : null}
            <FieldDescription id={`${id}-base-url-hint`} className="text-xs">
              The `/v1` root the chat completions endpoint hangs off. Typical values:
              <span className="mt-1 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5">
                {OPENAI_COMPATIBLE_EXAMPLES.map((example) => (
                  <span key={example.name} className="contents">
                    <span>{example.name}</span>
                    <code className="truncate font-mono">{example.url}</code>
                  </span>
                ))}
              </span>
            </FieldDescription>
          </Field>
        ) : null}

        <Field data-invalid={capError ? true : undefined} className="sm:max-w-xs">
          <FieldLabel htmlFor={`${id}-token-cap`}>Daily token cap</FieldLabel>
          <Input
            id={`${id}-token-cap`}
            type="number"
            inputMode="numeric"
            min={0}
            step={1000}
            value={draft.reasonerDailyTokenCap}
            onChange={(event) => onChange({ reasonerDailyTokenCap: event.target.value })}
            placeholder="No cap"
            aria-invalid={capError ? true : undefined}
            aria-describedby={capError ? `${id}-token-cap-error` : `${id}-token-cap-hint`}
            className="tabular-nums"
          />
          {capError ? (
            <FieldError id={`${id}-token-cap-error`}>{capError}</FieldError>
          ) : (
            <FieldDescription id={`${id}-token-cap-hint`} className="text-xs tabular-nums">
              {tokensToday !== undefined ? `${formatNumber(tokensToday)} spent today. ` : ''}
              Once reached, the reasoner answers heuristically until midnight UTC; empty or 0 means no cap.
            </FieldDescription>
          )}
        </Field>
      </FieldGroup>
    </FieldSet>
  )
}
