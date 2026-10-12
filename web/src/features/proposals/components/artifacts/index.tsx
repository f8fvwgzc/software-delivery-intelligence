import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { FileArchiveIcon, FilesIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { EmptyState } from '@/components/shared/feedback'
import { FileViewer, downloadFilesZip } from '@/components/system/file-viewer'
import { Button } from '@/components/ui/button'

import { optionArtifactsQuery } from '../../api/queries'

export { FileViewerSkeleton as ArtifactsSkeleton } from '@/components/system/file-viewer'

interface ArtifactsPanelProps {
  proposalId: string
  optionId: string
}

/**
 * The generated files of the option in focus, in the shared file viewer (a
 * directory tree and a code pane). Suspends on its own query; mount it
 * inside a QueryBoundary.
 */
export function ArtifactsPanel({ proposalId, optionId }: ArtifactsPanelProps) {
  const { data: artifacts } = useSuspenseQuery(optionArtifactsQuery(proposalId, optionId))

  if (artifacts.length === 0) {
    return (
      <EmptyState
        icon={FilesIcon}
        title="No generated files"
        description="This option does not add or change any file in the repository."
      />
    )
  }

  return <FileViewer files={artifacts} />
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
      await downloadFilesZip(artifacts, `${proposalId}-${optionId}-files.zip`)
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
