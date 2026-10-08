/** Shared cell classes so the skeleton and the live rows have identical geometry. */
export const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

export const COLUMN_CLASS = {
  name: 'h-11 px-3',
  status: 'h-11 px-3',
  mode: 'h-11 px-3',
  stack: 'h-11 px-3',
  questions: 'h-11 px-3 text-right tabular-nums',
  scanned: 'h-11 px-3',
  actions: 'h-11 px-3 text-right',
} as const
