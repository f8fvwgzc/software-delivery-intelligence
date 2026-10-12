# System primitives

The customised layer every feature builds on. Each primitive is composed
from shadcn/ui (`components/ui`, re-exported, never forked) and the tokens
in `src/index.css`. `DESIGN.md` at `web/` describes the world these live in;
this file is the contract.

```
import { Frame, Panel, Row, RowList, Tag, Rail, RailStep, Toolbar, Stat, Code, Stack, Inline } from '@/components/system'
```

| Primitive | Job | Rules |
|---|---|---|
| `Frame` | Hairline-bordered section with a 32px header row: `title` (12px medium, `h2`), `description` (12px muted, truncates), `meta` (tags, counts), `actions` (ghost controls). `padding="none"` for tables and row lists. `bare` drops the ring. Sub-frames: `as="h3"`. | One Frame per region. Never a Frame inside a Card or a Card inside a Frame. |
| `Panel` | `Frame` + `QueryBoundary`: the header paints at once, the body suspends behind `skeleton` (default: `PanelSkeleton` rows), errors render a compact retry inside the body. Children render `PanelEmpty` when the list is empty. | Every data region of a page is a Panel with its own `useSuspenseQuery`. Nothing above the fold waits for anything below it. |
| `Tag` | 11px, 20px tall, 4px radius, tinted by `tone` (`neutral info success warning destructive brand`); `dot` and `live` for lifecycle states; `mono` for keys; `variant="outline"` for counts. `TagDot` on its own for table cells. | Status → tone maps live in `features/<feature>/lib/labels.ts`. Never a pill, never a saturated fill. |
| `Row` / `RowList` | One list row: `leading` (16px glyph, avatar or dot), `title`, `meta` (12px muted, inline), `trailing`. `to` makes it a link, `onClick` a button, `active` tints it brand/5. `RowList` adds the hairlines; wrap each Row in an `li`. | Lists of ≤ 50 things are Rows; dense data with columns stays a `Table`. |
| `Rail` / `RailStep` | The vertical step rail: a 20px marker per step (`status`: pending ready running waiting done failed skipped blocked), one hairline behind them, a framed body with `title`, `meta`, `trailing`, `detail` and expanded `children`. `emphasis` tints the body ring; `compact` drops the body frame for previews. | Executions, reviews, scans and plan previews all use it. Step changes fade (`--motion-base`); nothing slides. |
| `Toolbar` | 32px strip of ghost controls (`ToolbarGroup`, `ToolbarSeparator`, `ToolbarSpacer`, `ToolbarText`). | Filters, toggles and counts. The page's primary action is in its header, not here. |
| `Stat` / `StatStrip` | Label in 11px small caps, value in tabular figures, `delta` as plain muted text. `framed` boxes one stat; `StatStrip` lays several out with hairlines between. | Comparisons are text ("+3 this week"), never coloured pills. One `tone="brand"` stat per page at most. |
| `Code` / `InlineCode` | 12px mono block on a muted wash with an inset hairline; `wrap` for prose-length lines. `InlineCode` for a path or sha in a sentence. | Mono is for code, data and measurement, not decoration. |
| `Stack` / `Inline` | Flex column / row with `gap` on the 4px grid (`0 1 2 3 4 5 6 8`). | Page rhythm is `Stack gap={4}` (16px) between frames, `gap={2}` inside them. |
| `PageTransition` | `motion` fade + 4px rise on route change, 150ms, honours reduced motion. Mounted once in the shell. | The only place `motion` is used for page-level presence. |

## Compatibility shims

`components/shared/page`'s `Section` renders a `Frame`; `shared/data`'s
`StatusBadge`, `ActionTypeChip`, `TechChip` render `Tag`; `Stat` re-exports
the system `Stat`. Existing feature components keep working and pick up the
new look; new code imports from `@/components/system` directly.

## Sizes

- Frame header 32px, Row 36/44px, Tag 20px, Toolbar 32px, Rail marker 20px.
- Type: title 12px/500 in frame headers, 13px body, 12px meta, 11px tags.
- Radius: 8px frames, 6px rail bodies and code, 4px tags.
- Lines: `ring-1 ring-border` (foreground at 7%). No shadows.
