import { useId, useState, type FormEvent } from 'react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { ComboboxField } from '@/components/shared/form/combobox-field'
import { InlineCode } from '@/components/system/code'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Switch } from '@/components/ui/switch'

import { useUpdateSettings } from '../../api/mutations'
import type { IntelligenceSettings, IntelligenceSettingsPatch } from '../../api/queries'
import { AUTO_MERGE_DESCRIPTION, AUTO_MERGE_LABEL, AUTO_MERGE_MODES, autoMergeMode, type AutoMergeMode } from '../../lib/settings'
import { WatcherStatus } from './watcher-status'

interface Draft {
  autoReview: boolean
  autoDraftFixes: boolean
  autoMerge: AutoMergeMode
  mergeRequiresReview: boolean
}

function toDraft(settings: IntelligenceSettings): Draft {
  return {
    autoReview: settings.autoReview,
    autoDraftFixes: settings.autoDraftFixes,
    autoMerge: autoMergeMode(settings.autoMerge),
    mergeRequiresReview: settings.mergeRequiresReview,
  }
}

/** Changed fields only; unset fields keep their value on the server. */
function toPatch(draft: Draft, settings: IntelligenceSettings): IntelligenceSettingsPatch {
  const patch: IntelligenceSettingsPatch = {}
  if (draft.autoReview !== settings.autoReview) patch.autoReview = draft.autoReview
  if (draft.autoDraftFixes !== settings.autoDraftFixes) patch.autoDraftFixes = draft.autoDraftFixes
  if (draft.autoMerge !== autoMergeMode(settings.autoMerge)) patch.autoMerge = draft.autoMerge
  if (draft.mergeRequiresReview !== settings.mergeRequiresReview) patch.mergeRequiresReview = draft.mergeRequiresReview
  return patch
}

const MERGE_OPTIONS = AUTO_MERGE_MODES.map((mode) => ({ value: mode, label: AUTO_MERGE_LABEL[mode], description: AUTO_MERGE_DESCRIPTION[mode] }))

/**
 * What SDI does to pull requests without being asked (CONTRACT.md §23):
 * review them, draft fixes with the model, ask for a merge. Each is its own
 * switch; the merge gate sits next to them. Saves like the other sections,
 * and under it the watcher says when it last looked.
 */
export function AutomationForm({ settings }: { settings: IntelligenceSettings }) {
  const id = useId()
  const [draft, setDraft] = useState<Draft>(() => toDraft(settings))
  const update = useUpdateSettings('Automation')

  const patch = toPatch(draft, settings)
  const dirty = Object.keys(patch).length > 0
  const set = <K extends keyof Draft>(key: K, value: Draft[K]) => setDraft((current) => ({ ...current, [key]: value }))

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!dirty) return
    update.mutate({ intelligence: patch })
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-4 rounded-lg p-4 ring-1 ring-border">
      <FieldGroup className="gap-4">
        <Field orientation="horizontal">
          <Switch id={`${id}-auto-review`} checked={draft.autoReview} onCheckedChange={(checked) => set('autoReview', checked)} />
          <div className="flex flex-col gap-0.5">
            <FieldLabel htmlFor={`${id}-auto-review`} className="font-normal">
              Review pull requests automatically
            </FieldLabel>
            <FieldDescription className="text-xs">
              Starts a review when a pull request is opened or pushed to. A review never publishes itself: it stops at the findings and
              waits for you.
            </FieldDescription>
          </div>
        </Field>

        <Field orientation="horizontal">
          <Switch id={`${id}-auto-draft`} checked={draft.autoDraftFixes} onCheckedChange={(checked) => set('autoDraftFixes', checked)} />
          <div className="flex flex-col gap-0.5">
            <FieldLabel htmlFor={`${id}-auto-draft`} className="font-normal">
              Draft fixes with the model
            </FieldLabel>
            <FieldDescription className="text-xs">
              During a review, asks the model for a fix where the rule has none, for at most 10 findings. Needs the reasoner and the
              “Source lines to the model” switch on <Link to={routes.dataPrivacy()}>Data &amp; privacy</Link>.
            </FieldDescription>
          </div>
        </Field>

        <Field>
          <FieldLabel htmlFor={`${id}-auto-merge`}>Merge on its own</FieldLabel>
          <ComboboxField
            id={`${id}-auto-merge`}
            className="sm:max-w-xs"
            value={draft.autoMerge}
            onValueChange={(value) => set('autoMerge', autoMergeMode(value))}
            options={MERGE_OPTIONS}
            searchPlaceholder="Search"
            emptyText="No option matches."
            aria-describedby={`${id}-auto-merge-hint`}
          />
          <FieldDescription id={`${id}-auto-merge-hint`} className="text-xs">
            Whose pull requests are merged without anyone asking, once nothing blocks them. The merge still waits for an approval unless
            the policy for <InlineCode>repo.merge_pull_request</InlineCode> is automatic, on <Link to={routes.policies()}>Policies</Link>.
          </FieldDescription>
        </Field>

        <Field orientation="horizontal">
          <Switch
            id={`${id}-requires-review`}
            checked={draft.mergeRequiresReview}
            onCheckedChange={(checked) => set('mergeRequiresReview', checked)}
          />
          <div className="flex flex-col gap-0.5">
            <FieldLabel htmlFor={`${id}-requires-review`} className="font-normal">
              Require a review before merging
            </FieldLabel>
            <FieldDescription className="text-xs">
              A merge needs a review of the current head with no undecided finding of severity high or critical. This holds for merges a
              person asks for too.
            </FieldDescription>
          </div>
        </Field>
      </FieldGroup>

      <div className="flex flex-wrap items-end justify-between gap-3">
        <WatcherStatus settings={settings} />
        <Button type="submit" size="sm" loading={update.isPending} disabled={!dirty} className="ml-auto">
          Save automation
        </Button>
      </div>
    </form>
  )
}
