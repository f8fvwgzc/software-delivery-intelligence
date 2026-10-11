import { ChevronDownIcon, DownloadIcon, FileCodeIcon, ImageIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

import { diagramFileName } from '../../lib/labels'
import { downloadBlob } from '../../lib/download'

export interface Exporter {
  exportSvg: () => string
  exportPng: () => Promise<Blob>
}

interface ExportMenuProps {
  title: string
  /** Null while the canvas chunk has not mounted yet. */
  getExporter: () => Exporter | null
}

/** "Export" split: SVG with inlined tokens, or a 2× PNG. */
export function ExportMenu({ title, getExporter }: ExportMenuProps) {
  const [busy, setBusy] = useState<'svg' | 'png' | null>(null)

  const run = async (format: 'svg' | 'png') => {
    const exporter = getExporter()
    if (!exporter) {
      toast.error('The canvas is still loading', { description: 'Open the Editorial tab, then export again.' })
      return
    }
    setBusy(format)
    try {
      const blob =
        format === 'svg'
          ? new Blob([exporter.exportSvg()], { type: 'image/svg+xml;charset=utf-8' })
          : await exporter.exportPng()
      downloadBlob(blob, diagramFileName(title, format))
    } catch (error) {
      toast.error(`Could not export the ${format.toUpperCase()}`, {
        description: error instanceof Error ? error.message : undefined,
      })
    } finally {
      setBusy(null)
    }
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" loading={busy !== null}>
          <DownloadIcon data-icon="inline-start" />
          {busy ? `Exporting ${busy.toUpperCase()}` : 'Export'}
          {busy ? null : <ChevronDownIcon data-icon="inline-end" />}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuGroup>
          <DropdownMenuLabel className="text-xs text-muted-foreground">Editorial view</DropdownMenuLabel>
          <DropdownMenuItem onSelect={() => void run('svg')}>
            <FileCodeIcon />
            SVG
            <span className="ml-auto text-2xs text-muted-foreground">vector, themed</span>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void run('png')}>
            <ImageIcon />
            PNG
            <span className="ml-auto text-2xs text-muted-foreground">2× raster</span>
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
