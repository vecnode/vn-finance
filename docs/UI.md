# The interface

The window's rules, in one place. `crates/ui/src/theme.rs` holds the values and
`crates/ui/src/widgets.rs` the shapes they are used in; this file says what those values
are for and why they are what they are, so a change to one of them is a decision somebody
can read rather than a number that drifted.

The colours, spacing and type began as a transcription of the reviewed mockup in
`design/`, and the four places the window deliberately differs from it are listed in
`theme.rs`. Everything below is later than the mockup: the window was reviewed, and these
are the answers.

## One palette, one window

White, and only white. There is no switch and no dark mode: `theme::apply(ctx, false)`
sets the light palette and pins the theme preference to `Light`, so a desktop in dark mode
cannot repaint the window on the next frame. The dark palette stays in `theme.rs` because
the contrast tests measure it — a table of colours nobody checks rots.

Every text colour clears WCAG AA (4.5:1) on every surface it is used on, and the control
boundaries clear 3:1. `theme::tests` fails the build if a pair is added that does not.

## Type

| Token | Size | What it is for |
| --- | --- | --- |
| `FIGURE` | 24 | The one number a page is about: the quarter's contribution. |
| `FIGURE_SMALL` | 18 | A figure a step down, inside a card. |
| `FIGURE_ROW` | 16 | One of several figures listed in a column (see *Figures are rows*). |
| `SECTION_TITLE` | 17 | A section heading on a page. |
| `PAGE_TITLE` | 16 | The application's name in the window header. |
| `BODY` | 13.5 | Prose, and the subject of a table row. |
| `BODY_SMALL` | 12.5 | Secondary prose, a table's caption line. |
| `CAPTION` | 12 | A note under something. |
| `MICRO` | 11 | Uppercase, tracked: headings of tables, and labels of figures. |
| `MONO` | 11.5 | Money in a table cell, dates, hashes. |

Nothing is set below 11 points — the stylesheet this was ported from says so in its own
header and then used 9.5 in six places. Numbers in a table are monospace so that columns
of them line up digit for digit.

Consistency rule: a figure is never larger than the thing it is a figure *of*. If a number
appears in a list with others it is `FIGURE_ROW`; if it is the page's answer it is
`FIGURE`. A number never uses `BODY` — that is what a sentence is made of.

## Layout

- **The header** is the application mark and two lines: `vn-finance`, then
  `Page — subtitle`. Nothing else lives there: the folder, the exercise year, the pack's
  counts and today's date are on Diagnóstico, where somebody asking about them can read
  them properly. The mark is a `theme::MARK`-point square from `brand.rs`, drawn from the
  same bytes as the window icon; it is the application tile and not the bare crystal,
  because a tile reads at that size and needs no second piece of artwork for the theme
  (`assets/logo/BRAND.md`).
- **The rail** lists the pages in three groups, ends with **Perfil**, and does not scroll.
  It is 264 points wide. The privacy lines stay pinned at its foot.
- **A page starts with its own heading** — except where the window's header already states
  it, as on IVA, where the page would otherwise say "IVA" twice in two centimetres.
- **A page is never wider than its column.** The window scrolls down and never sideways.
  Tables scale their columns, long headings truncate, and prose wraps; a horizontal
  scrollbar is not a way of hiding a layout that does not fit. `no_page_paints_past_the_
  right_edge` measures every page at the smallest window and at a maximised one.
- **The window's minimum is 920 × 700.** The height is measured from the rail's own list
  (`the_rail_fits_in_the_smallest_window`), not guessed: add a page and that test fails
  before the eleventh button hides under the foot of the rail.
- Page margins are 20 points (`CONTENT_MARGIN`); the content is capped at 1440 points on a
  very wide screen.

## Tables

`widgets::table` is the only table in the window, and it has rules worth keeping:

- **Columns are fixed widths** taken from the page. A flexible column (`Column::new`) takes
  what is left, and the client or obligation column is usually the flexible one.
- **12 points of gutter** between columns. The widths are computed from the space left
  after the gutters, so a row's cells and its heading line up to the point.
- **Every cell starts at the top of its row** (`horizontal_top`). A row whose client name
  carries a second line does not push its money to the middle.
- **The whole set scales** when the window is narrow, in proportion, rather than
  overflowing.
- **Optional columns step out** (`Column::optional`) when keeping them would shrink every
  column past three quarters of its authored width. The invoices table drops IVA and
  Retenção on a small window; both are already in the figures above it.
- Headings are `MICRO`, uppercase and tracked. Money is right-aligned and monospace.
  Legal citations are clipped to one line with the rest on hover — a decree that amended
  another decree three times is nine lines in a column with room for two.

## Figures are rows, not cards

A page that states several numbers states them as rows inside one bordered block: the name
in a `MICRO` label, the sentence saying what the number counts, where it came from in a
caption on the left, and the number alone at `FIGURE_ROW`, right-aligned, in a column of
its own so the numbers line up with each other.

They were five cards side by side, which read as five competing headlines, squeezed their
labels into each other on a narrow window, and put the shorter answers under longer
questions.

## Small vocabulary

- **chip** — a filter or a choice, clickable, 24 points high.
- **pill** — a status, read-only, in a table cell.
- **badge** — metadata, smaller and quieter than a pill (an id, a verification state).
- **severity** — the tag that opens an alert row: a painted mark (diamond for urgent,
  triangle for attention, dot for everything fine) and a word. The mark is a shape, not a
  character: the window's fonts have no dependable coverage for `▲`, and a missing glyph is
  drawn as a box.
- **defs** — a two-column definition list, 172 points of labels.
- **accented card** — a card tinted by tone, for the one thing worth saying first.

## Dialogs

A modal — the profile form, the PDF import — is a form, not a menu: it carries 28 × 24
points of inner padding, so the text is never against the border, and it is the only place
the window uses a shadow.

## What the window does not do

No network code, no telemetry, no account, no server: the promise is in the rail's foot and
the About page, and it is kept by the dependency tree rather than by a setting. The
interface cannot write: a page returns an `Action` and the window performs it.
