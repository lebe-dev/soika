# Design guidelines

Soika's UI is built on shadcn-svelte / bits-ui primitives, restyled into a
quieter, lighter look ("v2"). This document describes that look so new screens
stay consistent with it. Tokens live in `frontend/src/app.css`; the restyled
primitives live in `frontend/src/lib/components/ui/`.

## Principles

- **Quiet surfaces, one confident colour.** Neutrals do the layout work; the
  primary blue marks only what is active or actionable (primary button, active
  nav item, focus ring, selected tab).
- **Hairlines instead of boxes.** Separate content with 1px borders and
  whitespace, not shadows or nested cards. Shadows appear only on hover, on
  the active segment of a segmented control, and on popovers.
- **Hierarchy through colour, not size.** Body and descriptions are one step
  lighter (`text-muted-foreground`), not smaller. Headings use semibold weight
  with slightly negative tracking.
- **Mono for machine-made values.** Slugs, environment variable names, DSNs,
  culprits, counters in badges and table column labels use the mono font.
- **Numbers line up.** Counts and times use `tabular-nums`.

## Colour

The primary hue is fixed and must not change:

| Token       | Light                  | Dark                   |
| ----------- | ---------------------- | ---------------------- |
| `--primary` | `oklch(0.61 0.17 252)` | `oklch(0.68 0.15 252)` |

Neutrals and status colours:

| Token                | Light       | Dark        | Use                                    |
| -------------------- | ----------- | ----------- | -------------------------------------- |
| `--background`       | `#f8f9fb`   | `#0d0f13`   | Page canvas                            |
| `--card`             | `#ffffff`   | `#14171c`   | Surfaces: cards, lists, inputs         |
| `--popover`          | `#ffffff`   | `#1a1e24`   | Menus, dialogs                         |
| `--foreground`       | `#14171c`   | `#eceef2`   | Primary text                           |
| `--muted-foreground` | `#5f6570`   | `#8f959f`   | Secondary text, descriptions, labels   |
| `--muted`            | `#eef0f3`   | `#1a1e24`   | Segmented-control track, soft pills    |
| `--accent`           | `#f1f3f6`   | `#20242b`   | Hover background                       |
| `--border`           | `#e6e8ec`   | `#23272e`   | Outer borders of surfaces and controls |
| `--hairline`         | `#f0f1f4`   | `#1b1f25`   | Dividers inside a surface (rows)       |
| `--destructive`      | red, L 0.55 | red, L 0.68 | Errors, unresolved counts, removal     |
| `--success`          | green       | green       | Healthy / resolved states              |

Rules:

- Use the semantic Tailwind classes (`bg-card`, `border-border`,
  `border-hairline`, `text-muted-foreground`, `text-destructive`,
  `bg-success`), never raw hex values in components.
- Status tints are the status colour at 10% (`bg-destructive/10 text-destructive`).
- Avatar tints come from `$lib/avatar.ts` (`avatarTint(id)`): one deterministic
  soft hue per entity, so a project keeps its colour everywhere.
- Text must reach 4.5:1 contrast against its background in both themes.

## Typography

- **Sans:** Onest (variable, Latin + Cyrillic) — `font-sans`.
- **Mono:** JetBrains Mono (variable) — `font-mono`.
- Both are bundled through `@fontsource-variable/*` and imported in
  `routes/+layout.svelte`. Do not load fonts from external CDNs: the SPA is
  embedded in the binary and must work offline and behind strict proxies.
- The root size is 15px, so `rem` values are slightly smaller than Tailwind's
  defaults.

| Role                          | Style                                                          |
| ----------------------------- | -------------------------------------------------------------- |
| Page title (`PageHeader`)     | `text-[1.75rem] font-semibold tracking-[-0.025em]`             |
| Entity title (`EntityHeader`) | `text-[1.65rem] font-semibold tracking-[-0.025em]`             |
| Card / section title          | `text-[15px] font-semibold tracking-[-0.01em]`                 |
| Body                          | `text-sm`, foreground                                          |
| Description                   | `text-sm text-muted-foreground`                                |
| Column label                  | `font-mono text-[11px] uppercase tracking-[0.08em]` muted      |
| Machine value                 | `font-mono text-xs` muted                                      |

## Shape and spacing

- **Radius:** `--radius` (~8px) for controls (`rounded-lg`); surfaces use
  `rounded-xl` (~12px); inner chips and segments ~7px. Avatars: people are
  circles, projects and teams are rounded tiles.
- **Heights:** controls are `h-8`; list rows are at least 48px; the top bar is
  `h-14`.
- **Content width:** the `container` utility caps content at 1264px.
- Group siblings with `flex`/`grid` and `gap`, not margins.

## Components

- **Button** — `default` (primary fill with a faint inner highlight) for the
  single main action of a view; `outline` (card surface + hairline) for
  secondary actions; `ghost` for icon buttons and tertiary actions;
  `destructive` (tinted) for removal. One primary button per area.
- **Input / NativeSelect** — card surface, hairline border, primary border plus
  a soft 3px halo on focus. An input holding an active filter gets
  `border-primary/50 ring-primary/15 ring-3` so applied filters are visible.
- **Card** — card surface, 1px border, `rounded-xl`, no shadow. `Card.Footer`
  is a plain hairline-separated action row, not a grey band.
- **SegmentedControl** (`$lib/components/segmented-control.svelte`) — for
  mutually exclusive view options (sort order, status filter). Prefer it over
  ad-hoc button groups.
- **Tabs** — the default variant is a segmented track; page-level navigation
  (project sections, top bar) uses a 2px primary underline on the active item
  and no icons.
- **Badge** — `rounded-md`, `h-5`. Issue status uses `IssueStatusBadge`.
- **StatPill** — compact `h-6 rounded-md` summary chips under entity titles.
- **Avatar** — `shape="tile"` for projects and teams, `circle` for people;
  always pass `id` so the tint is stable.

## Patterns

- **List page** (`/`, `/teams`, `/admin`): `PageHeader` with title, a one-line
  description and actions; a toolbar row (search, filters, then a
  `SegmentedControl` pushed right with `sm:ml-auto`); then the content.
- **Detail page** (project, team, profile): `EntityHeader` with back link,
  tile avatar, title and stat pills; underline tabs for sub-sections.
- **Project cards** (dashboard): identity row (avatar, name, mono slug,
  favourite) and a hairline-separated footer that carries the one number that
  matters — the unresolved count in large tabular digits, or a green dot with
  "No unresolved issues".
- **Tables / issue list**: a card-surface container with a mono uppercase
  header row, `border-hairline` row dividers, hover `bg-accent/60`, numbers
  right-aligned with `tabular-nums`. Wide tables scroll horizontally inside
  their container (`overflow-x-auto` + `min-w-*`).
- **Severity**: a small dot plus a mono label. Only `error`/`fatal` are red and
  `warning` is amber; everything else stays neutral.

## Dark theme

Dark is the default mode. It is near-neutral (not blue-black): surfaces step
up in lightness (`background` < `card` < `popover`) and borders stay faint.
The primary button uses dark text on the lighter primary. Check every new
screen in both themes.

## Accessibility

- Use real `<button>`, `<a href>`, `<input>` + `<label>`; icon-only buttons
  need `aria-label`.
- Mark the current nav item or tab with `aria-current="page"` and toggles with
  `aria-pressed`.
- Do not convey state by colour alone: pair status colours with a label or icon.
- Interactive targets are at least 32px; keep focus rings visible.

## Don'ts

- No gradients, glows, emoji, or coloured left-border cards.
- No nested cards inside cards; use hairline rows instead.
- No new hues for decoration; new colours must come from the token set.
- Do not change `--primary`.
