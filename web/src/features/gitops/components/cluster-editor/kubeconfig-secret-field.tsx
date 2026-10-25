import { KeyRoundIcon } from 'lucide-react'
import { useState } from 'react'

import { SecretNameField } from '@/components/shared/form'
import { Button } from '@/components/ui/button'
import { useSecretField } from '@/features/delivery-cd/lib/use-secret-field'

import { StoreKubeconfig } from './store-kubeconfig'

interface KubeconfigSecretFieldProps {
  id: string
  value: string
  onValueChange: (name: string) => void
  'aria-invalid'?: boolean
  'aria-describedby'?: string
}

/**
 * The workspace secret that holds a cluster's kubeconfig: picked by name
 * from the workspace's secrets, as registries pick theirs, or stored here
 * through "Store a new secret". Only names ever reach the screen.
 */
export function KubeconfigSecretField({ id, value, onValueChange, ...aria }: KubeconfigSecretFieldProps) {
  const { onAdd, ...secretField } = useSecretField(true)
  const [storing, setStoring] = useState(false)

  return (
    <div className="flex min-w-0 flex-col gap-2">
      {/* The field's own one-line "Add secret" would join a kubeconfig's lines: the store path below replaces it. */}
      <SecretNameField id={id} value={value} onValueChange={onValueChange} {...secretField} {...aria} />
      {onAdd && !storing ? (
        <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => setStoring(true)}>
          <KeyRoundIcon data-icon="inline-start" />
          Store a new secret
        </Button>
      ) : null}
      {onAdd && storing ? (
        <StoreKubeconfig
          onStore={onAdd}
          taken={secretField.secrets.map((secret) => secret.name)}
          onDone={(name) => {
            setStoring(false)
            if (name) onValueChange(name)
          }}
        />
      ) : null}
    </div>
  )
}
