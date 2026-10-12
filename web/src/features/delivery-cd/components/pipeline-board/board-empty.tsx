import { FileCogIcon, WorkflowIcon } from 'lucide-react'

import { PanelEmpty } from '@/components/system/panel'
import { Button } from '@/components/ui/button'

import { AddEnvironmentButton } from '../environment-editor/add-environment-button'

interface BoardEmptyProps {
  projectId: string
  canAdmin: boolean
  /** Opens the setup files panel further down the page. */
  onGenerateSetup: () => void
}

/** No environment yet: what continuous delivery means here, and the two ways to start. */
export function BoardEmpty({ projectId, canAdmin, onGenerateSetup }: BoardEmptyProps) {
  return (
    <PanelEmpty
      icon={WorkflowIcon}
      title="No environments yet"
      description="CI builds one image; continuous delivery moves that exact image through your environments in order, each step behind the same permission, policy and approval gates as any other execution. Add the first environment, or start from the build and deploy files generated for this repository."
      action={
        <div className="flex flex-wrap items-center justify-center gap-2">
          {canAdmin ? <AddEnvironmentButton projectId={projectId} size="sm" /> : null}
          <Button variant="outline" size="sm" onClick={onGenerateSetup}>
            <FileCogIcon data-icon="inline-start" />
            Generate setup files
          </Button>
        </div>
      }
    />
  )
}
