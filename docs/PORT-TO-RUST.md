# Porting vn-finance to pure Rust + egui

The plan this repository is being built from, and the record of what is done. It
supersedes `ROADMAP.md` M6: the desktop application is a Rust rewrite, not a Tauri
wrap.

## The decisions this plan rests on

| Decision | Choice | Why |
| --- | --- | --- |
| Assistant layer | **dropped** | The rule pack is a reviewed repository artifact. Dropping the layer removes TLS, AES-GCM and scrypt from the tree, and lets CI *prove* the binary has no socket. |
| Interfaces | **one GUI binary**, no CLI | The browser panel was always the primary interface; `doctor`, `agenda` and `estimate` are pages. `--json`, `ledger import`, `vault add` and `estimate --despesas` retire with the console. |
| Repository | **same repo**, root Cargo workspace | `src/` stays as the reference implementation and the golden-fixture generator until the cutover. |
| Licence | **AGPL-3.0-only** | Matches vn-ranger. |
| Navigation | **sidebar**, no URL | A `Page` enum, the last page remembered, `--page <id>` for a shortcut. Deep links and the back button were browser properties. |
| Vault | **unchanged on disk** | An existing `~/.vn-finance` opens as-is. That is what makes the port abandonable at any point. |

## End state

One `vnfin` binary per platform. No runtime, no browser, no port, no token, no
network listener. The same ten pages, the same Portuguese copy, the same numbers,
light and dark following the OS. Release archives carry `vnfin`, README, LICENSE,
THIRD-PARTY-NOTICES and SHA256SUMS, built by CI on tag for Windows, Linux and
macOS.

## Architecture

```
crates/core   vnf-core    pure domain: no I/O, no clock, no environment
crates/store  vnf-store   vault I/O, locations, atomic writes, the view model
crates/ui     vnf-ui      egui: theme, fonts, widgets, icons, the ten pages
apps/vnfin    vnfin       the window, the action loop, "today in Lisbon"
xtask                     cargo xtask ci — exactly what CI runs
```

`core` depends on `serde`/`sha2` only. `ui` draws and returns `Action`s; the app
executes them against the vault and rebuilds one `DashboardModel`, which is the
browser panel's "every write returns the whole model" turned into a type.

## What changes, what is lost

| Today | Tomorrow |
| --- | --- |
| `node:http` server, Host allowlist, per-run token, CSP, CORS refusals, body caps | gone — there is no listener to defend |
| `web`, `init`, `rules`, `flags`, `ledger *`, `vault *`, `ai-key *`, `update` | gone; their subjects are pages |
| uploads over HTTP | drag and drop, plus the in-window folder browser the panel already had |
| URL hash routes, back button | `Page` enum, remembered last page |
| print stylesheet | lost; the printable year pack stays on the roadmap |
| DeepSeek key, redaction, update flow (1 043 lines, 21 tests) | gone with the layer |

## Persistence: same format, four fixes

`profile.json`, `ledger/invoices.jsonl`, `obligations/completions.jsonl`,
`audit/audit.jsonl`, `documents/index.json`, `documents/<sha12>-<name>` and
`~/.vn-finance/vault-location.json` keep their exact shapes — camelCase keys, and
`serde` **must not** deny unknown fields, because that JSON is a contract with
every vault already in existence.

The port fixes what the reference implementation got wrong:

1. **No locking anywhere** — the document index is a whole-file read-modify-rewrite,
   so two processes silently lose entries. Add an advisory lock; a second launch
   asks the running window to come forward.
2. **`writeJson` shares one `<target>.tmp`** — use a unique temp, `sync_all`,
   rename, fsync the directory.
3. **No fsync on the append-only ledgers** — `sync_data` after each append.
4. **`describe()` walks the whole vault on every model build** — cache it,
   invalidate on write.

Two more, found while porting the core:

5. **`validateProfile` reads the clock** (`monthsSince` defaults to `new Date()`),
   so the twelve-month exemption warning depends on the day the test runs. In Rust
   it takes `today` as a parameter.
6. **`apply_bp`'s float path** becomes integer arithmetic in `i128`; the two are
   provably identical while `|amount × bp| < 2^53`, and the bound is tested.

## Safety and purity, as checks

- `unsafe_code = "forbid"` workspace-wide; `missing_docs = "warn"`.
- clippy `deny`: `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`,
  `dbg_macro`.
- `overflow-checks = true` in release: money that wraps in silence is a wrong
  figure nobody notices.
- `cargo xtask ci` scans the shipped crates for networking APIs and the lockfile
  for crates that would provide one; it also refuses an `unsafe` outside
  `unsafe_code` and any mojibake in a Rust source.
- No C dependencies on Windows or macOS. Linux brings the platform's
  windowing/graphics ABI — the same position as vn-ranger.
- No timezone crate: the core takes `today`; the binary computes Europe/Lisbon from
  the EU DST rule, tested against known dates.

## Testing: parity first

`cargo xtask ci` runs formatting, clippy with warnings denied, the whole suite, and
the two audits.

**The golden harness is the backbone.** `tools/golden/emit.ts` walks the reference
`src/core/` and writes `crates/core/tests/golden/*.json`; the Rust tests assert
against it value for value, with digests over sweeps whose bounds travel in the
JSON. Done for M1: a million amounts through the currency formatter and back
through the parser, a decade of calendar days, the applyBp grid, the whole basis
point range through both display helpers.

Fixture-byte parity for the PDF reader: `fixtures.rs` regenerates the invented
`fatura-recibo` and asserts its SHA-256 equals the TypeScript-built one, then the
reader extracts identical rows and the parser identical fields.

The bar for the engine: the smoke profile (`NIF 123456789`, start 2019-04-01, IVA
trimestral, today 2026-09-20) yields **74 instances** — 7 `not_applicable`, 45
`official`, 29 `derived` — with identical ids.

## Milestones

| # | Milestone | State |
| --- | --- | --- |
| M0 | Workspace, lints, toolchain, xtask, CI, `.gitignore` | **done** |
| M1 | `money`, `dates`, `nif`, `countries` + the golden harness | **done** |
| M2 | `types`, `rules` (pack embedded and validated), `conditions`, `calendar` | **done** |
| M3 | `profile`, `estimate`, `flags` | **done** |
| M4 | `vnf-store`: vault, locations, atomic writes, lock, view model | **done** |
| M5 | `vnf-ui`: theme (27 tokens × light/dark), fonts, icons, widgets, Resumo | **done** |
| M6 | the other nine pages, the profile modal, onboarding, the vault chooser | **done** |
| M7 | `pdf` + `receipt` + fixtures, the receipt flow, the invoice form | **done** |
| M8 | release workflow for three platforms, THIRD-PARTY-NOTICES, docs rewrite, cutover | **done** |

### What M2 cleared

The obligation engine reproduces the reference exactly, and the harness now proves
it rather than asserting it: **16 calendar scenarios** (each IVA regime, a company,
EU operations with and without the 50 000 EUR declaration, exports, employees,
uncertified software, the AT webservice, the start-up exemption, two completed
obligations, a tracking start, a holiday, an unpublished year, and a year before the
pack) compared on the full id list, every field of every instance through a
SHA-256, the status counts and the dashboard window. Plus the pack itself: its
checksum, summary, the complete validator problem list, freshness, and the typed
reads of eleven constants.

The harness earned its keep twice in this milestone:

- **`residentPT`**, not `residentPt`. `rename_all = "camelCase"` lower-cased an
  initialism, and a profile read with the wrong key loses a declaration silently.
  A vault written by the TypeScript application would have opened with
  `residentPt: false`.
- **The condition reason's wording.** The reference takes an optional label map and
  falls back to the wire name, so `explainFailure(profile, [condition])` says
  `iva.regime` while the calendar says `regime de IVA`. The first Rust version
  always used the label. `FieldLabels::{Raw, Portuguese}` now makes the distinction
  explicit and both callers are checked against the reference.

### What M3 cleared

`profile`, `estimate` and `flags` are ported: creating and editing a profile with
its three-state optional fields, importing one, the missing-input list, the
consistency checks; invoice totals, default terms, period totals, the Segurança
Social contribution with its three instalments, the quarter report, the
simplified-regime IRS base, the reserve range; and the twenty flag codes with their
Portuguese prose.

The harness covers it: 8 profile builds (including two refusals and the
edit-keeps / null-deletes pair), 9 imports, the missing inputs and the problem list
for 10 profiles, 6 invoice-total sets, 15 default-term combinations (three profiles
by five countries), 4 period windows, 3 Segurança Social incomes, 8 quarter
reports, 4 IRS bases, 4 reserve ranges, 13 flag scenarios compared as whole JSON
documents, and the pack-health flags for three dates.

Two more things the harness caught:

- **A rate I assumed instead of reading.** The first test asserted 25 % retention
  for resident professional services; the pack says 23 %. The pack is the only
  place a rate is written down, so the test now reads it and asserts it is
  positive, rather than repeating a remembered figure.
- **A flag detail that quotes a CLI command.** The art. 53.º flag tells the person
  to run `vnfin init --turnover-ano-anterior`. That command will not exist after
  the cutover, but rewriting it now would have diverged from the reference and
  weakened the comparison. It is reproduced verbatim and listed below for the text
  sweep.

### What M4 cleared

`vnf-store` is the only crate that touches the filesystem: location resolution with
the reference's precedence, the data-dir risk check, atomic writes, the append-only
ledgers, the document archive, an advisory lock, and `build_model` — the view model
the window will render.

**The migration claim is now tested, not asserted.** `tools/golden/vault.ts` writes a
vault with the **reference implementation's own `Vault` class** into
`crates/store/tests/fixtures/vault-ts/` — a profile, two invoices, a completion, an
archived PDF with its index entry, and the audit trail those writes left behind.
`crates/store/tests/legacy_vault.rs` opens that folder and asserts the profile, both
invoices, the completion, the five audit actions, the index entry and its invoice
link, the archived bytes hashing to the indexed hash, and that `build_model` renders
the whole picture from it (agenda with the completed instance as `done`, Q1 and Q2
totals, the reserve at 25 % of the year's invoicing). It also copies the fixture,
writes into the copy, and asserts the shapes the reference relies on survive: one
compact JSON object per line, a pretty-printed profile with a trailing newline.

The four fixes the plan promised, in code:

1. **Unique temporary names, `sync_all`, rename, directory sync** (`atomic.rs`), with
   a test that no `.tmp` is left behind and a replacement never merges.
2. **`sync_data` on every append**, so a recorded invoice survives a power cut.
3. **An advisory lock** on `<vault>/.lock` (`fs4`), refused for a second writer and
   released when the handle closes.
4. **The size walk is cached** and invalidated by a write.

Two deliberate divergences, both recorded rather than silent:

- `ensure()` creates `ledger`, `obligations`, `audit` and `documents` — not the
  reference's `ai/` (the assistant is gone) or `exports/` (nothing ever wrote there,
  and a folder that exists to be empty is a promise the application does not keep).
- The model's fifth guarantee now reads "não abre uma única ligação à Internet",
  because the reference's sentence described a rule-pack update request that no
  longer exists. The About page renders whatever the model carries.

### What M5 cleared

The window exists. `vnf-ui` carries the palette (27 tokens × two modes, with the four
contrast defects fixed and asserted), the type scale, the eleven line icons drawn with
the painter, fourteen widget builders taken from the stylesheet's own measurements,
the rail with its counters, the header, and the Resumo page with real figures. `vnfin`
opens it, reads the clock for Europe/Lisbon, opens the vault, and writes the audit
line.

Three things are worth recording:

- **The palette is tested, not eyeballed.** `theme.rs` computes WCAG contrast for every
  text token against every surface it is used on, and for the status colours against
  their own tints, in both modes. The stylesheet failed that in twenty places; the port
  cannot regress into it.
- **"It draws" is a test.** `crates/ui/tests/render.rs` runs one real egui pass per
  page through `Context::run_ui` — no window, no GPU, no display — and asserts each
  page produced its shapes, in both palettes, from a real vault the test seeded. That
  runs in CI on three platforms. What it cannot judge is whether it looks right; what
  it catches is a page that panics, a width that goes negative, or a model field read
  when it is empty.
- **The window was also opened for real**, against a temporary vault: it stayed up,
  took the lock, wrote `vault.opened` and closed cleanly.

One API surprise worth writing down for anyone reading this later: egui 0.37 replaced
`ctx.run(...)` and `TopBottomPanel`/`SidePanel` with a root-`Ui` model —
`eframe::App::ui(&mut self, ui, frame)`, `egui::Panel::left/top/bottom`, and
`Context::run_ui`. The old examples on the internet do not compile.

Deliberate gaps at this milestone:

- **Fonts are egui's built-in ones**, not Inter and JetBrains Mono. Vendoring them means
  committing two OFL files (~500 KB) plus their licence; the type scale, the tracking
  and the weights are all in place, so dropping the TTFs into `assets/fonts/` and
  registering them in `theme.rs` is the whole remaining change.
- **Navigation is the rail** and Ctrl+1..0, with no remembered page across launches
  yet: that arrives with the UI state file, which has not been built.
- The nine pages other than Resumo say they are being ported, rather than showing an
  empty card that would read as a bug.

### What M6 cleared

All ten pages exist and render from the same model. What is worth recording is not the
list of pages but the four decisions inside them:

- **A table is a widget, not a grid of labels.** `widgets::table` takes columns, items
  and a closure that fills cells in order, so a row cannot drift out of step with its
  heading, and every table in the product shares one set of hairlines, one heading
  treatment and one right-alignment rule for money. Nine pages use it.
- **A page still cannot write.** Every interaction returns an `Action`, and the window
  performs it through the session — including the one write on the agenda
  ("Marcar tratada") and the profile save. `App::perform` is the single write path, and
  a test drives it exactly as a click does.
- **The profile form validates twice, in two places.** The form catches what a text
  field makes possible (a bad check digit, a date that is not one, a turnover that is
  not money); `validate_profile` catches what only the domain knows (an activity that
  started in the future, a regime that contradicts the turnover). Neither is redundant.
- **A page that needs the profile says so once**, in `pages::needs_profile`, instead of
  five near-identical paragraphs drifting apart on five pages.

Deliberate gaps at this milestone, all of them things a person would notice:

- **There is no form to record an invoice.** The ledger page reads what is in the
  vault; entering a receipt by hand arrives with the PDF reader in M7, which is the
  sane way round — the reader fills the form rather than the form being built twice.
- **No vault chooser.** `location.rs` has the browser (`browse_directories`,
  `starting_points`) and the model carries the pointer path, but nothing opens a
  folder picker yet: that needs a native dialog, which is a dependency decision for M7.
- **Page state is not remembered.** The agenda filter and the chosen quarter reset on
  launch. A UI-state file is a one-afternoon job and belongs with the window's
  geometry, which is also not persisted.
- Fonts are still egui's built-in ones, as recorded under M5.

One process note for whoever works here next: **do not edit source with
`Get-Content -Raw | Set-Content` or `WriteAllText` in this shell.** It reads UTF-8 as
the ANSI code page and writes it back double-encoded, which turns `exercício` into
`exercÃ­cio` — and the source audit catches it as a mojibake finding, after which a
`WriteAllText` with a failed expression truncated three files to zero bytes. The edit
tool round-trips UTF-8 correctly; the shell does not.

### What M7 cleared

The PDF reader and the `fatura-recibo` parser are ported, and the parity harness now runs
them on real bytes: `tools/golden/emit.ts` emits six documents — a standard fatura-recibo,
one whose total does not add up, one with a retention, one with no issuer section, one
issued by somebody else, and an image-only PDF — with the reference's own extraction and
its whole parsed draft. `golden/pdf.json` carries the bytes as hex, so a test in Rust
parses exactly what the reference parsed, and asserts the **entire draft** value for
value: rows, text, problems, every field, every check and its arithmetic, every formatted
amount. It passed on the first run.

Four decisions inside that:

- **`miniz_oxide` for inflate.** The reference used `node:zlib`, which is C behind a
  JavaScript API. This is the only place the application has to decompress anything, and
  a pure-Rust DEFLATE keeps the "no C in the shipped binary" property without hand-rolling
  an inflate next to a tax parser. Recorded as a deliberate divergence.
- **The bytes are parsed as bytes.** The reference decoded the file to a `latin1` string,
  where one byte is one character by construction. Rust gets that structurally: the
  cursor, the strings, the content streams and the CMaps are all `&[u8]`, and text appears
  only where a person reads it.
- **No regex engine.** Every label pattern became a function answering "does this row
  start with this label", with the boundary test spelled out. Two things the reference's
  regexes did that are easy to lose, and are now tests: a pattern must match at the START
  of a row (`IVA` also appears inside `VALOR IVA`), and `TOTAL` must not swallow
  `TOTAL A PAGAR`.
- **The receipt flow is three steps.** Read the document, archive it, show what was read
  **including what could not be read**, let the person correct it, then write. The PDF is
  archived before the fields are confirmed, because the archive is what proves the invoice
  exists later.

The window's side is tested end to end:
`a_pdf_imported_through_the_window_lands_in_the_ledger` feeds one of the golden PDFs
through `Action::ParseReceipt`, checks the form filled with the document's date, client,
rate and base, confirms it, and asserts the ledger grew by one line with the right base,
rate, retention and `paymentProofInVault`. Its sibling feeds the document issued by
somebody else and asserts **nothing** was written and the refusal names the reason.

Still open, and recorded rather than hidden: the vault chooser (a native folder dialog is
a dependency decision), remembered window geometry and page state, and vendored fonts.







## Risks

1. **The PDF reader** (1 016 lines: lexer, xref-free object scan, four-step inflate
   retry ladder, cp1252, `/ToUnicode` in both forms, affine text matrices, baseline
   grouping). Port it structurally, not conceptually; the fixture bytes and the
   nine receipt cases are the judge.
2. **Byte offsets in a latin-1 world.** `pdf.ts` decodes the file as latin-1 so
   string index equals byte offset. Keep `&[u8]` throughout; a UTF-8 `String` has
   UTF-8 indices and breaks every offset silently.
3. **Absent vs null.** `intra_community_operations_above_50k`, `tracking_start` and
   the profile draft's "empty field deletes" are three places where collapsing the
   two changes tax output.
4. **The formatter.** pt-PT does not group a four-digit integer and uses U+202F.
   Pinned by golden digests, because a `format!` that groups `1000` breaks every
   fixture and every screenshot.
5. **Sort order is behaviour.** The flags comparator is not a total order in the
   reference; Rust needs an explicit one, and the order must be reproduced because
   the Resumo page prints it.
6. **egui cannot reproduce CSS.** No cascade, no grid auto-flow, no letter-spacing
   on a plain label, no dotted stroke. "Same UI" means the same information,
   hierarchy, type relationships and palette — not identical rows.

## Deliberate divergences from the browser panel

- Light `--text-4` becomes `#5B6779` (was `#8A97A8`, 2.7–3.0:1 on every surface);
  dark becomes `#8B9CB3`. `--text-3` darkens by one step. Control borders move to
  a 3:1 boundary. The six 9.5–10.5px labels are raised to the stylesheet's own 11px
  floor.
- A manual light/dark switch, alongside following the OS.
- Copy buttons where the panel had none.
- The vault lock and the second-launch behaviour above.

## Text to sweep at the cutover

Carried verbatim from the reference so the parity fixtures stay honest, and wrong
the moment `src/` is deleted:

| Where | Says | Should say |
| --- | --- | --- |
| `flags.rs`, `ART53_NO_TURNOVER_INPUT` | "Regista-o com `vnfin init --turnover-ano-anterior <valor>` ou editando o perfil no cofre." | point at the Perfil form only |
| `profile.rs`, twelve-month warning | "Segurança Social Social Direta" (a duplicated word) | "Segurança Social Direta" |
| `flags.rs` — anywhere else a `vnfin` subcommand is quoted | — | grep `vnfin` across `crates/` |

The first two change a golden fixture, so they are one edit each plus a
regenerated fixture, done once nothing needs to match the reference any more.


### What M8 cleared, and the cutover

The repository is now a Rust workspace and nothing else. `src/`, `tools/`,
`package.json`, `package-lock.json`, `tsconfig.json` and `node_modules` are gone, and
with them the `legacy` CI job that kept the TypeScript suite green while it was still
the reference. The last commit that contains them is `8efcc44`, recorded in
`crates/core/tests/golden/README.md` because the golden fixtures cannot be regenerated
without it.

What replaced them:

- **`cargo xtask notices`** writes `THIRD-PARTY-NOTICES.md` from `cargo metadata` —
  397 crates with their own licences — and `cargo xtask notices-check` runs inside
  `cargo xtask ci`, so a dependency change that nobody documented fails the gate. A
  hand-written licence list drifts the first time a dependency changes, and a licence
  file that has drifted is worse than none: it is a claim nobody checked.
- **`.github/workflows/release.yml`** builds three artefacts, each shaped for the
  platform that will run it: Windows with `+crt-static` (verified: the binary has no
  import of `vcruntime140`, `msvcp140` or `ucrtbase`, so it runs on a machine with no
  redistributable installed), macOS as one universal binary fused with `lipo` from both
  architectures, and Linux on `ubuntu-22.04` so the glibc it needs is the one an LTS
  has. Each archive carries `LICENSE`, `THIRD-PARTY-NOTICES.md` and `README.md` beside
  the binary, plus a `.sha256`. Publishing is gated on the same `ci.yml`, through
  `workflow_call`.
- **`scripts/run.bat`** used to start a server, read its tokenised address out of a log file and
  hand it to Chrome. It now finds `vnfin.exe` beside itself and runs it — which is the
  whole of what a double-click needs to do.
- **`README.md`, `ARCHITECTURE.md`, `AGENTS.md`, `SECURITY.md`** rewritten for a window
  instead of a browser tab: the six promises (the fifth is now "does not open a single
  network connection"), where the vault lives, the three download files, and the two
  invariants a contributor has to know before touching a number.

### What is still not built

Recorded here rather than discovered later:

- **The vault chooser.** `location.rs` has the directory browser and the model carries
  the pointer path, but nothing opens a folder dialog: that needs a native file dialog,
  which is a dependency decision nobody has made. The vault can still be moved by
  setting `VN_FINANCE_DATA_DIR` or by writing the pointer file.
- **Vendored fonts.** The window uses egui's built-in faces. Inter and JetBrains Mono
  are two OFL files and one registration call in `theme.rs`, and they would make the
  window match the design's type exactly.
- **Remembered state.** The window opens at a fixed size on the Resumo page, and the
  agenda filter and chosen quarter reset at each launch.
- **Encryption at rest.** Unchanged from the reference: the vault is plain JSON and PDF,
  protected by the disk's encryption and file permissions.
### Two decisions taken after the cutover

**The `pdf` crate was not adopted.** `pdf-rs/pdf` (the `pdf` crate, v0.10.0) is MIT,
pure Rust, and a *reader* — its own description is "PDF reader", and the writing
half moved to `pdf-canvas` years ago. Adopting it would replace 1,300 lines that
already reproduce the reference's output on six committed documents, in exchange for a
different API, a global object cache, an mmap path and a worker pool, and it would not
give the ability to write a PDF. When this application needs to *emit* a PDF — a
receipt, an export — the crate to add is a writer (`lopdf`, MIT, pure Rust), which is a
far smaller surface than a reader and answers the question that was actually asked. The
reader here stays, and `deny.toml` would need no change.

**`README.md` is in European Portuguese** and follows the shape of `vn-ranger`'s: the
badge row, the build commands, the data folder, the documentation table. It is the
user-facing document; `ARCHITECTURE.md`, `AGENTS.md` and this file stay in English.