import { useQuery } from '@tanstack/react-query'
import { ShieldCheckIcon } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { DisabledReason } from '@/components/shared/feedback/disabled-reason'
import { RefusalNote } from '@/components/shared/feedback/refusal-note'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { FieldGroup } from '@/components/ui/field'
import { useAuth } from '@/features/auth/lib/auth-context'
import { projectQueries } from '@/features/projects/api/queries'

import { useConfigureDependabot } from '../../api/mutations'
import type { SecurityOverview } from '../../api/types'
import { dependabotGate, dependabotOptions, initialSelection, toRequest, type DependabotSelection } from '../../lib/dependabot'
import { OptionField } from './option-field'

interface DependabotDialogProps {
  projectId: string
  overview: SecurityOverview
}

/**
 * "Turn on Dependabot" (admin): alerts, security updates and a pull request
 * that adds `.github/dependabot.yml`, through one execution (CONTRACT.md
 * §23.4). What is off starts ticked; what is on is shown ticked and locked,
 * and is never sent. Below admin the button says why it is off.
 */
export function DependabotDialog({ projectId, overview }: DependabotDialogProps) {
  const id = useId()
  const navigate = useNavigate()
  const { currentWorkspace } = useAuth()
  const configure = useConfigureDependabot(projectId)
  const [open, setOpen] = useState(false)
  const [selection, setSelection] = useState<DependabotSelection>(() => initialSelection(overview))
  // The repository's name, from the project the layout above already loaded; never asked for here.
  const { data: project } = useQuery({ ...projectQueries.detail(projectId), enabled: false })

  const disabledReason = dependabotGate(currentWorkspace.myRole)
  const options = dependabotOptions(overview)
  const request = toRequest(selection, overview)

  const show = () => {
    // Each time it opens it starts from what GitHub says now.
    setSelection(initialSelection(overview))
    configure.reset()
    setOpen(true)
  }

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!request) return
    configure.mutate(request, {
      onSuccess: ({ executionId }) => {
        setOpen(false)
        toast.success('Dependabot setup requested', {
          description: 'Its first run pauses to ask for access to the repository’s settings.',
          action: { label: 'Open execution', onClick: () => navigate(routes.execution(executionId)) },
        })
      },
    })
  }

  return (
    <>
      <DisabledReason reason={disabledReason}>
        <Button size="sm" onClick={show} disabled={disabledReason !== null}>
          <ShieldCheckIcon data-icon="inline-start" />
          Turn on Dependabot
        </Button>
      </DisabledReason>

      <Dialog open={open} onOpenChange={(next) => (configure.isPending ? undefined : setOpen(next))}>
        <DialogContent className="sm:max-w-md">
          <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
            <DialogHeader>
              <DialogTitle>Turn on Dependabot</DialogTitle>
              <DialogDescription>
                Creates an execution on {project?.repoFullName ? <span className="font-mono text-xs">{project.repoFullName}</span> : 'this repository'}.
                Changing a repository’s settings needs admin access to it, granted for a limited time, so the first run pauses to ask
                for that access. Nothing changes before then.
              </DialogDescription>
            </DialogHeader>

            <FieldGroup className="gap-3">
              {options.map((option) => (
                <OptionField
                  key={option.key}
                  id={`${id}-${option.key}`}
                  option={option}
                  checked={option.on || selection[option.key]}
                  onCheckedChange={(checked) => setSelection((current) => ({ ...current, [option.key]: checked }))}
                  disabled={configure.isPending}
                />
              ))}
            </FieldGroup>

            {configure.isError ? <RefusalNote error={configure.error} /> : null}

            <DialogFooter className="items-center">
              {request ? null : (
                <p id={`${id}-hint`} className="mr-auto text-xs text-muted-foreground">
                  Choose at least one.
                </p>
              )}
              <Button type="button" variant="outline" onClick={() => setOpen(false)} disabled={configure.isPending}>
                Not now
              </Button>
              <Button type="submit" loading={configure.isPending} disabled={!request} aria-describedby={request ? undefined : `${id}-hint`}>
                Turn on Dependabot
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  )
}
