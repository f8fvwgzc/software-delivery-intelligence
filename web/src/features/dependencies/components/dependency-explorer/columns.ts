/** Shared cell classes so skeleton rows and live rows line up. */
export const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

export const CELL_CLASS = {
  name: 'h-10 px-3 font-mono text-sm',
  ecosystem: 'h-10 px-3 text-xs',
  version: 'h-10 px-3 font-mono text-xs tabular-nums',
  scope: 'h-10 px-3 text-xs',
  manifest: 'h-10 px-3 font-mono text-xs text-muted-foreground',
} as const

export const PAGE_SIZE = 50
