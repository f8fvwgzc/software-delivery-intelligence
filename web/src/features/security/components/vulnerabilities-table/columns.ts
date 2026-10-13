/**
 * Estimated row height in px for the windowing math: `h-13` on a 15px root
 * plus the row's hairline. Windowed rows are measured, so this only has to
 * be close.
 */
export const ROW_HEIGHT = 50

export const COLUMN_COUNT = 6

/** Shared cell classes so the skeleton and the live rows have identical geometry. */
export const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'
export const CELL_CLASS = 'h-13 px-3 py-1.5'
