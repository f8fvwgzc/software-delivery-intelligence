import { useSuspenseQuery } from '@tanstack/react-query'
import { FileArchiveIcon, FilesIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { FileViewer, downloadFilesZip } from '@/components/system/file-viewer'
import { PanelEmpty } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { count } from '@/lib/intl'

import { cdQueries } from '../../api/queries'
import { OpenPullRequestButton } from './open-pull-request-button'
import { RequiredSecrets } from './required-secrets'

const HEADING_CLASS = 'text-xs font-medium text-foreground'

/**
 * The generated files in the shared viewer, what they need before they
 * work (notes, secrets), and the two ways to take them: a pull request or
 * an archive. Suspends on the artifacts; mounted only once asked for.
 */
export function SetupFilesBody({ projectId }: { projectId: string }) {
  const { data: files } = useSuspenseQuery(cdQueries.artifacts(projectId))
  const [zipping, setZipping] = useState(false)

  if (files.artifacts.length === 0) {
    return (
      <PanelEmpty
        icon={FilesIcon}
        title="Nothing to generate yet"
        description="Setup files are written for the deployers this project uses. Add an environment, then generate them again."
      />
    )
  }

  const download = async () => {
    setZipping(true)
    try {
      await downloadFilesZip(files.artifacts, 'delivery-setup-files.zip')
    } catch {
      toast.error('Could not build the archive')
    } finally {
      setZipping(false)
    }
  }

  return (
    <div className="flex min-w-0 flex-col gap-3 p-3">
      <FileViewer files={files.artifacts} label="Setup files" />
      <div className="grid gap-x-6 gap-y-4 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <section className="flex min-w-0 flex-col gap-2" aria-label="Before these files work">
          <h3 className={HEADING_CLASS}>Before these files work</h3>
          {files.notes.length > 0 ? (
            <ul className="flex list-disc flex-col gap-1 pl-4 text-xs leading-5 text-muted-foreground marker:text-muted-foreground/60">
              {files.notes.map((note) => (
                <li key={note} className="max-w-[75ch]">
                  {note}
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-xs text-muted-foreground">Nothing to configure by hand.</p>
          )}
        </section>
        <section className="flex min-w-0 flex-col gap-2" aria-label="Secrets the files reference">
          <h3 className={HEADING_CLASS}>Secrets the files reference</h3>
          <RequiredSecrets names={files.requiredSecrets} />
        </section>
      </div>
      <div className="flex flex-wrap items-center gap-2 border-t pt-3">
        <OpenPullRequestButton projectId={projectId} files={files} />
        <Button variant="outline" size="sm" onClick={download} loading={zipping}>
          <FileArchiveIcon data-icon="inline-start" />
          Download all
        </Button>
        <span className="text-xs tabular-nums text-muted-foreground">
          {count(files.artifacts.length, 'file', 'files')}, committed to a new branch through the github adapter.
        </span>
      </div>
    </div>
  )
}
