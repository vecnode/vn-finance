# Roadmap

From the verified skeleton to an application you can rely on for a full tax year.

Each milestone has an **exit criterion** that can be checked without a meeting.
The dependency order matters: the domain is right before the interface is
pretty, and the interface is right before the desktop shell exists.

---

## M0 — Correctness core · **done**

Rule-pack schema with provenance validation; obligation engine with published-date
precedence and discrepancy reporting; money/rate discipline; IVA, Segurança Social
and simplified-regime estimates that refuse to guess; local vault with an
append-only ledger; the CLI; the AI redaction gateway with a branded payload type;
`pt/2026` rule pack; 54 tests including a golden test against the AT 2026 calendar.

**Exit criterion:** `npm run verify` is green, and the engine reproduces the
published 2026 dates. ✔

---

## M1 — Rule packs you can trust as data

- Complete the `pt/2026` pack: every remaining `TO CONFIRM` value resolved against
  an official source, or explicitly recorded as unresolved.
- Ship the bounded update flow as the way packs are maintained year to year: the
  assistant proposes values, a human confirms them, and every applied value carries
  its provenance.
- Add a `pnpm`-free `vnfin rules --verify-urls` mode that re-fetches each source and
  reports which ones moved or disappeared.
- Add the PT public-holiday list per year, so the engine's business-day adjustment
  stops being weekend-only. Holidays are never guessed.
- Property test: for every rule, the derived date and the published date must
  agree, or the rule must declare why they differ.

**Exit criterion:** zero `TO CONFIRM` items in the pack without a recorded reason;
`vnfin doctor` reports the verification ratio and no unverified rule is silently
used.

---

## M2 — Invoicing and expenses for a real year

- Complete the invoice model: line items, IVA by rate, retention, ATCUD and QR
  fields, credit notes (*notas de crédito*), payment tracking.
- Expense ledger with the categories that matter under the simplified regime, and
  the documented-expenses deduction computed only when the cap is known.
- Sequential numbering per series, with a check that no number is reused or
  skipped — the failure that gets invoices rejected.
- CSV import from a bank statement, matched to invoices.

**Exit criterion:** a full year of real invoices and expenses can be entered, and
the quarterly and annual totals reconcile with the portal's own figures to the
cent.

---

## M3 — The document vault and the reminder loop

- Link each document to the obligation it satisfies; show what is missing *before*
  the deadline, not after.
- The document checklist per obligation, from the rule pack's `documentsToKeep`.
- Retention periods per document class, once the legal period is verified.
- Local notifications (OS-native, no cloud) at 30/14/7/1 days, and an escalation
  for overdue obligations.
- Encrypted export of the whole vault for backup, and a verified restore path — a
  backup that has never been restored is not a backup.

**Exit criterion:** every obligation in a year has its required documents attached
or an explicit "missing" state, and a restore from export reproduces the vault
hash-for-hash.

---

## M4 — The local panel · **done (core of it)**

Built: an HTTP server on `127.0.0.1` with no framework and no dependency, the
single view model that feeds it, and the panel itself — agenda, alerts, CAE/CIRS
situation, the "inputs only you can supply" form, the invoice ledger, the Segurança
Social derivation, the quarter reports, the document vault, the rule sources and
the bounded rule-update flow. The server's guards (loopback binding, Host
allowlist, per-run token, no CORS, CSP, path containment) are covered by tests, and
the stylesheet is lifted from the reviewed mockup rather than re-invented.

Closed the remaining command-line-only steps, so a browser-only user never needs a
terminal: the panel now carries the `doctor` diagnostic (profile validity, missing
inputs, pack freshness and verification, vault size, git risk, verdict), its own
form to store, unlock, replace or delete the DeepSeek key — with the key held in the
server's memory when it is not saved — a file picker that uploads a document instead
of asking for an absolute path, and the `estimate --despesas` calculator for the
simplified regime's taxable income. Every write still goes through the same vault,
the same validation and the same append-only audit log, and `ARCHITECTURE.md` keeps
the command-by-command equivalence table.

That calculator has since left the panel, and the hand-typed invoice form with it.
Both asked for something the vault could not prove — an expense figure nothing
records, and an invoice with no document behind it — and the panel now has one door
into the ledger: the `fatura-recibo` PDF, imported, read, checked and confirmed.
The two capabilities remain on the command line, and `ARCHITECTURE.md` lists them as
deliberate omissions rather than letting them read as oversights.

Two things followed from using it. **The folder is now chosen, not assumed**: on a
first run with an empty vault the panel asks where the cofre should live, offers a
navigator limited to directories, remembers the answer in
`~/.vn-finance/vault-location.json` and can open the folder in the file manager —
because `~/.vn-finance` is a folder the person who owns the data cannot find, and a
data store nobody can find is a data store nobody backs up. And **the panel became
a set of pages rather than one long scroll**: each subject has its own tab and its
own address (`#painel`, `#agenda`, `#iva`, `#cofre`, …), so a tab can be linked and
the back button works, and the dashboard is the first one. IVA and IRS were one
seven-column table and are now two pages, because "what I hand the State", "what my
clients already handed it for me" and "the base next year's assessment uses" are
three questions and a single table answered none of them clearly.

The typography changed with them. The first version fitted a whole tax year into
13px with 9.5px uppercase micro-labels, which is not dense, it is unreadable.
Nothing a person has to read is below 12px now, page titles are the largest text on
screen, and every page opens with one sentence saying why it exists.

Still to add:

- **A printable year pack** — agenda, ledger summary, document checklist and
  per-quarter reconciliation as one printable artefact, the thing you hand your
  accountant. The print rules are already in the stylesheet.
- **Trend and comparison views** — this year against last, quarter against quarter,
  once there is more than one year of ledger to compare.
- **A keyboard-first path through the agenda** — marking obligations handled without
  a mouse, for people who live in a terminal all day.

**Exit criterion (met for the core):** a full year can be operated from the panel
alone, with the CLI remaining a first-class equivalent interface — every write in
the panel goes through the same vault, the same audit log and the same validation
as the command line.

---

## M5 — Import instead of typing · **started**

- **Done: a `fatura-recibo` PDF.** A text-layer reader in `src/core/pdf.ts` and a
  form parser in `src/core/receipt.ts` read the document, check its own arithmetic
  (base + IVA + stamp duty = total, total − retention = payable, rate × base = IVA),
  archive the PDF in the vault under its hash and register the invoice in the
  ledger — after the person confirms the reading in the panel. Nothing is
  uploaded, there is no OCR, and a document whose issuer is not the profile's NIF
  is refused rather than recorded as income. CLI: `vnfin ledger import <pdf>`.
- Still to do: SAF-T (PT) XML import of issued invoices, and e-fatura CSV/XML
  export import.
- Still to do, and blocking the other direction: the **expense ledger**. Until it
  exists, an invoice somebody issued *to* the taxpayer is archived and explained
  rather than recorded, because there is nowhere correct to put it.
- If the text layer fails (a scan, an image-only PDF, a font with no character
  map), the file is NOT uploaded anywhere and NOT guessed at: it stays archived in
  the vault and the failure is reported as a task for you.
- Opening an obligation's page in your own browser stays your action, not the
  application's. The pack cites the URL; nothing here contacts it.

**Exit criterion:** an existing year's invoices can be imported and reconciled
without manual retyping, and no import path performs a network call at all.

---

## M6 — Desktop application (Tauri)

- Wrap the core and the panel in Tauri; no domain logic is rewritten.
- Move the DeepSeek key into the OS credential store (Windows Credential Manager /
  DPAPI, libsecret, Keychain) and retire the passphrase file.
- OS-level encryption at rest for the vault, with auto-lock.
- Signed builds, and an updater that updates the *application* — never silently
  updates a rule pack, because a rule change deserves a reviewable diff.

**Exit criterion:** the same vault, unchanged, works in the CLI and the desktop
application, and the key never exists in plaintext on disk.

---

## Deliberately not on this roadmap

- **Filing anything.** See `docs/PROPOSAL.md` §2.
- **Browser automation of the portal.** Same reasoning; the fragility-to-benefit
  ratio is bad and the failure mode is invisible.
- **Multi-user, teams, accountant practice management.**
- **Other countries.** The architecture allows it; Portugal comes first.
- **Cloud sync, of any kind.** Decided against: one machine, one folder, no
  account. If that ever changes it is a different project, not a later milestone.

---

## Sequencing note

M1 comes before M2 on purpose. Building an invoicing UI on top of rates that are
still `TO CONFIRM` would produce a tool that looks finished and is wrong — the
exact outcome this project exists to avoid. The order is: get the law right, then
the ledger, then the reminders, then the interface.
