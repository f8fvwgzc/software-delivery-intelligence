import { FileCogIcon } from 'lucide-react'
import type { Ref } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { FileViewerSkeleton } from '@/components/system/file-viewer'
import { Frame } from '@/components/system/frame'
import { Button } from '@/components/ui/button'

import { SetupFilesBody } from './setup-files-body'

interface SetupFilesProps {
  projectId: string
  /** The files are generated only once asked for; the page owns the switch so the board's empty state can flip it too. */
  shown: boolean
  onShow: () => void
  ref?: Ref<HTMLElement>
}

/**
 * The files that wire a repository to this pipeline: the build and deploy
 * workflows, GitOps manifests, a Jenkinsfile or azure-pipelines.yml, only
 * for the deployers in use. Nothing is fetched until "Generate".
 */
export function SetupFiles({ projectId, shown, onShow, ref }: SetupFilesProps) {
  return (
    <Frame
      ref={ref}
      // Focusable from code only: the board's empty state sends focus here.
      tabIndex={-1}
      title="Setup files"
      description="Build and deploy workflows, GitOps manifests, Jenkinsfile or azure-pipelines.yml: only for the deployers this project uses."
      padding="none"
      className="scroll-mt-[calc(var(--header-h)+var(--project-band,0px)+1rem)] outline-none"
    >
      {shown ? (
        <QueryBoundary
          compact
          fallback={
            <div className="p-3">
              <FileViewerSkeleton files={4} lines={10} />
            </div>
          }
        >
          <SetupFilesBody projectId={projectId} />
        </QueryBoundary>
      ) : (
        <div className="flex flex-wrap items-center justify-between gap-3 p-3">
          <p className="max-w-[70ch] text-xs leading-5 text-muted-foreground">
            Generated from this repository’s facts and the environments above, complete and ready to commit. Review them here,
            then take them as a pull request.
          </p>
          <Button variant="outline" size="sm" onClick={onShow}>
            <FileCogIcon data-icon="inline-start" />
            Generate setup files
          </Button>
        </div>
      )}
    </Frame>
  )
}
