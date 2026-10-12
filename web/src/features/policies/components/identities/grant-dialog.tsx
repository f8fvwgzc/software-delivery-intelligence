import { KeyRoundIcon } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { useGrantPermission } from '../../api/mutations'
import { PERMISSIONS, isExpired } from '../../lib/labels'
import { count } from '@/lib/intl'

interface GrantDialogProps {
  identity: components['schemas']['Identity']
}

const DEFAULT_TTL = 30

export function GrantDialog({ identity }: GrantDialogProps) {
  const [open, setOpen] = useState(false)

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <KeyRoundIcon data-icon="inline-start" />
          Grant permission
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Grant to {identity.name}</DialogTitle>
          <DialogDescription>
            Grants are short-lived and scoped. Nothing is ever granted permanently.
          </DialogDescription>
        </DialogHeader>
        <GrantForm identity={identity} onDone={() => setOpen(false)} />
      </DialogContent>
    </Dialog>
  )
}

interface GrantFormProps {
  identity: components['schemas']['Identity']
  onDone: () => void
}

function GrantForm({ identity, onDone }: GrantFormProps) {
  const grant = useGrantPermission()
  const [selected, setSelected] = useState<string[]>([])
  const [ttl, setTtl] = useState(String(DEFAULT_TTL))
  const [submitted, setSubmitted] = useState(false)

  const held = new Set(identity.grants.filter((item) => !isExpired(item.expiresAt)).map((item) => item.permission))
  const ttlMinutes = Number.parseInt(ttl, 10)
  const ttlError = submitted && (!Number.isInteger(ttlMinutes) || ttlMinutes < 1 || ttlMinutes > 1440)
    ? 'Enter a lifetime between 1 and 1440 minutes.'
    : undefined
  const selectionError = submitted && selected.length === 0 ? 'Pick at least one permission.' : undefined

  const toggle = (permission: string, checked: boolean) => {
    setSelected((previous) => (checked ? [...previous, permission] : previous.filter((item) => item !== permission)))
  }

  const submit = (event: FormEvent) => {
    event.preventDefault()
    setSubmitted(true)
    if (selected.length === 0 || !Number.isInteger(ttlMinutes) || ttlMinutes < 1 || ttlMinutes > 1440) return

    grant.mutate(
      { identityId: identity.id, permissions: selected, ttlMinutes },
      {
        onSuccess: () => {
          toast.success(`Granted ${count(selected.length, 'permission', 'permissions')} for ${ttlMinutes} min`)
          onDone()
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
      <Field data-invalid={selectionError ? true : undefined}>
        <FieldLabel>Permissions</FieldLabel>
        <div className="grid grid-cols-2 gap-1" role="group" aria-label="Permissions">
          {PERMISSIONS.map((permission) => {
            const id = `grant-${identity.id}-${permission}`
            const alreadyHeld = held.has(permission)
            return (
              <Label
                key={permission}
                htmlFor={id}
                className={cn(
                  'flex h-8 cursor-pointer items-center gap-2 rounded-md px-2 font-mono text-xs font-normal hover:bg-muted',
                  alreadyHeld && 'text-muted-foreground',
                )}
              >
                <Checkbox id={id} checked={selected.includes(permission)} onCheckedChange={(checked) => toggle(permission, checked === true)} />
                {permission}
                {alreadyHeld && <span className="ml-auto font-sans text-2xs">held</span>}
              </Label>
            )
          })}
        </div>
        {selectionError && <FieldError>{selectionError}</FieldError>}
      </Field>

      <Field data-invalid={ttlError ? true : undefined}>
        <FieldLabel htmlFor={`ttl-${identity.id}`}>Lifetime in minutes</FieldLabel>
        <Input
          id={`ttl-${identity.id}`}
          type="number"
          inputMode="numeric"
          min={1}
          max={1440}
          value={ttl}
          onChange={(event) => setTtl(event.target.value)}
          className="w-32 tabular-nums"
          aria-invalid={ttlError ? true : undefined}
        />
        <FieldDescription>Defaults to {DEFAULT_TTL}. Expired grants fail the permission check before a step runs.</FieldDescription>
        {ttlError && <FieldError>{ttlError}</FieldError>}
      </Field>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={grant.isPending}>
          Cancel
        </Button>
        <Button type="submit" loading={grant.isPending}>
          Grant
        </Button>
      </DialogFooter>
    </form>
  )
}
