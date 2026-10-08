# Design brief — Software Delivery Intelligence

## 1. Product identity

An engineering control plane for one person's GitHub account. It connects a
repository, understands the system (facts first, inference second), proposes
the next best delivery action with evidence, negotiates that proposal in plain
language, and executes the agreed plan one policy-gated step at a time.

Surface class: **product / dashboard** (operator persona). Not marketing.

## 2. Audience and the 60-second decision

A developer or lead who connected their own repositories. Their question on
every screen: *"what does the platform know, what does it recommend, and what
is it waiting on me for?"* Approvals and paused executions are the work queue.

## 3. Voice

Calm, specific, evidence-led. Sentence case everywhere. No exclamation marks,
no "AI magic" language. Every recommendation names its evidence; every failure
names its cause and the ways out. Em dashes stay out of UI strings.

## 4. Craft Read

> **Craft Read:** operator dashboard for a solo developer, product language,
> neutral cool-grey with one indigo accent (`--brand`, oklch hue 268),
> variance 4, signature bet: **the execution rail** — a vertical step rail
> with a live checkpoint marker and the paused step expanded in place, used
> identically on the execution page, the approvals queue and the overview.

## 5. Knobs

- CRAFT_LEVEL 7 · MOTION_INTENSITY 3 · VISUAL_DENSITY 7 · DESIGN_VARIANCE 4
- Composition: **Command (operator)** from `recipe-dashboard.md`.

## 6. Learned constraints (from the owner, 2026-10-08)

These override skill defaults.

- shadcn/ui only; compose its primitives, never hand-roll equivalents.
- Strokes, type sizes and colours must read as **smooth and light**: 1px
  hairlines at low contrast, no heavy borders, no saturated status fills.
- Frames are quieter than stock shadcn: smaller radii (8px cards, 6px
  controls), rings instead of shadows, lighter sidebar chrome.
- Every button that triggers async work shows its loader **inside the
  button** (`<Button loading>`), never a separate spinner next to it.
- Pages are `React.lazy` chunks behind Suspense; data via TanStack Query.
- Readable, nested component folders; no god components.
