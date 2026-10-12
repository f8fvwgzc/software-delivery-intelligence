# Tokens — Software Delivery Intelligence

Source of truth is `web/src/index.css`. This file records the decisions.

## Type

- Root 15px, but the scale is pinned in pixels: body `text-sm` 13px/20,
  metadata `text-xs` 12px/16, tags and labels `text-2xs` 11px/16. Frame
  titles 12px/500, page title 15px/600. Small caps only on stat labels.
- Sans: Geist Variable. Mono: Geist Mono Variable for ids, SHAs, timestamps,
  durations, code and `<Kbd>`.
- Headings `tracking-tight`; page title 18px/600, section 14px/500.
- `tabular-nums` on every number.

## Colour (OKLCH, near-neutral with a faint cool tint)

| Token | Light | Dark | Use |
|---|---|---|---|
| `--background` | `1 0 0` | `0.17 0.006 265` | page |
| `--foreground` | `0.24 0.008 265` | `0.94 0.004 265` | text |
| `--muted-foreground` | `0.54 0.012 265` | `0.68 0.012 265` | secondary text |
| `--border` | foreground `/ 7%` | `1 0 0 / 8%` | hairlines, one contrast in both themes |
| `--brand` | `0.55 0.17 268` | `0.7 0.14 268` | the one accent |
| `--success` | `0.6 0.13 158` | `0.72 0.14 158` | status only |
| `--warning` | `0.7 0.14 72` | `0.78 0.13 78` | status only |
| `--destructive` | `0.6 0.2 25` | `0.68 0.18 22` | status / danger |
| `--info` | `0.6 0.12 238` | `0.72 0.11 238` | status only |
| `--sidebar` | `0.984 0.002 265` | `0.155 0.006 265` | tinted, never black |

Accent budget: ≤5 brand placements per viewport (primary action, active nav,
hero metric, selected row, focus ring). Status colours appear as 6px dots,
badge text and bar fills; backgrounds stay neutral or ≤10% tint.

## Radius

`--radius: 0.5rem` → sm 4.8px (chips) · md 6.4px (controls) · lg 8px (cards)
· xl 11px (dialogs). Badges use `rounded-md`, never pill.

## Elevation

Rings over shadows: `ring-1 ring-border` on cards and the inset frame. Popovers
and dialogs keep shadcn's single soft shadow. No glow, no layered colour
shadows.

## Spacing

4/8px grid. Page gutter 24px (16px on phones). Card padding 16px (12px for
`size="sm"`). Table cells `px-3 py-2`, rows 40–44px. Toolbars `gap-2 h-8`.

## Motion

Tokens: `--motion-fast` 120ms (colour, opacity), `--motion-base` 180ms
(presence, rail steps), `--motion-slow` 250ms (the ceiling); easings
`--motion-ease` (standard) and `--motion-ease-out` (expo). Page enter is a
150ms fade + 4px rise through `motion` (`PageTransition`); list rows stagger
80ms in CSS. Only the live status dot pulses. `prefers-reduced-motion`
disables all of it.

## System primitives

`web/src/components/system` (Frame, Panel, Tag, Row, Rail, Toolbar, Stat,
Code, Stack/Inline, PageTransition) is the layer every page composes;
`web/DESIGN.md` describes the world, the folder's README the contract.

## Icons

Lucide only, `stroke-width: 1.5` globally, 16px in buttons and rows, 14px in
metadata.
