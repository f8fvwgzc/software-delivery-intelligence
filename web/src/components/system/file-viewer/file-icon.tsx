import { createElement } from 'react'

import { fileIcon, type ViewerFile } from './files'

interface FileGlyphProps {
  file: Pick<ViewerFile, 'language' | 'path'>
  className?: string
}

/** The lucide icon for a file's language (or extension). Decorative. */
export function FileGlyph({ file, className }: FileGlyphProps) {
  return createElement(fileIcon(file), { className, 'aria-hidden': true })
}
