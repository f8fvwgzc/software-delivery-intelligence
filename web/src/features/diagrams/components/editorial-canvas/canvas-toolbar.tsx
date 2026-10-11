import { MaximizeIcon, ZoomInIcon, ZoomOutIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

interface CanvasToolbarProps {
  zoom: number
  onZoomIn: () => void
  onZoomOut: () => void
  onFit: () => void
}

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)

/** Bottom-right zoom controls over the canvas, plus the pan/zoom hint bottom-left. */
export function CanvasToolbar({ zoom, onZoomIn, onZoomOut, onFit }: CanvasToolbarProps) {
  return (
    <>
      <p
        data-screen-only
        className="pointer-events-none absolute bottom-3 left-3 hidden items-center gap-1.5 text-2xs text-muted-foreground sm:flex"
      >
        <Kbd>{isMac ? '⌘' : 'Ctrl'}</Kbd>
        <span>scroll to zoom, drag to pan</span>
      </p>
      <div
        data-screen-only
        role="toolbar"
        aria-label="Canvas zoom"
        className="absolute right-3 bottom-3 flex items-center gap-1 rounded-lg bg-background/90 p-1 ring-1 ring-border backdrop-blur-sm"
      >
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-sm" onClick={onZoomOut} aria-label="Zoom out">
              <ZoomOutIcon />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Zoom out</TooltipContent>
        </Tooltip>
        <span className="w-11 text-center font-mono text-xs tabular-nums text-muted-foreground" aria-live="polite">
          {Math.round(zoom * 100)}%
        </span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-sm" onClick={onZoomIn} aria-label="Zoom in">
              <ZoomInIcon />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Zoom in</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-sm" onClick={onFit} aria-label="Fit to view">
              <MaximizeIcon />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Fit to view</TooltipContent>
        </Tooltip>
      </div>
    </>
  )
}
