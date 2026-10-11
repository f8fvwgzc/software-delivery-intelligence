import type { Box } from './types'

const SVG_NS = 'http://www.w3.org/2000/svg'
const TOKEN_PATTERN = /var\(--([a-z0-9-]+)\)/g

/** Token values land inside attributes, so a font stack's quotes must be escaped. */
function escapeAttribute(value: string) {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')
}

/** Resolves every `var(--token)` in the markup against the document's current theme. */
function inlineTokens(markup: string): string {
  const rootStyle = getComputedStyle(document.documentElement)
  const cache = new Map<string, string>()
  return markup.replace(TOKEN_PATTERN, (match, name: string) => {
    let value = cache.get(name)
    if (value === undefined) {
      const resolved = rootStyle.getPropertyValue(`--${name}`).trim()
      value = resolved ? escapeAttribute(resolved) : match
      cache.set(name, value)
    }
    return value
  })
}

/**
 * The live `<svg>` as a standalone document: the pan/zoom transform is
 * dropped, the viewBox is the drawing's bounds, hover state is cleared and
 * Tailwind classes (meaningless outside the app) are removed.
 */
export function serialiseSvg(svg: SVGSVGElement, bounds: Box, title: string): string {
  const clone = svg.cloneNode(true) as SVGSVGElement
  clone.setAttribute('xmlns', SVG_NS)
  clone.setAttribute('xmlns:xlink', 'http://www.w3.org/1999/xlink')
  clone.setAttribute('viewBox', `${bounds.x} ${bounds.y} ${bounds.w} ${bounds.h}`)
  clone.setAttribute('width', String(Math.round(bounds.w)))
  clone.setAttribute('height', String(Math.round(bounds.h)))
  clone.removeAttribute('class')
  clone.removeAttribute('style')
  // An open menu (the export menu itself) hides the rest of the page from assistive technology.
  clone.removeAttribute('aria-hidden')
  clone.removeAttribute('data-aria-hidden')
  clone.setAttribute('style', `font-family: var(--font-sans); background: var(--card)`)
  clone.setAttribute('role', 'img')
  clone.setAttribute('aria-label', title)

  const content = clone.querySelector<SVGGElement>('[data-content]')
  content?.removeAttribute('transform')

  for (const element of clone.querySelectorAll<SVGElement>('*')) {
    element.removeAttribute('class')
    element.removeAttribute('tabindex')
    if (element.hasAttribute('data-dim')) element.style.removeProperty('opacity')
    if (element.hasAttribute('data-screen-only')) element.remove()
  }

  const markup = new XMLSerializer().serializeToString(clone)
  return `<?xml version="1.0" encoding="UTF-8"?>\n${inlineTokens(markup)}`
}

/** Rasterises an SVG document at `scale`× its intrinsic size. */
export async function rasterisePng(svgMarkup: string, bounds: Box, scale = 2): Promise<Blob> {
  const url = URL.createObjectURL(new Blob([svgMarkup], { type: 'image/svg+xml;charset=utf-8' }))
  try {
    // `decode()` never settles for a blob SVG in Chromium; the load events do.
    const image = await new Promise<HTMLImageElement>((resolve, reject) => {
      const element = new Image()
      element.onload = () => resolve(element)
      element.onerror = () => reject(new Error('The SVG could not be rasterised.'))
      element.src = url
    })

    const canvas = document.createElement('canvas')
    canvas.width = Math.round(bounds.w * scale)
    canvas.height = Math.round(bounds.h * scale)
    const context = canvas.getContext('2d')
    if (!context) throw new Error('Canvas is not available in this browser.')
    context.scale(scale, scale)
    context.drawImage(image, 0, 0, bounds.w, bounds.h)

    return await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('The PNG could not be encoded.'))), 'image/png')
    })
  } finally {
    URL.revokeObjectURL(url)
  }
}
