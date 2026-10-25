import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { toSaveError, type SaveError } from '@/features/delivery-cd/lib/field-errors'

import { useSaveCluster } from '../../api/mutations'
import type { Cluster } from '../../api/types'
import { CLUSTER_FIELDS, clusterControlId, toClusterDraft, toClusterInput, validateCluster, type ClusterDraft } from '../../lib/cluster-form'
import { controlProps, FieldMessage, Optional } from '../form-parts'
import { KubeconfigSecretField } from './kubeconfig-secret-field'

interface ClusterEditorProps {
  /** The cluster to edit; omitted to add one. It stays set while the dialog closes. */
  cluster?: Cluster
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** Adds or edits a cluster (admin): its name, the secret that holds its kubeconfig, and what SDI may watch in it. */
export function ClusterEditor({ cluster, open, onOpenChange }: ClusterEditorProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        {open ? <ClusterForm cluster={cluster} onDone={() => onOpenChange(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

/** The answer says whether the cluster answered: an unreachable one is saved anyway, and the toast says why. */
function announce(saved: Cluster, isNew: boolean) {
  const verb = isNew ? 'added' : 'saved'
  if (saved.status === 'connected') {
    toast.success(`${saved.name} ${verb} and connected`, {
      description: [saved.serverVersion ? `Kubernetes ${saved.serverVersion}` : null, saved.server].filter(Boolean).join(' at ') || undefined,
    })
  } else if (saved.status === 'unreachable') {
    toast.warning(`${saved.name} ${verb}, but it did not answer`, { description: saved.message ?? 'Check the kubeconfig and the API server address.' })
  } else {
    toast.success(`${saved.name} ${verb}`)
  }
}

function ClusterForm({ cluster, onDone }: { cluster?: Cluster; onDone: () => void }) {
  const save = useSaveCluster()
  const [draft, setDraft] = useState<ClusterDraft>(() => toClusterDraft(cluster))
  const [submitted, setSubmitted] = useState(false)
  const [refused, setRefused] = useState<SaveError | null>(null)

  const clientErrors = submitted ? validateCluster(draft) : {}
  const errors = refused?.field ? { ...clientErrors, [refused.field]: refused.message } : clientErrors
  const id = clusterControlId

  const change = (patch: Partial<ClusterDraft>) => {
    setDraft((current) => ({ ...current, ...patch }))
    setRefused(null)
  }
  const focusField = (field: string) => document.getElementById(id(field))?.focus()

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setSubmitted(true)
    const firstInvalid = Object.keys(validateCluster(draft))[0]
    if (firstInvalid) {
      focusField(firstInvalid)
      return
    }
    save.mutate(
      { id: cluster?.id, input: toClusterInput(draft) },
      {
        onSuccess: (saved) => {
          announce(saved, !cluster)
          onDone()
        },
        onError: (error) => {
          const next = toSaveError(error, CLUSTER_FIELDS)
          setRefused(next)
          if (next.field) focusField(next.field)
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4" noValidate>
      <DialogHeader>
        <DialogTitle>{cluster ? `Edit ${cluster.name}` : 'Add cluster'}</DialogTitle>
        <DialogDescription>
          A kubeconfig is never typed here: the cluster names the workspace secret that holds it. SDI reads it in memory to reach the
          API server and never shows or returns it. Saving reaches the cluster once.
        </DialogDescription>
      </DialogHeader>

      <div className="-mx-4 flex max-h-[min(36rem,calc(100svh-14rem))] flex-col gap-4 overflow-y-auto px-4 py-1">
        {refused && !refused.field ? (
          <Alert variant="destructive">
            <AlertTitle>The cluster was not saved</AlertTitle>
            <AlertDescription>{refused.message}</AlertDescription>
          </Alert>
        ) : null}

        <FieldGroup className="gap-4">
          <Field data-invalid={errors.name ? true : undefined}>
            <FieldLabel htmlFor={id('name')}>Name</FieldLabel>
            <Input
              {...controlProps(id('name'), errors.name)}
              value={draft.name}
              onChange={(event) => change({ name: event.target.value })}
              placeholder="production"
              autoComplete="off"
              spellCheck={false}
              maxLength={63}
              autoFocus={!cluster}
            />
            <FieldMessage id={id('name')} error={errors.name} />
          </Field>
          <Field data-invalid={errors.kubeconfigSecret ? true : undefined}>
            <FieldLabel htmlFor={id('kubeconfigSecret')}>Kubeconfig secret</FieldLabel>
            <KubeconfigSecretField
              {...controlProps(id('kubeconfigSecret'), errors.kubeconfigSecret, true)}
              value={draft.kubeconfigSecret}
              onValueChange={(kubeconfigSecret) => change({ kubeconfigSecret })}
            />
            <FieldMessage
              id={id('kubeconfigSecret')}
              error={errors.kubeconfigSecret}
              hint="A workspace secret whose value is the kubeconfig, YAML or JSON."
            />
          </Field>
        </FieldGroup>

        <FieldGroup className="grid gap-4 sm:grid-cols-2">
          <Field data-invalid={errors.context ? true : undefined}>
            <FieldLabel htmlFor={id('context')}>
              Context <Optional />
            </FieldLabel>
            <Input
              {...controlProps(id('context'), errors.context, true)}
              value={draft.context}
              onChange={(event) => change({ context: event.target.value })}
              placeholder="current context"
              autoComplete="off"
              spellCheck={false}
              className="font-mono text-xs md:text-xs"
            />
            <FieldMessage id={id('context')} error={errors.context} hint="Empty uses the kubeconfig’s current context." />
          </Field>
          <Field data-invalid={errors.defaultNamespace ? true : undefined}>
            <FieldLabel htmlFor={id('defaultNamespace')}>
              Default namespace <Optional />
            </FieldLabel>
            <Input
              {...controlProps(id('defaultNamespace'), errors.defaultNamespace, true)}
              value={draft.defaultNamespace}
              onChange={(event) => change({ defaultNamespace: event.target.value })}
              placeholder="default"
              autoComplete="off"
              spellCheck={false}
              className="font-mono text-xs md:text-xs"
            />
            <FieldMessage
              id={id('defaultNamespace')}
              error={errors.defaultNamespace}
              hint="For applications and resources that name none."
            />
          </Field>
          <Field data-invalid={errors.namespaces ? true : undefined} className="sm:col-span-2">
            <FieldLabel htmlFor={id('namespaces')}>
              Namespaces <Optional />
            </FieldLabel>
            <Input
              {...controlProps(id('namespaces'), errors.namespaces, true)}
              value={draft.namespaces}
              onChange={(event) => change({ namespaces: event.target.value })}
              placeholder="shop, payments"
              autoComplete="off"
              spellCheck={false}
              className="font-mono text-xs md:text-xs"
            />
            <FieldMessage
              id={id('namespaces')}
              error={errors.namespaces}
              hint="Separated by commas. When set, SDI watches and writes only these; empty watches the whole cluster."
            />
          </Field>
        </FieldGroup>
      </div>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={save.isPending}>
          Cancel
        </Button>
        <Button type="submit" loading={save.isPending}>
          {cluster ? 'Save cluster' : 'Add cluster'}
        </Button>
      </DialogFooter>
    </form>
  )
}
