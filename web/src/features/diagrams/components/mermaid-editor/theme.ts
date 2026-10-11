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
  'diagram-1',
  'diagram-1-fg',
  'diagram-2',
  'diagram-2-fg',
  'diagram-3',
  'diagram-3-fg',
  'diagram-4',
  'diagram-4-fg',
  'diagram-5',
  'diagram-5-fg',
  'diagram-6',
  'diagram-6-fg',
] as const

type TokenName = (typeof TOKEN_NAMES)[number]
export type Tokens = Record<TokenName, string> & {
  fontSans: string
  fontMono: string
  /** The diagram palette's fill and stroke alphas, as the editorial canvas uses them. */
  fillAlpha: number
  strokeAlpha: number
}

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
  return {
    ...tokens,
    fontSans: read('font-sans'),
    fontMono: read('font-mono'),
    fillAlpha: Number(read('diagram-fill-alpha')) || 0.12,
    strokeAlpha: Number(read('diagram-stroke-alpha')) || 0.45,
  }
}

function hexChannels(hex: string): [number, number, number] | null {
  const match = hex.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i)
  return match ? [parseInt(match[1], 16), parseInt(match[2], 16), parseInt(match[3], 16)] : null
}

/** `colour` at `alpha` over `base`, flattened to hex: Mermaid wants opaque colours it can derive from. */
export function blend(colour: string, base: string, alpha: number): string {
  const top = hexChannels(colour)
  const under = hexChannels(base)
  if (!top || !under) return colour
  const channel = (index: number) => Math.round(top[index] * alpha + under[index] * (1 - alpha))
  return `#${[0, 1, 2].map((index) => channel(index).toString(16).padStart(2, '0')).join('')}`
}

/**
 * Mermaid `themeVariables` for the `base` theme, mapped from the tokens.
 * The editorial canvas's palette carries over: primary/secondary/tertiary
 * are the first three group tints at their fill alpha over the background,
 * with matching borders, so nodes and clusters in the Mermaid tab share
 * the editorial view's colours.
 */
export function themeVariables(tokens: Tokens, dark: boolean) {
  const bg = tokens.background
  const fill = (hue: string) => blend(hue, bg, tokens.fillAlpha)
  const border = (hue: string) => blend(hue, bg, tokens.strokeAlpha)
  const primary = fill(tokens['diagram-1'])
  const secondary = fill(tokens['diagram-2'])
  const tertiary = fill(tokens['diagram-3'])
  return {
    darkMode: dark,
    background: bg,
    fontFamily: tokens.fontSans,
    fontSize: '13px',
    primaryColor: primary,
    primaryTextColor: tokens['diagram-1-fg'],
    primaryBorderColor: border(tokens['diagram-1']),
    secondaryColor: secondary,
    secondaryTextColor: tokens['diagram-2-fg'],
    secondaryBorderColor: border(tokens['diagram-2']),
    tertiaryColor: tertiary,
    tertiaryTextColor: tokens['diagram-3-fg'],
    tertiaryBorderColor: border(tokens['diagram-3']),
    lineColor: tokens['muted-foreground'],
    textColor: tokens.foreground,
    mainBkg: primary,
    nodeBorder: border(tokens['diagram-1']),
    nodeTextColor: tokens['diagram-1-fg'],
    clusterBkg: blend(tokens['diagram-1'], bg, tokens.fillAlpha / 2),
    clusterBorder: blend(tokens['diagram-1'], bg, tokens.strokeAlpha * 0.7),
    titleColor: tokens['diagram-1-fg'],
    edgeLabelBackground: bg,
    defaultLinkColor: tokens['muted-foreground'],
    // Sequence diagrams.
    actorBkg: primary,
    actorBorder: border(tokens['diagram-1']),
    actorTextColor: tokens['diagram-1-fg'],
    actorLineColor: tokens.border,
    signalColor: tokens.foreground,
    signalTextColor: tokens.foreground,
    labelBoxBkgColor: tertiary,
    labelBoxBorderColor: border(tokens['diagram-3']),
    labelTextColor: tokens['diagram-3-fg'],
    loopTextColor: tokens['diagram-3-fg'],
    noteBkgColor: secondary,
    noteBorderColor: border(tokens['diagram-2']),
    noteTextColor: tokens['diagram-2-fg'],
    activationBkgColor: primary,
    activationBorderColor: border(tokens['diagram-1']),
    // ER diagrams.
    attributeBackgroundColorOdd: tokens.card,
    attributeBackgroundColorEven: tokens.muted,
  }
}
