import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { FileArchiveIcon, FilesIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { EmptyState } from '@/components/shared/feedback'
import { Button } from '@/components/ui/button'

import { optionArtifactsQuery } from '../../api/queries'
import { downloadArtifactsZip, groupByDirectory } from '../../lib/artifacts'
import { CodePane } from './code-pane'
import { FileTree } from './file-tree'

export { ArtifactsSkeleton } from './skeleton'

interface ArtifactsPanelProps {
  proposalId: string
  optionId: string
}

/**
 * The generated files of the option in focus: a directory tree and a code
 * pane. Suspends on its own query; mount it inside a QueryBoundary.
 */
export function ArtifactsPanel({ proposalId, optionId }: ArtifactsPanelProps) {
  const { data: artifacts } = useSuspenseQuery(optionArtifactsQuery(proposalId, optionId))
  const groups = groupByDirectory(artifacts)
  const [chosenPath, setChosenPath] = useState<string | null>(null)
  const selected = artifacts.find((artifact) => artifact.path === chosenPath) ?? groups[0]?.files[0] ?? null

  if (artifacts.length === 0 || !selected) {
    return (
      <EmptyState
        icon={FilesIcon}
        title="No generated files"
        description="This option does not add or change any file in the repository."
      />
    )
  }

  return (
    // Container query: the tree sits beside the pane only when the panel itself is wide enough.
    <div className="@container">
      <div className="grid overflow-hidden rounded-lg ring-1 ring-border @2xl:grid-cols-[14rem_minmax(0,1fr)]">
        <div className="border-b @2xl:border-r @2xl:border-b-0">
          <FileTree groups={groups} selectedPath={selected.path} onSelect={setChosenPath} />
        </div>
        <CodePane key={selected.path} artifact={selected} />
      </div>
    </div>
  )
}

interface DownloadAllButtonProps {
  proposalId: string
  optionId: string
  /** From the proposal body; the files themselves are only fetched on click. */
  artifactCount: number
}

/** Zips every file of the option client-side. Reuses the cached artifacts when the panel already loaded them. */
export function DownloadAllButton({ proposalId, optionId, artifactCount }: DownloadAllButtonProps) {
  const queryClient = useQueryClient()
  const [busy, setBusy] = useState(false)

  const download = async () => {
    setBusy(true)
    try {
      const artifacts = await queryClient.ensureQueryData(optionArtifactsQuery(proposalId, optionId))
      if (artifacts.length === 0) {
        toast('This option has no files to download')
        return
      }
      await downloadArtifactsZip(artifacts, `${proposalId}-${optionId}-files.zip`)
    } catch {
      toast.error('Could not build the archive')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Button variant="outline" size="sm" onClick={download} loading={busy} disabled={artifactCount === 0}>
      <FileArchiveIcon data-icon="inline-start" />
      Download all
    </Button>
  )
}
