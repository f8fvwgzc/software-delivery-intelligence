import { ArrowRightIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { ComboboxField, type ComboboxOption } from '@/components/shared/form'
import { Field, FieldLabel } from '@/components/ui/field'

import type { Cluster } from '../../api/types'
import { clusterStatus } from '../../lib/labels'
import { controlProps, FieldMessage } from '../form-parts'

interface ClusterFieldProps {
  id: string
  value: string
  onValueChange: (clusterId: string) => void
  clusters: readonly Cluster[]
  loading: boolean
  /** The clusters could not be read: the field says so instead of looking empty. */
  failed: boolean
  error?: string
}

/** The registered cluster an application deploys to, with the way to register one. */
export function ClusterField({ id, value, onValueChange, clusters, loading, failed, error }: ClusterFieldProps) {
  const options: ComboboxOption[] = clusters.map((cluster) => ({
    value: cluster.id,
    label: cluster.name,
    description: [clusterStatus(cluster.status).label, cluster.server, cluster.serverVersion].filter(Boolean).join(' · '),
    keywords: [cluster.server ?? '', cluster.defaultNamespace ?? ''],
  }))
  // A cluster deleted since keeps the application's choice visible rather than blank.
  if (value && !clusters.some((cluster) => cluster.id === value) && !loading) {
    options.unshift({ value, label: 'Cluster no longer registered', description: 'Choose another one' })
  }
  const empty = !loading && !failed && clusters.length === 0
  const hint = failed
    ? 'The registered clusters could not be read. Close the editor and try again.'
    : empty
      ? 'No cluster is registered in this workspace yet.'
      : undefined

  return (
    <Field data-invalid={error ? true : undefined}>
      <span className="flex items-baseline justify-between gap-2">
        <FieldLabel htmlFor={id}>Cluster</FieldLabel>
        <Link
          to={routes.clusters()}
          className="inline-flex items-center gap-1 rounded-sm text-xs text-muted-foreground outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          Register a cluster
          <ArrowRightIcon className="size-3" aria-hidden />
        </Link>
      </span>
      <ComboboxField
        {...controlProps(id, error, Boolean(hint))}
        value={value}
        onValueChange={onValueChange}
        options={options}
        loading={loading}
        disabled={empty || failed}
        placeholder={empty ? 'No clusters registered' : 'Choose a cluster'}
        searchPlaceholder="Search clusters"
        emptyText="No cluster matches."
      />
      <FieldMessage id={id} error={error} hint={hint} />
    </Field>
  )
}
