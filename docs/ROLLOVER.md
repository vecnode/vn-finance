# The yearly rollover

What a new year of law costs, and how to pay it without inventing a date.

A pack is an annual artefact: the rates and the shape of each obligation carry forward,
the published calendar does not. The authority publishes next year's calendar weeks
before it starts, which means the work is always done at the last moment, by a person,
against a page that has just appeared. This document is the checklist for that moment.

Nothing here fetches anything. The application has no network code and neither does
the gate; the documents are collected by hand and committed, which is what makes the
result reviewable.

## The two commands

```bash
cargo xtask rules scaffold --from 2026 --to 2027 [--on YYYY-MM-DD] [--force]
cargo xtask rules diff 2026 2027
```

`scaffold` writes `rules/pt/2027.json` from the 2026 pack and **validates it before
writing** — a pack that the loader would reject never reaches the disk. `diff` compares
two packs and prints what moved; it is the same function the Regras page shows, so the
window and the terminal cannot disagree.

## What the scaffold does, and what it refuses to do

| | |
| --- | --- |
| Carries forward | every rule, every constant, every source binding (`file`, `sha256`), every quoted passage and every citation |
| Removes | every `officialDates*` block, at the root and inside each obligation |
| Sets | `year`, `packVersion` (`<year>.1.0`), `generatedAt`, and `verification: "stale"` on every rule |
| Rewrites | each `verifyNote` to say the rule was carried forward and not reconfirmed |
| Adds | a `todo` entry naming the sources that still have the old year in their id |

It does not transcribe a calendar, guess a rate, or mark anything verified. A rule
carried forward is a rule **not** confirmed.

## The checklist

1. **Wait for the calendar.** The AT publishes the next year's declarative and payment
   calendars in December or January. A scaffold built before that has nothing to
   transcribe.
2. **Scaffold.** `cargo xtask rules scaffold --from 2026 --to 2027`.
3. **Transcribe the published dates** into `officialDates2027`, per obligation, from
   the calendar itself. The scaffold's `todo` lists the sources whose id still says
   2026: those are the ones to re-fetch first. Note that the calendar publishes a
   *shifted* day when the legal day falls on a weekend — record the published day, not
   the legal one; the engine already explains the difference.
4. **Re-check every constant.** The IAS moves every year (a *Portaria*), and the
   coefficients, ceilings and rates are worth re-reading even when nobody expects them
   to move. Each change goes in the pack with the source that states it.
5. **Collect the new documents** into `rules/pt/sources/`, add `file` and `sha256` to
   the source, and quote the sentence each figure rests on as a `passages[]` entry.
   `fetch.json` records the retrieval; the README there explains what may be
   redistributed.
6. **Read the diff.** `cargo xtask rules diff 2026 2027` — every line is a claim that
   changed. A line nobody expected is the finding.
7. **Mark rules verified only after reading the source.** The scaffold sets every rule
   to `stale`; a rule goes back to `verified` when its own `verifyNote` says what was
   read and where.
8. **Run the gate.** `cargo xtask ci` — the pack validator, the calendar fixtures, the
   corpus check and the privacy scan over the sources all have an opinion. Two of those
   fixtures follow the pack by construction: `golden/rules.json` pins its checksum, version
   and `todo`, and `golden/calendar.json` a digest over every agenda instance. A new pack
   therefore means re-baselining them — `UPDATE_GOLDEN=1 cargo test -p vnf-core --test
   golden` — and reading that diff rather than accepting it: every line of it is a date or
   a page somebody will now read differently. `crates/core/tests/golden/README.md` says
   what the run may and may not touch.
9. **Commit.** One commit for the pack, one for the documents if they are separate
   concerns, and the diff output in the message where it is long.

## Do not ship a scaffold

A scaffolded pack is a starting point, not a year. It says `2027` while every rule is
`stale` and every date is derived, so releasing it **loses** the verification the 2026
pack had without adding any of 2027's. Keep it out of the repository and out of a
release until step 5 is done; the file is one command away and nothing depends on it
existing early.

## A worked example

`cargo xtask rules scaffold --from 2026 --to 2027` on the 2026 pack, then `rules diff`:

```
2026 2026.1.4 → 2027 2027.1.0
verificação
  ~ irs.modelo3: verificada → desatualizada
  ~ iva.dp.mensal: verificada → desatualizada
  … 18 more, every rule carried forward
datas publicadas
  ~ irs.modelo3: 1 → 0
  ~ irs.pagamentosPorConta: 3 → 0
  ~ iva.dp.mensal: 11 → 0
  … 13 more
36 alteração(ões).
```

Every one of those 36 lines is the scaffold doing its job: the rules are no longer
claimed verified, and no published date came along. After step 3 the `datas publicadas`
lines read `0 → 12` and the count of changes falls to what actually changed.

## What changes every year, and what does not

- **Every year:** the calendar (declarative and payment dates, including shifts and
  *despachos de prorrogação*), the IAS, the year-specific sources.
- **Some years:** the IVA rates, the withholding rates, the simplified-regime
  coefficients, the ceilings — by law, not by calendar.
- **Almost never:** the shape of an obligation. When it does change, the pack needs a
  new `deadlineRule` kind or a new condition field, and that is a code change with a
  test, not a data edit.

## When the law changes, not just the date

A new figure is a new passage: quote the sentence, cite it from the rule, and let the
corpus check prove it is in the copy. If a rule's *shape* changes — a deadline that
becomes a window, an obligation that gains a condition — the change belongs in
`crates/core/` with a test, and the pack only carries it afterwards.
