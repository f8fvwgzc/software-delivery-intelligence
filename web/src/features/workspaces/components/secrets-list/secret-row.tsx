import { KeyRoundIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { Row } from '@/components/system/row'
import { Button } from '@/components/ui/button'
import type { SecretInfo } from '@/features/delivery-cd/api/types'

interface SecretRowProps {
  secret: SecretInfo
  onReplace: (secret: SecretInfo) => void
  onDelete: (name: string) => void
}

/** One secret: its name, the last four characters for recognition, who set it and when. Never the value. */
export function SecretRow({ secret, onReplace, onDelete }: SecretRowProps) {
  return (
    <Row
      mono
      leading={<KeyRoundIcon className="text-muted-foreground" aria-hidden />}
      title={secret.name}
      meta={
        <>
          {secret.hint ? (
            <span>
              ends in <span className="font-mono text-foreground/80">{secret.hint}</span>
            </span>
          ) : null}
          {secret.description ? <span className="max-w-[60ch] truncate">{secret.description}</span> : null}
          <span>
            {secret.updatedBy ? `set by ${secret.updatedBy} ` : 'set '}
            <RelativeTime value={secret.updatedAt} />
          </span>
        </>
      }
      trailing={
        <>
          <Button variant="ghost" size="xs" onClick={() => onReplace(secret)} aria-label={`Replace the value of ${secret.name}`}>
            Replace
          </Button>
          <Button variant="ghost" size="xs" className="text-destructive hover:text-destructive" onClick={() => onDelete(secret.name)} aria-label={`Delete ${secret.name}`}>
            Delete
          </Button>
        </>
      }
    />
  )
}
