import { createElement } from 'react'

import { entryIcon } from '../../lib/icons'

interface EntryGlyphProps {
  name: string
  isDir: boolean
  language?: string
  /** Open folder glyph for an expanded directory. */
  open?: boolean
  className?: string
}

/** The Lucide glyph for a tree entry, picked by kind, name and extension. */
export function EntryGlyph({ name, isDir, language, open = false, className }: EntryGlyphProps) {
  return createElement(entryIcon({ name, isDir, language }, open), { className, 'aria-hidden': true })
}
