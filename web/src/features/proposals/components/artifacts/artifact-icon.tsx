import { createElement } from 'react'

import { artifactIcon, type Artifact } from '../../lib/artifacts'

interface ArtifactIconProps {
  artifact: Pick<Artifact, 'language' | 'path'>
  className?: string
}

/** The lucide icon for a generated file's language (or extension). Decorative. */
export function ArtifactIcon({ artifact, className }: ArtifactIconProps) {
  return createElement(artifactIcon(artifact), { className, 'aria-hidden': true })
}
