import { PlusIcon } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

import { useCreateRelease } from '../../api/mutations'
import { aliasesOf, toSaveError, type SaveError } from '../../lib/field-errors'

const SHA_PATTERN = /^[0-9a-f]{7,40}$/i
const FIELDS = [
  { field: 'commitSha', aliases: [...aliasesOf('commitSha'), 'commit', 'sha'] },
  { field: 'version', aliases: aliasesOf('version') },
]

interface NewReleaseDialogProps {
  projectId: string
  variant?: 'default' | 'outline' | 'ghost'
  size?: 'sm' | 'xs'
}

/** Creates a release by hand (member): by default the image built at the default branch head. */
export function NewReleaseDialog({ projectId, variant = 'outline', size = 'sm' }: NewReleaseDialogProps) {
  const [open, setOpen] = useState(false)

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant={variant} size={size}>
          <PlusIcon data-icon="inline-start" />
          New release
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        {open ? <NewReleaseForm projectId={projectId} onDone={() => setOpen(false)} /> : null}
      </DialogContent>
    </Dialog>
  )
}

function NewReleaseForm({ projectId, onDone }: { projectId: string; onDone: () => void }) {
  const id = useId()
  const create = useCreateRelease(projectId)
  const [commitSha, setCommitSha] = useState('')
  const [version, setVersion] = useState('')
  const [submitted, setSubmitted] = useState(false)
  const [refused, setRefused] = useState<SaveError | null>(null)

  const sha = commitSha.trim()
  const shaError =
    refused?.field === 'commitSha'
      ? refused.message
      : submitted && sha && !SHA_PATTERN.test(sha)
        ? 'A commit SHA is 7 to 40 hexadecimal characters.'
        : null
  const versionError = refused?.field === 'version' ? refused.message : null

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setSubmitted(true)
    if (sha && !SHA_PATTERN.test(sha)) {
      document.getElementById(`${id}-sha`)?.focus()
      return
    }
    create.mutate(
      { commitSha: sha, version },
      {
        onSuccess: (release) => {
          toast.success(`Release ${release.version} created`, { description: 'It is the first stage’s candidate now.' })
          onDone()
        },
        onError: (error) => {
          const next = toSaveError(error, FIELDS)
          setRefused(next)
          if (next.field) document.getElementById(`${id}-${next.field === 'commitSha' ? 'sha' : 'version'}`)?.focus()
        },
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
      <DialogHeader>
        <DialogTitle>New release</DialogTitle>
        <DialogDescription>
          Names the image built at one commit so it can be promoted. Leave both fields empty for the default branch head.
        </DialogDescription>
      </DialogHeader>
      {refused && !refused.field ? (
        <Alert variant="destructive">
          <AlertTitle>The release was not created</AlertTitle>
          <AlertDescription>{refused.message}</AlertDescription>
        </Alert>
      ) : null}
      <FieldGroup className="gap-4">
        <Field data-invalid={shaError ? true : undefined}>
          <FieldLabel htmlFor={`${id}-sha`}>
            Commit SHA <span className="font-normal text-muted-foreground">optional</span>
          </FieldLabel>
          <Input
            id={`${id}-sha`}
            value={commitSha}
            onChange={(event) => {
              setCommitSha(event.target.value)
              setRefused(null)
            }}
            placeholder="Default branch head"
            autoComplete="off"
            spellCheck={false}
            maxLength={40}
            autoFocus
            aria-invalid={shaError ? true : undefined}
            aria-describedby={shaError ? `${id}-sha-error` : undefined}
            className="font-mono text-xs md:text-xs"
          />
          {shaError ? (
            <FieldError id={`${id}-sha-error`} className="text-xs">
              {shaError}
            </FieldError>
          ) : null}
        </Field>
        <Field data-invalid={versionError ? true : undefined}>
          <FieldLabel htmlFor={`${id}-version`}>
            Version <span className="font-normal text-muted-foreground">optional</span>
          </FieldLabel>
          <Input
            id={`${id}-version`}
            value={version}
            onChange={(event) => {
              setVersion(event.target.value)
              setRefused(null)
            }}
            placeholder="The short SHA"
            autoComplete="off"
            spellCheck={false}
            maxLength={64}
            aria-invalid={versionError ? true : undefined}
            aria-describedby={versionError ? `${id}-version-error` : `${id}-version-hint`}
            className="font-mono text-xs md:text-xs"
          />
          {versionError ? (
            <FieldError id={`${id}-version-error`} className="text-xs">
              {versionError}
            </FieldError>
          ) : (
            <FieldDescription id={`${id}-version-hint`} className="text-xs">
              What people call it, for example v1.4.2. The image tag stays the short SHA.
            </FieldDescription>
          )}
        </Field>
      </FieldGroup>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={create.isPending}>
          Cancel
        </Button>
        <Button type="submit" loading={create.isPending}>
          Create release
        </Button>
      </DialogFooter>
    </form>
  )
}
