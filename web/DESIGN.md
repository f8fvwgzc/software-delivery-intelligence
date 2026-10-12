# DESIGN.md — Software Delivery Intelligence (web)

The visual world of the control plane. `.ui-craft/brief.md` holds the
product identity; this file holds the durable decisions a new surface
inherits. `src/components/system/README.md` is the component contract.

## The world in one line

A light, quiet workbench: white surfaces, hairlines instead of boxes,
13px text, 11px tags, one indigo accent, and motion that never exceeds a
quarter of a second. The feel of a code-review tool, not a dashboard
template.

## Surfaces

- Page background is white (`--background`); the sidebar is barely tinted
  (`--sidebar`, L 0.985). The dark theme keeps the same structure on a
  near-black cool grey (L 0.175) with the same 8% hairlines.
- Structure is drawn with 1px lines at 7% of the foreground (`--border`),
  never with filled cards or shadows. Popovers and dialogs keep shadcn's
  one soft shadow; nothing else casts one.
- `--muted` is barely off-white (L 0.982): hover washes, code blocks and
  the table header use it. It must read as "paper", not grey.
- Radius: 8px for frames and tables, 6px for controls, rail bodies and
  code, 4px for tags. Dialogs 11px.

## Type

Geist Variable for text, Geist Mono for ids, shas, paths, durations and
code. Root 15px, but the scale is pinned in pixels: body 13px/20,
metadata 12px/16, tags and labels 11px/16, control text 13px. Page title
15px/600, frame title 12px/500. Numbers are tabular everywhere. Sentence
case; small caps only for the 11px stat labels.

## Colour

Neutral cool grey (hue 265) with one accent, `--brand` (indigo, hue 268).
The accent budget is five placements per viewport: the primary button, the
active nav pill, one hero stat, the selected row, the focus ring. Status
colours (`success warning destructive info`) appear only as 6px dots, tag
text and bar fills on ≤ 10% tints. No gradients, no glows.

## Motion

Durations are tokens: `--motion-fast` 120ms (colour, opacity),
`--motion-base` 180ms (presence, rail steps), `--motion-slow` 250ms (the
ceiling). Easing `--motion-ease` (standard) and `--motion-ease-out`
(expo). Page enter is a 150ms fade with a 4px rise through `motion`; list
rows stagger 80ms in CSS; only the live status dot pulses.
`prefers-reduced-motion` disables all of it.

## Layout

- Shell: a 40px header (breadcrumb, ⌘K search, theme, user) over the page;
  a permanent 216px sidebar with one calm list (11px section labels, 36px
  rows, active row a brand-tinted pill) and the workspace switcher at the
  top.
- Page gutters 24px (16px on phones); frames stack with 16px between them;
  inside a frame 12px padding, or none for tables and row lists.
- Dense data keeps its table density: 36px rows, `px-3 py-2` cells, a
  muted header row.
- Two-column pages put the reading column first and the sticky aside
  (negotiation, event log) at 22–28rem.

## Components

Every region is a `Frame`; every data region is a `Panel`. Lists are
`Row`s; sequences are `Rail`s; states are `Tag`s; numbers are `Stat`s.
shadcn/ui primitives underneath are composed, never forked.

## Refuse

Card grids of icon + heading + text; eyebrows over headings; thick coloured
left borders; pills on comparisons; uppercase headings; glass, blur and
gradient decoration; emoji icons; anything that moves on scroll.
