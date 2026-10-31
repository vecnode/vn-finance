# Architecture

How the application is put together, and which decisions are load-bearing.

## The shape

Four crates and a binary, and dependencies point one way:

| Layer | Depends on | Owns |
| --- | --- | --- |
| `apps/vnfin` | all three | the clock, opening the vault, running the window |
| `crates/ui` | `core`, `store` | the theme, the widgets, the ten pages, the forms — **Actions in, model out** |
| `crates/store` | `core` | the vault: locations, atomic writes, ledgers, the document index, the lock, and the view model |
| `crates/core` | nothing of ours | the domain, with no I/O, no clock and no filesystem |

`core` cannot open a file, and that is kept true by review rather than by a compiler
flag — the way it holds is that every function that would need the world takes it as a
parameter.

## The one rule that shapes everything else

**The interface contains no tax logic.**

Every number on screen comes from `core`, through a single value assembled in
`store::model::build_model`. A page receives that value and may only format and
lay it out. A rate written as a literal in `crates/ui/` is a bug, and it is a bug
this project has already had once: the invoice form once defaulted the IRS
withholding to 25 %, which is not a Portuguese rate and contradicted the alert
engine beside it.

Two mechanisms keep it true:

1. **The pack is the only place the law is written down.** Rates, ceilings,
   deadlines and coefficients live in `rules/pt/2026.json`, compiled into the
   binary. A function that needs a rate calls `constant_bp(pack, &["iva", "rates",
   "normal"])`, and a missing value makes the calculation refuse rather than
   default.
2. **A page cannot write.** Every interaction returns an `Action` — "mark this
   handled", "save this profile", "register this invoice" — and `App::perform` is
   the single place that carries one out, through the session, against the vault.
   The model is rebuilt after every write, and the page redraws from the new one.

## The crates

### `crates/core` — the domain

Deterministic and total: given the same inputs it produces the same outputs, and
every function that depends on a date takes the date as a parameter rather than
reading a clock. That is what lets the obligation calendar be tested at any point
in a decade.

| Module | What it owns |
| --- | --- |
| `money` | `i64` cents, `i64` basis points, pt-PT formatting and parsing, half-away-from-zero rounding |
| `dates` | civil-date arithmetic on ISO strings, deliberately lax parsing, business-day moves |
| `nif` | the Portuguese modulus-11 check digit, and masking |
| `countries` | the EU-27, and what a country means for turnover |
| `types` | the domain vocabulary: profile, rules, sources, conditions, obligations |
| `rules` | loading, validating, canonicalising and checksumming the pack |
| `conditions` | `applies_when`: which fields a rule reads, and the first failure explained |
| `calendar` | turning the pack plus a profile into dated obligations |
| `profile` | the draft, the merge, and the validation messages |
| `estimate` | invoices, period totals, IVA, Segurança Social, the reserve |
| `flags` | the alert engine, and what the pack says about its own health |
| `pdf` | a small PDF reader: objects, pages, fonts, content streams, rows |
| `receipt` | reading a `fatura-recibo` out of those rows, and checking it |

### `crates/store` — the vault

The only crate that touches the filesystem. It owns four promises:

- **Atomic replacement.** A unique temporary name, `sync_all`, the rename, and a
  directory sync. The reference shared one `<file>.tmp`, so two writers could
  rename each other's bytes into place.
- **Synced appends.** `sync_data` after each JSONL line, so a power cut does not
  lose the invoice that was just recorded.
- **One writer at a time.** An advisory lock on the vault directory, held for the
  window's lifetime.
- **A cached size walk.** `describe()` used to walk every file on every model
  build; it now runs once and is invalidated by a write.

It also holds `model.rs`: the single view model the window renders, which is the
browser panel's `buildDashboard` minus the two blocks that died with the assistant.

### `crates/ui` — the window

egui, drawn the way the browser panel was laid out: the same 27 colour tokens in
light and dark (with the four contrast defects fixed and asserted), the same
spacing, the same cards, tables and status pills.

Two things are worth knowing:

- **The palette is tested.** `theme.rs` computes WCAG contrast for every text
  token against every surface it is used on, in both modes. The stylesheet this
  was ported from failed that in twenty places.
- **"It draws" is a test.** `crates/ui/tests/render.rs` runs one real egui pass
  per page through `Context::run_ui` — no window, no GPU, no display — and asserts
  each page produced its shapes, in both palettes, from a real vault. It runs in
  CI on three platforms. It cannot judge whether a layout looks right; it catches
  the page that panics, the column that goes negative, and the model field read
  when it is empty.

### `apps/vnfin` — the binary

Three things the libraries deliberately do not do: read the clock (in Europe/
Lisbon, from the EU's own summer-time rule rather than a time-zone database),
resolve and open the vault, and hand both to the window. A refusal — the folder is
inside a repository, another window holds it — is shown *in the window*, because
this build has no console to print to.

## The invariants

These are the things that must not break. They are checked by `cargo xtask ci`
where a machine can check them, and by review where it cannot.

| Invariant | How it is kept |
| --- | --- |
| No network code | `cargo xtask ci` scans the crates that link into the binary for HTTP APIs, and the lockfile for the crates that would add one |
| No `unsafe` in our code | `unsafe_code = "forbid"`, plus a source audit that fails on the word outside `unsafe_code` |
| European Portuguese, correctly accented | a mojibake audit over every source file |
| Money is integer cents | the type is `i64` everywhere; there is no float in `money.rs`'s public surface |
| Writes are append-only or atomic | `store::atomic`, and the vault has no other write path |
| The pack is the only law | no rate literal outside `rules/pt/2026.json`; the model carries the values the forms need |
| The interface cannot write | `Action` → `App::perform` → session, and the model is rebuilt |

`overflow-checks = true` is on in release, because a tax figure that silently wraps
is worse than one that panics.

## The parity harness

The application was a TypeScript program before it was a Rust one. The port was
checked against it, and the fixtures that did the checking are committed under
`crates/core/tests/golden/`:

| Fixture | Covers |
| --- | --- |
| `money.json` | formatting, parsing, rounding — including SHA-256 digests over a million values |
| `dates.json` | civil arithmetic, business days, quarters |
| `nif.json`, `countries.json` | the check digit, the EU-27, turnover scope |
| `rules.json` | pack loading, validation, constants, checksums |
| `calendar.json` | the obligation calendar across profiles and dates |
| `conditions.json` | `applies_when`, and the first failure explained |
| `profile.json` | drafts, merges, validation |
| `estimate.json` | invoices, quarters, Segurança Social, the reserve |
| `flags.json` | the alert engine |
| `pdf.json` | six real PDFs: the reader's rows and the parser's whole draft |

The Rust tests assert value for value. Where a sweep is too large to commit — a
million formatter inputs — the fixture carries the bounds and a SHA-256, so
neither side can quietly change the range.

Both implementations are gone from the tree now: the TypeScript one lives in the
git history at the commit named in `crates/core/tests/golden/README.md`, which is
where the fixtures came from.

## Data model

```
profile.json            TaxProfile                       (replaced atomically)
ledger/invoices.jsonl   Invoice, one JSON object per line (appended, synced)
obligations/completions.jsonl   CompletionRecord         (appended, synced)
documents/index.json    Vec<DocumentEntry>               (replaced atomically)
documents/<sha12>-<name>        the archived bytes
audit/audit.jsonl       AuditEvent                       (appended, synced)
.lock                   the advisory lock                (held while open)
```

Field names are camelCase on the wire — the vault is the same file format the
JavaScript implementation wrote, and a folder from that era opens here unchanged.
`serde_json`'s `preserve_order` is on so a file written by either implementation
has its keys in the same order.

`core` never sees a path, a `PathBuf` or a `std::io` type. That is what makes the
domain testable without a temporary directory, and it is why `store` is the only
crate that can create one.

## Where the seams are

- **A new page**: `crates/ui/src/pages/`, an entry in `Page::ALL`, a branch in
  `App::content`. If it needs a number the model does not carry, the number
  belongs in `build_model`, not in the page.
- **A new rule**: `rules/pt/2026.json`, with a source. Then check that
  `cargo xtask ci` still passes — the pack validator and the calendar fixtures
  will both have an opinion.
- **A new estimate**: a function in `crates/core/src/estimate.rs`, a field on the
  view model, and a widget. Not a calculation in a page.
- **Storage that is not JSON**: do not. The vault is readable with a text editor,
  and that is the point.
