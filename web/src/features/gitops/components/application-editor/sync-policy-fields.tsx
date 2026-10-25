import { Field, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

import { applicationControlId, type ApplicationDraft } from '../../lib/application-form'
import { controlProps, FieldMessage, Optional, SwitchField } from '../form-parts'

interface SyncPolicyFieldsProps {
  draft: ApplicationDraft
  onChange: (patch: Partial<ApplicationDraft>) => void
  errors: Record<string, string>
}

/**
 * When the application syncs without an execution. Automated sync is an
 * admin's explicit choice: it is the only way anything reaches a cluster
 * without an approval (CONTRACT.md §24.6), and its switch says so. Self heal
 * and prune refine it, so they are off while it is.
 */
export function SyncPolicyFields({ draft, onChange, errors }: SyncPolicyFieldsProps) {
  const id = applicationControlId
  const manual = !draft.automated

  return (
    <FieldSet className="gap-4">
      <FieldLegend variant="label">Sync policy</FieldLegend>
      <FieldGroup className="gap-4">
        <SwitchField
          id={id('automated')}
          label="Automated sync"
          description="Syncs on its own, as the system user, whenever the repository has a new state. It is the only way anything reaches the cluster without an approval."
          checked={draft.automated}
          onCheckedChange={(automated) => onChange({ automated })}
        />
        <SwitchField
          id={id('selfHeal')}
          label="Self heal"
          description="Automated syncs also undo changes made to the cluster by hand, not only new commits."
          checked={draft.selfHeal}
          onCheckedChange={(selfHeal) => onChange({ selfHeal })}
          disabled={manual}
        />
        <SwitchField
          id={id('prune')}
          label="Prune"
          description="Automated syncs also delete the resources that left the repository. A manual sync asks each time."
          checked={draft.prune}
          onCheckedChange={(prune) => onChange({ prune })}
          disabled={manual}
        />
        <SwitchField
          id={id('createNamespace')}
          label="Create namespace"
          description="Any sync creates the destination namespace when it does not exist yet."
          checked={draft.createNamespace}
          onCheckedChange={(createNamespace) => onChange({ createNamespace })}
        />
        {draft.automated ? (
          <Field data-invalid={errors.retryLimit ? true : undefined} className="max-w-56">
            <FieldLabel htmlFor={id('retryLimit')}>
              Retries <Optional />
            </FieldLabel>
            <Input
              {...controlProps(id('retryLimit'), errors.retryLimit, true)}
              value={draft.retryLimit}
              onChange={(event) => onChange({ retryLimit: event.target.value.replace(/\D/g, '').slice(0, 2) })}
              inputMode="numeric"
              placeholder="3"
              autoComplete="off"
              className="font-mono text-xs tabular-nums md:text-xs"
            />
            <FieldMessage
              id={id('retryLimit')}
              error={errors.retryLimit}
              hint="Attempts of a failed automated sync before it waits for a new commit."
            />
          </Field>
        ) : null}
      </FieldGroup>
    </FieldSet>
  )
}
