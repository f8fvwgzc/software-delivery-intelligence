import { ComboboxField, type ComboboxOption } from '@/components/shared/form'
import { Field, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import type { Project } from '@/features/projects/api/queries'

import type { SourceType } from '../../api/types'
import { applicationControlId, type ApplicationDraft } from '../../lib/application-form'
import { DETECT_SOURCE, SOURCE_TYPE } from '../../lib/labels'
import { controlProps, FieldMessage, Optional, SwitchField } from '../form-parts'

/** The combobox cannot carry ''; this value stands for "Detect". */
const DETECT = '__detect__'

const TYPE_OPTIONS: ComboboxOption[] = [
  { value: DETECT, ...DETECT_SOURCE },
  ...(Object.keys(SOURCE_TYPE) as SourceType[]).map((type) => ({ value: type, ...SOURCE_TYPE[type] })),
]

const MONO = 'font-mono text-xs md:text-xs'

interface SourceFieldsProps {
  draft: ApplicationDraft
  onChange: (patch: Partial<ApplicationDraft>) => void
  errors: Record<string, string>
  /** The project's repository and default branch, the defaults of an empty source. */
  project?: Pick<Project, 'repoFullName' | 'defaultBranch'>
  githubHost: string
}

/**
 * Where the desired state comes from (Argo CD's `spec.source`): the
 * repository, the revision and the path, and how it renders. A type shows
 * only its own options; "Detect" shows all three groups, since any of them
 * may apply once the type is known.
 */
export function SourceFields({ draft, onChange, errors, project, githubHost }: SourceFieldsProps) {
  const id = applicationControlId
  const text = (field: keyof ApplicationDraft, hint?: string) => ({
    ...controlProps(id(field), errors[field], Boolean(hint)),
    value: String(draft[field]),
    autoComplete: 'off',
    spellCheck: false,
  })
  const detect = draft.type === ''
  const shows = (type: SourceType) => detect || draft.type === type
  const typeHint = detect ? `${DETECT_SOURCE.description}.` : `${SOURCE_TYPE[draft.type as SourceType].description}.`

  return (
    <FieldSet className="gap-4">
      <FieldLegend variant="label">Source</FieldLegend>
      <FieldGroup className="grid gap-4 sm:grid-cols-2">
        <Field data-invalid={errors.repo ? true : undefined}>
          <FieldLabel htmlFor={id('repo')}>
            Repository <Optional />
          </FieldLabel>
          <Input
            {...text('repo', 'hint')}
            onChange={(event) => onChange({ repo: event.target.value })}
            placeholder={project?.repoFullName ?? 'owner/name'}
            className={MONO}
          />
          <FieldMessage id={id('repo')} error={errors.repo} hint={`owner/name on ${githubHost || 'GitHub'}. Empty is this project’s repository.`} />
        </Field>
        <Field data-invalid={errors.revision ? true : undefined}>
          <FieldLabel htmlFor={id('revision')}>
            Revision <Optional />
          </FieldLabel>
          <Input
            {...text('revision', 'hint')}
            onChange={(event) => onChange({ revision: event.target.value })}
            placeholder={project?.defaultBranch ?? 'main'}
            className={MONO}
          />
          <FieldMessage id={id('revision')} error={errors.revision} hint="Branch, tag or commit SHA. Empty follows the default branch." />
        </Field>
        <Field data-invalid={errors.path ? true : undefined}>
          <FieldLabel htmlFor={id('path')}>
            Path <Optional />
          </FieldLabel>
          <Input
            {...text('path', 'hint')}
            onChange={(event) => onChange({ path: event.target.value })}
            placeholder="deploy/production"
            className={MONO}
          />
          <FieldMessage id={id('path')} error={errors.path} hint="Directory inside the repository. Empty is its root." />
        </Field>
        <Field data-invalid={errors.type ? true : undefined}>
          <FieldLabel htmlFor={id('type')}>Type</FieldLabel>
          <ComboboxField
            {...controlProps(id('type'), errors.type, true)}
            value={draft.type || DETECT}
            onValueChange={(value) => onChange({ type: value === DETECT ? '' : (value as SourceType) })}
            options={TYPE_OPTIONS}
            searchPlaceholder="Search types"
          />
          <FieldMessage id={id('type')} error={errors.type} hint={typeHint} />
        </Field>
      </FieldGroup>

      {shows('directory') ? (
        <SwitchField
          id={id('recurse')}
          label={detect ? 'Directory: include subdirectories' : 'Include subdirectories'}
          description="Read the manifests in every folder under the path, not only the path itself."
          checked={draft.recurse}
          onCheckedChange={(recurse) => onChange({ recurse })}
        />
      ) : null}

      {shows('kustomize') ? (
        <Field data-invalid={errors.kustomizeImages ? true : undefined}>
          <FieldLabel htmlFor={id('kustomizeImages')}>
            {detect ? 'Kustomize: image overrides' : 'Image overrides'} <Optional />
          </FieldLabel>
          <Textarea
            {...text('kustomizeImages', 'hint')}
            onChange={(event) => onChange({ kustomizeImages: event.target.value })}
            placeholder={'ghcr.io/acme/web=ghcr.io/acme/web:1.4.2'}
            rows={2}
            className={MONO}
          />
          <FieldMessage
            id={id('kustomizeImages')}
            error={errors.kustomizeImages}
            hint="One per line: name=newName:tag, name:tag or name@sha256:digest. A promotion through the pipeline sets these."
          />
        </Field>
      ) : null}

      {shows('helm') ? (
        <FieldGroup className="grid gap-4 sm:grid-cols-2">
          <Field data-invalid={errors.helmReleaseName ? true : undefined}>
            <FieldLabel htmlFor={id('helmReleaseName')}>
              {detect ? 'Helm: release name' : 'Release name'} <Optional />
            </FieldLabel>
            <Input
              {...text('helmReleaseName', 'hint')}
              onChange={(event) => onChange({ helmReleaseName: event.target.value })}
              placeholder={draft.name.trim() || 'the application’s name'}
              className={MONO}
            />
            <FieldMessage id={id('helmReleaseName')} error={errors.helmReleaseName} hint="Empty uses the application’s name." />
          </Field>
          <Field data-invalid={errors.helmValueFiles ? true : undefined}>
            <FieldLabel htmlFor={id('helmValueFiles')}>
              {detect ? 'Helm: values files' : 'Values files'} <Optional />
            </FieldLabel>
            <Textarea
              {...text('helmValueFiles', 'hint')}
              onChange={(event) => onChange({ helmValueFiles: event.target.value })}
              placeholder={'values.yaml\nvalues-production.yaml'}
              rows={2}
              className={MONO}
            />
            <FieldMessage id={id('helmValueFiles')} error={errors.helmValueFiles} hint="Relative to the path, applied in order, one per line." />
          </Field>
          <Field data-invalid={errors.helmParameters ? true : undefined} className="sm:col-span-2">
            <FieldLabel htmlFor={id('helmParameters')}>
              {detect ? 'Helm: parameters' : 'Parameters'} <Optional />
            </FieldLabel>
            <Textarea
              {...text('helmParameters', 'hint')}
              onChange={(event) => onChange({ helmParameters: event.target.value })}
              placeholder={'image.tag=1.4.2\nreplicaCount=3'}
              rows={2}
              className={MONO}
            />
            <FieldMessage
              id={id('helmParameters')}
              error={errors.helmParameters}
              hint="key=value, one per line, applied after the values files like --set. A promotion sets image.repository and image.tag."
            />
          </Field>
        </FieldGroup>
      ) : null}
    </FieldSet>
  )
}
