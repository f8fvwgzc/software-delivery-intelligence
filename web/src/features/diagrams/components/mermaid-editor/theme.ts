/**
 * Mermaid's `base` theme derives colours with khroma, which does not read
 * `oklch()`. The tokens are therefore resolved once per theme and converted
 * to hex through a 2D canvas, which normalises any CSS colour.
 */

const TOKEN_NAMES = [
  'background',
  'card',
  'foreground',
  'muted',
  'muted-foreground',
  'border',
  'brand',
  'info',
  'warning',
  'destructive',
] as const

type TokenName = (typeof TOKEN_NAMES)[number]
export type Tokens = Record<TokenName, string> & { fontSans: string; fontMono: string }

let canvasContext: CanvasRenderingContext2D | null | undefined

const OKLCH_PATTERN = /^oklch\(\s*([\d.]+%?)\s+([\d.]+)\s+([\d.]+)(?:deg)?\s*(?:\/\s*([\d.]+%?))?\s*\)$/i

function channel(value: number) {
  const gamma = value <= 0.0031308 ? 12.92 * value : 1.055 * Math.pow(value, 1 / 2.4) - 0.055
  return Math.round(Math.min(1, Math.max(0, gamma)) * 255)
}

/** OKLCH → sRGB (Björn Ottosson's matrices); the tokens are all in gamut. */
function oklchToCss(match: RegExpMatchArray): string {
  const L = match[1].endsWith('%') ? parseFloat(match[1]) / 100 : parseFloat(match[1])
  const C = parseFloat(match[2])
  const H = (parseFloat(match[3]) * Math.PI) / 180
  const a = C * Math.cos(H)
  const b = C * Math.sin(H)
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  const r = channel(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s)
  const g = channel(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s)
  const bl = channel(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s)
  const alpha = match[4] === undefined ? 1 : match[4].endsWith('%') ? parseFloat(match[4]) / 100 : parseFloat(match[4])
  if (alpha < 1) return `rgba(${r}, ${g}, ${bl}, ${alpha})`
  return `#${[r, g, bl].map((v) => v.toString(16).padStart(2, '0')).join('')}`
}

/** Any CSS colour → something khroma reads: oklch by hand, the rest normalised through a canvas. */
function toHex(cssColor: string): string {
  const oklch = cssColor.match(OKLCH_PATTERN)
  if (oklch) return oklchToCss(oklch)
  if (canvasContext === undefined) canvasContext = document.createElement('canvas').getContext('2d')
  if (!canvasContext) return cssColor
  canvasContext.fillStyle = '#000000'
  canvasContext.fillStyle = cssColor
  return canvasContext.fillStyle
}

/** Reads the design tokens the Mermaid theme maps from, for the current light/dark theme. */
export function readTokens(): Tokens {
  const style = getComputedStyle(document.documentElement)
  const read = (name: string) => style.getPropertyValue(`--${name}`).trim()
  const tokens = Object.fromEntries(TOKEN_NAMES.map((name) => [name, toHex(read(name))])) as Record<TokenName, string>
  return { ...tokens, fontSans: read('font-sans'), fontMono: read('font-mono') }
}

/** Mermaid `themeVariables` for the `base` theme, mapped from the tokens. */
export function themeVariables(tokens: Tokens, dark: boolean) {
  return {
    darkMode: dark,
    background: tokens.background,
    fontFamily: tokens.fontSans,
    fontSize: '13px',
    primaryColor: tokens.card,
    primaryTextColor: tokens.foreground,
    primaryBorderColor: tokens.border,
    secondaryColor: tokens.muted,
    secondaryTextColor: tokens.foreground,
    secondaryBorderColor: tokens.border,
    tertiaryColor: tokens.muted,
    tertiaryTextColor: tokens.foreground,
    tertiaryBorderColor: tokens.border,
    lineColor: tokens['muted-foreground'],
    textColor: tokens.foreground,
    mainBkg: tokens.card,
    nodeBorder: tokens.border,
    nodeTextColor: tokens.foreground,
    clusterBkg: tokens.muted,
    clusterBorder: tokens.border,
    titleColor: tokens.foreground,
    edgeLabelBackground: tokens.background,
    defaultLinkColor: tokens['muted-foreground'],
    // Sequence diagrams.
    actorBkg: tokens.card,
    actorBorder: tokens.border,
    actorTextColor: tokens.foreground,
    actorLineColor: tokens.border,
    signalColor: tokens.foreground,
    signalTextColor: tokens.foreground,
    labelBoxBkgColor: tokens.muted,
    labelBoxBorderColor: tokens.border,
    labelTextColor: tokens.foreground,
    loopTextColor: tokens.foreground,
    noteBkgColor: tokens.muted,
    noteBorderColor: tokens.border,
    noteTextColor: tokens.foreground,
    activationBkgColor: tokens.muted,
    activationBorderColor: tokens.border,
    // ER diagrams.
    attributeBackgroundColorOdd: tokens.card,
    attributeBackgroundColorEven: tokens.muted,
  }
}
