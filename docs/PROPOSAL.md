# Proposal — vn-finance

*A local-first assistant for the Portuguese tax obligations of one self-employed
professional. This document is the design; `README.md` describes what is already
built and verified.*

---

## 1. The proposal in one paragraph

Build a single-user, local-first tool that owns the **calendar, the ledger and
the provenance** of one person's professional activity in Portugal. It knows
which CAE and CIRS situation you have, what you invoiced, what is due next, what
each obligation is worth, which documents must exist before a deadline, and
exactly which article of which code says so. It is deterministic where it must be
answerable (dates, arithmetic, applicability) and it uses an LLM only where
language is genuinely the problem (classification, explanation, drafting) — always
behind a redaction gateway, always with explicit approval, and never as the
source of a number or a date.

It is packaged the way `npx` packages things: one command, no server, no account.
The same core is designed to be wrapped in Tauri later for an application
without rewriting the domain.

---

## 2. What it replaces, and what it must never pretend to replace

**Replaces:** the spreadsheet, the mental load of remembering that a deadline
moved because the 20th fell on a Saturday, and the monthly "how much do I need to
set aside" anxiety.

**Does not replace:** a certified accountant, and the act of filing. This is a
design decision, not a limitation to be engineered away later. Three reasons:

1. **There is no public API for most filings.** Automating the Portal das
   Finanças would mean driving a browser session against a system that changes
   without notice, with credentials for a taxpayer's entire fiscal life. The
   failure mode is a silently missed or wrong declaration.
2. **Liability is asymmetric.** A tool that reminds you wrongly costs you a
   penalty. A tool that *filed* wrongly and told you it was handled is far worse,
   because you would stop checking.
3. **The honest value is comprehension.** Most of the benefit is knowing what is
   due, why, and what to keep — not avoiding the click.

So the tool **prepares, computes, checks, and explains**, and stops at the
portal. Where a deep link can save you navigation, it offers the link.

---

## 3. Design principles

These are the rules that decide arguments later. Each one is enforced somewhere
in the code, not just asserted in a document.

**P1. The law is data, never code.** Rates, ceilings, deadlines and their
citations live in versioned rule packs under `src/rules/<jurisdiction>/<year>.json`.
When the law changes, a data file changes and the diff is reviewable. No tax rate
appears in an `if` statement.

**P2. Every number carries its provenance, or it does not ship.** A rule cites a
source URL and a retrieval date. The loader warns when a rule claims to be
`verified` while citing nothing. `vnfin doctor` and `vnfin rules` surface the
count, and the interface marks unverified rules with `?`.

**P3. Published dates beat derived dates.** General legal rules and the
authority's annual calendar disagree regularly — for weekends, for holidays, and
through *despachos de prorrogação*. Both are recorded; the published date wins;
and when the published date is neither the rule's date nor that date shifted to
the next business day, the obligation carries a **discrepancy** for a human to
read. The engine never silently prefers its own arithmetic.

**P4. Refuse to guess.** When a rate, a ceiling or a legal basis is missing, the
calculation returns `null` and a stated limitation instead of a plausible number.
A wrong number in a tax tool is worse than a missing one, because the user cannot
tell the difference. This is why the reserve is a *range* and why the
simplified-regime estimate stops at taxable income rather than pretending to
produce "the tax".

**P5. Money is integer cents; rates are integer basis points.** 1% is `100`,
21.4% is `2140`, a coefficient of 0.75 is `7500`. Instalments split so that they
sum **exactly** to the total (`2 906,12 €` is `968,71 + 968,71 + 968,70`). The
loader flags any fractional rate as a probable bug, except IAS ratios where a
fraction is legitimate.

**P6. Nothing leaves the machine without an explicit, specific approval.** Not
"the user enabled AI"; the user approves *this* payload, having seen the
substitution list. The default is a dry run. Offline is a fully functional mode,
not a degraded one.

**P7. Boring, auditable technology.** Zero runtime dependencies. One network call
in the entire codebase (`src/ai/deepseek.ts`) — a claim a reviewer can check with
`grep -rn "await fetch(" src/`, which returns exactly one hit (the plain `fetch(`
grep also matches the comment that documents it). Append-only files for anything
that is a record. A folder you can copy for backup and delete for erasure.

---

## 4. Scope

### In scope for v1

- One taxpayer, one jurisdiction (Portugal), CIRS category B, services.
- The annual obligation calendar, derived from the rule pack, with statuses.
- Invoice ledger and expense ledger, in euros, with IVA and retention modelled
  per invoice, including intra-community reverse charge.
- Periodic estimates: per-quarter and year-to-date IVA, Segurança Social and the
  simplified-regime taxable base, each showing its own arithmetic.
- The document vault: what to keep, for which obligation, for how long, with a
  hash proving the file has not changed.
- Proactive warnings: the art. 53.º margin, retention rules for resident versus
  EU clients, coefficient applied, documents missing for a deadline.
- Alerts computed in code, plus a tightly bounded assistant whose only job is
  proposing updates to the rule variables that change over time (§8).

### Explicitly out of scope

- Filing anything, anywhere.
- Any connection to a public body: no Portal das Finanças integration, no login, no
  scraping, no browser automation, no webservice, and no sync of any kind.
- Multi-user, teams, or an accountant's practice-management view.
- Companies (IRC, Modelo 22, IES) — modelled in the pack so they can be shown as
  *não aplicável*, not implemented as a supported profile.
- Payroll, real-estate income, capital gains, non-resident taxation.
- Any other country. The architecture allows it (rules are namespaced by
  jurisdiction), but nothing else will be written until Portugal is right.

---

## 5. Architecture

![Architecture](dsh-resource://diagram/library/vn-finance-architecture)

Three layers, with a hard rule about which way dependencies point.

**Interfaces** (`src/cli.ts` today; a local panel and a Tauri shell later) know
about presentation and argument parsing. They contain no tax logic. Every figure
they print comes from the core.

**Core** (`src/core/`) is deterministic, pure where it matters, and never touches
the network:
`rules.ts` (load and validate packs) → `conditions.ts` (the declarative
applicability language) → `calendar.ts` (periods, due dates, discrepancies) →
`estimate.ts` (IVA, Segurança Social, IRS base) with `money.ts`, `dates.ts`,
`nif.ts` and `profile.ts` as the primitives.

**AI** (`src/ai/`) depends on the core, never the reverse. The core must not know
that an assistant exists.

**Store** (`src/store/vault.ts`) is a port. The current adapter is a folder of
JSON and JSONL files; a SQLite adapter can replace it without touching the core,
because the core never reads a file.

The tested interface between the law and the application is one JSON document. If
you can read the rule pack, you can audit the whole application's fiscal
behaviour without reading TypeScript.

### Target repository layout

The current single package is deliberate: it keeps the CLI runnable by Node with
no build step while the domain is still moving. When the panel lands, split into
a workspace without changing the core:

```
packages/core       src/core  + src/rules     (no I/O except reading packs)
packages/store      src/store
packages/ai         src/ai
apps/cli            bin: vnfin
apps/web            local panel on 127.0.0.1
apps/desktop        Tauri shell (later)
```

---

## 6. Rule packs: the law as versioned data

A rule pack is one JSON file per jurisdiction-year. It carries constants
(ceilings, rates, coefficients), the obligation rules, the published dates for
that year, and the sources. Money in cents, rates in basis points, and a stated
`verification` level per obligation.

```jsonc
{
  "id": "iva.dp.mensal",
  "kind": "declare",
  "authority": "AT",
  "tax": "IVA",
  "title": "Declaração periódica de IVA (regime mensal)",
  "appliesWhen": [{ "field": "iva.regime", "op": "eq", "value": "mensal" }],
  "periodicity": { "type": "monthly", "lagMonths": 2 },
  "deadlineRule": { "kind": "day_of_month", "day": 20 },
  "officialDates": {
    "2026": [
      { "month": 6, "day": 22, "note": "Dia 20 caiu a um sábado." },
      { "month": 7, "day": 20 }
      // August is absent: AT published no deadline for that month, which is the
      // month the June period would fall into under a two-month lag. The engine
      // therefore derives 20 August, marks it `derived`, and lists the year's
      // published dates in the discrepancy so a human can see the alternative
      // (the second quarter is in fact due 20 September under CIVA art. 41.º).
    ]
  },
  "legalBasis": ["CIVA art. 41.º"],
  "sourceIds": ["at-agenda-decl-2026"],
  "documentsToKeep": ["Comprovativo da declaração submetida"],
  "verification": "verified"
}
```

Three things about this design are load-bearing.

**Applicability is declarative, not executable.** `appliesWhen` is a list of
`{field, op, value}` conditions over a closed vocabulary of profile fields
(`iva.regime`, `irs.regime`, `profile.isCompany`, `activity.exports`, …). The pack
is data a user can edit; it is never `eval`'d and never used to build a property
path, so a malformed pack can be wrong but cannot be dangerous. When a rule does
not apply, the engine still emits the row with status `não aplicável` **and the
reason** — "ser uma sociedade é false (a regra exige true)" — because silently
hiding an obligation is how people fail to notice they misunderstood their own
situation.

**Provenance is enforced at load time.** `sourceIds` must resolve to a declared
source with an HTTP URL; a rule claiming `verified` with no source is a warning;
a rule with no legal basis is a warning. `pnpm`-free, dependency-free validation
in `rules.ts`.

**Year-specific dates are pinned, not inferred.** This is the part that a
"formula-only" design gets wrong. From the AT calendar for 2026 that I retrieved
on 2026-09-20:

| Obligation | 2026 dates | What the formula alone would have said |
| --- | --- | --- |
| IRS pagamentos por conta | 20/07, **21/09**, **21/12** | 20/07, 20/09, 20/12 |
| IVA declaração mensal, June period | **22/06** | 20/06 (a Saturday) |
| IVA declaração mensal, June period — August | **no published deadline** | 20/08 |
| Comunicação de faturas, July period | **31/08** | 05/08 |

Every one of those differences is real, and every one is the kind of thing that
gets a taxpayer a penalty. The pack records the published dates **and** the general
rule, the engine uses the published date, and when the two cannot be reconciled
the obligation carries a discrepancy note naming both.

A separate consequence: **next year's calendar does not exist yet.** The authority
publishes it weeks before the year starts. Until then the engine derives dates and
marks every instance `provisional`, and `freshness()` says so in plain words. An
application that confidently shows you 2027 deadlines in September 2026 is lying to
you.

---

## 7. The obligation engine, and how you trust it

`buildAgenda(pack, profile, {year, today})` returns a sorted list of
`ObligationInstance`s. It is pure: no clock (the caller passes `today`), no
network, no model. Same inputs, byte-identical output — and there is a test that
asserts exactly that.

The trust mechanism is a **golden test**. `src/core/calendar.test.ts` asserts that
the engine reproduces the dates the authority actually published: the three
pagamentos por conta dates, the 30 June Modelo 3, the weekend shift to 22 June,
the deliberate absence of an August deadline, and the August 31 extension for the
July communication. When a rule pack changes, a wrong date fails a test instead of
reaching the user.

Two more properties worth stating because they are easy to get wrong:

- **A standing duty is never "late".** "Keep your invoices for ten years" has a
  date for display but can never be overdue; it is a shelf, not a deadline.
- **Instalments are separate obligations, but a weekend shift is not.** When the
  authority publishes three dates months apart (pagamentos por conta), the engine
  emits three instances, because each is separately payable and separately late.
  When it publishes two dates *within a week* of each other (28 February and
  2 March, because 28 February is a Saturday), that is one deadline plus its
  shift, and emitting two payments would invent a debt. Dates within seven days
  are clustered, keeping the later one.
- **A duty with no date is not given one.** An event-driven obligation (a
  *declaração de alterações*) has no calendar date, so it does not appear in the
  agenda at all; `vnfin doctor` reports that it exists and `vnfin rules` shows its
  basis. Inventing a date for it would be a lie with a due date.
- **A tool adopted in September did not miss January.** Obligations due before the
  profile's `trackingStart` are reported as `untracked` — history, never a nag.
  Without this, a first run announces forty overdue obligations and teaches the
  user to ignore the alarm.

### What the research corrected

Worth recording, because it is evidence that the guardrails work rather than an
anecdote. The first draft of this design described the simplified-regime expense
rule the way most secondary summaries state it: a 15% deduction of documented
expenses, capped at €1 500. The rule pack's research found that the current text
of CIRS art. 31.º n.º 13 has **no cap at all** — it *adds back* to taxable income
the positive difference between 15% of gross service income and the eligible
expenses actually documented. The engine was rewritten to the add-back, and the
docs were corrected. The same pass also killed a startup-contribution "exemption
followed by a 50%/25% reduction ladder" that the sources do not support: it is a
twelve-month deferral, and the reduction ladder does not exist.

Note what happened before the correction: because the pack had no cap value, the
estimate **refused to produce a taxable income** rather than producing a
plausible wrong one. P4 is not decoration; it is the thing that kept a wrong
model from reaching the user in a number they could not check.

---

## 8. The assistant: a data function, not a conversationalist

An earlier draft of this design had a chat assistant that answered questions about
your year. It was cut deliberately, and what replaced it is narrower and more
useful.

**The assistant does exactly one thing: it proposes new values for the rule
variables that Portuguese law changes over time** — VAT rates, the art. 53.º
ceiling, the IRS coefficients, the IAS, the withholding rates, the Segurança
Social contribution rate — as structured JSON in the pack's exact units, each
with a source URL.

What it is *not* allowed to do, enforced in code rather than promised in prose:

| Not allowed | How that is enforced |
| --- | --- |
| See any of your data | `buildUpdateMessages` can only be built from a rule pack. There is no code path that puts a profile, an invoice, a client or an alert into a request. |
| Answer in prose | `parseUpdateResponse` requires JSON, refuses a fenced or prose-wrapped answer whole, and can only return values keyed to variables that were actually asked about. Prose cannot reach you through this path. |
| Invent a value | Every value passes a unit check (money in cents, rates in basis points, IAS multiples in hundredths) *and* a magnitude check. A VAT rate of 0,23 % is a syntactically valid basis-point integer, and it is rejected. |
| Make a value true | Applying a proposal records it as `ai-proposed` and `unverified`. It stays flagged in `vnfin doctor` and in the alerts until a human confirms it. The assistant proposes; it never verifies. |
| Change anything by itself | `update` without `--send` sends nothing. With `--send` it writes a proposal into the local vault and still changes nothing. `--approve` applies it. |

**Everything that looks like judgement is computed in code instead.** Red flags and
missing to-dos live in `src/core/flags.ts`: art. 53.º eligibility and the
15 000 / 18 750 € transition zone, Portuguese VAT charged to a foreign client,
withholding missing on a resident invoice or charged to a non-resident one,
duplicated invoice numbers, missing payment proofs, documents missing before a
deadline, overdue obligations. Every one of them is arithmetic on a field, so
every one is deterministic, citable and testable — and none of them needs a model.
That is not a cost saving; it is the reason the alerts can be trusted at all.

**Why the payload is safe to send.** The request contains variable paths, their
Portuguese descriptions, their current values from the pack, and the official URLs
the pack already trusts. That is public law. The redaction gateway from the earlier
design is still in the path — `DeepSeekClient.complete()` still accepts only a
`RedactedPayload` — but it is now defence in depth rather than the primary control,
because a stronger statement holds: **the assistant never receives your data at
all.**

**The loop, end to end:**

```
vnfin update                        # lists the variables; sends nothing
vnfin update --send                 # fetches a proposal, validates it, stores it in
                                    #   the vault, prints a diff: current → proposed
vnfin update --approve              # applies it to the pack, marked "por confirmar"
vnfin update --approve --verified   # you assert you checked each value in the source
```

Nothing is written to the pack until `--approve`, and nothing is marked verified
without a person saying so. The proposal is stored in the local vault, never in the
repository, so the values the assistant suggested are not in your git history
unless you decide they should be.

## 9. Domain model

```
TaxProfile ──┬── ActivityProfile (CAE entries, start date, category B, exports,
             │                    intra-community operations, invoicing software)
             ├── IVA regime (isento_art53 | mensal | trimestral)
             ├── IRS regime (simplificado | organizada) + coefficient (bp)
             └── Segurança Social (startup exemption window)

Invoice        number, date, client (name, NIF, country), base, IVA rate and
               treatment (iva_pt | autoliquidacao_ue | isento_art53 | exportacao),
               retention rate, ATCUD, status, proof-in-vault
Obligation     rule-derived instance: period, due date, source of the date,
               status, discrepancies, checklist, legal basis, portal link
Document       hash, kind, linked obligation, retention-until, added-at
RulePack       jurisdiction, year, version, checksum, sources, constants, rules
AuditEvent     append-only: what changed, when, and by which action
```

Derived, never stored: the agenda, all estimates, and every warning. Storing a
computed deadline would let it drift from the pack that produced it.

---

## 10. Interface

**The CLI.** `vnfin` with `init`, `doctor`, `agenda`, `flags`, `rules`, `estimate`,
`ledger`, `vault`, `ai-key`, `update` and `web`. `doctor` is the flagship: it
validates the profile against real constraints (an art. 53.º profile that declares
exports is an *error*, not a note), reports the pack's verification ratio and
missing constants, and states which parts of the picture are provisional.

**The local panel — built.** `vnfin web` serves a browser interface on `127.0.0.1`
from the same core, with no framework, no build step and no runtime dependency. The
stylesheet is lifted from the reviewed mockup (`design/dashboard.html`) so the two
cannot drift apart, and the whole front end is one HTML file, one stylesheet and
one ES module.

The architectural rule is the same one that governs everything else: **the panel
contains no tax logic.** One view model (`src/web/report.ts`) assembles everything
from the deterministic core — agenda, alerts, CAE/CIRS situation, ledger, the
Segurança Social derivation, the vault, the rule sources, the bounded update state
— and the browser only decides how to lay it out. Every write goes through the same
vault and the same audit log as the command line.

Serving someone's entire financial life on a port deserves more than "it is only
loopback", so the server binds to `127.0.0.1` and never `0.0.0.0`, validates the
`Host` header against the loopback names it was started with (which is what defeats
DNS rebinding), requires a per-run token on every API call, sends no CORS headers
at all, keeps every path inside `public/`, and imposes a content security policy
with no inline script and no external resource. The details, and what each control
is actually for, are in `docs/PRIVACY-SECURITY.md` §2.

The panel is deliberately not a web application: it is one machine's view of one
machine's folder, and it is unreachable from the network.

**Later: Tauri.** The reason the core is a separate layer with no I/O is that the
desktop application must wrap it rather than reimplement it. Tauri also gives the
one thing the browser cannot: proper OS credential storage for the API key.

---

## 11. Risks, and what is done about each

| Risk | Why it is real | Mitigation |
| --- | --- | --- |
| **Wrong tax data** | The whole value proposition collapses on one wrong rate | Rule packs are data, cited, versioned, checksummed; `verified`/`unverified` per rule; calculations refuse to run on missing constants; the calendar has a golden test against the published table |
| **Law changes and the pack goes stale** | Rates and ceilings change every January | `freshness()` compares the pack year to the current year and says the dates are provisional; packs are per-year so the diff is a reviewable file |
| **The authority publishes dates late** | Next year's calendar appeared in my research only as the 2026 one | Provisional marking is a first-class state, visible in the CLI and the interface, not a footnote |
| **The user trusts it too much** | A confident interface invites abdication | Discrepancies are surfaced, not hidden; estimates carry `estimativa` badges; the disclaimer is in the pack, the CLI and the UI; nothing is ever filed |
| **Privacy of a whole financial life on one machine** | Full ledger, client list, documents | Local-only, one folder, no telemetry, hash-addressed documents, append-only audit; the AI boundary is enforced by the type system |
| **Portal automation tempting later** | It would be a great demo | Excluded outright, in both directions: no login into any institution and no read-only integration either. The reasoning lives in the repository (§2). |
| **Scope creep into a general accounting package** | Companies, payroll, other countries | Non-goals are listed explicitly in §4 and the pack models inapplicable obligations as `não aplicável` |

---

## 12. Decisions taken

The scope questions from the first draft are settled, and the design changed to
match them:

1. **Turnover and IVA regime are user inputs, never defaults.** `init` refuses to
   run without `--iva`, and `--turnover-ano-anterior` is what the art. 53.º
   evaluation needs. When it is missing, the application names the check it cannot
   run instead of assuming a value — for the previous year and for the current
   year's estimate alike.
2. **Work outside Portugal is first-class.** Every invoice carries its client's
   country, and client income is split into national territory, EU and third
   countries, because the art. 53.º ceiling measures national territory only.
   Reverse charge, VIES and the recapitulativa have their own alerts, and
   Portuguese VAT on a foreign invoice is an urgent finding.
3. **Local to one machine, and structurally so.** No sync. The vault refuses to
   open a data directory inside this application's repository, because personal
   data must never be one `git add -A` away from publication; `.gitignore` covers
   the vault as a second line of defence.
4. **The assistant is a data function.** Only rule variables that change over
   time. No conversation, no reviews, no advice. See §8.
5. **No connection to any public body.** No Portal das Finanças, no Segurança
   Social, no login, no scraping, no automation, no read-only webservice. The pack
   records where an obligation is fulfilled as a reference for a human; the
   application never contacts it.

Still worth deciding before the panel is built:

- **A second copy of the document checklist.** The vault is on one machine with no
  sync, so a lost disk loses the record. That is a backup decision, not a feature.
- **How an annual pack update lands.** The current answer is as a reviewable git
  diff: packs are law, and a change in the law deserves to be read before it is
  believed. If that is too much ceremony for a personal tool, the alternative is
  applying it in place with the provenance recorded — which is what `--approve`
  already does.

## 13. Status

| Area | State |
| --- | --- |
| Rule pack schema, loader, validation, provenance checks | **Built and tested** |
| Obligation engine, discrepancy reporting, provisional years | **Built and tested** |
| Golden test against the published 2026 calendar | **Built, passing** |
| Money/rate discipline, exact instalment splitting | **Built and tested** |
| IVA, Segurança Social, simplified-regime base, reserve range | **Built and tested** |
| Local vault, append-only ledger, audit log, document hashing | **Built** |
| CLI (`init`, `doctor`, `agenda`, `rules`, `estimate`, `ledger`, `vault`, `ai-key`, `ask`) | **Built** |
| Alerts: art. 53.º zone, foreign-invoice VAT, withholding, documents, duplicates | **Built and tested** |
| Bounded assistant: variable extraction, JSON validation, unit and magnitude checks, provenance | **Built and tested** |
| Vault guard against a data directory inside a git work tree | **Built and tested** |
| Local panel: HTTP server, view model, front end, security guards | **Built and tested** |
| AI redaction gateway, branded payload, DeepSeek client, key storage | **Built and tested** |
| `pt/2026` rule pack content and citations | **Populated: 19 obligations, 20 sources (17 official), 6 fully verified and 13 partial, with every gap recorded in the pack's `todo`** |
| Local web panel | **Built** — `vnfin web`, served on `127.0.0.1` from the same core, with loopback binding, a Host allowlist, a per-run token, no CORS and a strict CSP |
| SAF-T / PDF import, Tauri shell, reminders | **Designed, not built** |

`npm run verify` — strict typecheck plus 91 tests — is the gate for all of it.
