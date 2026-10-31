# The parity fixtures

These JSON files were produced by the **TypeScript implementation** this application
was ported from, at commit `8efcc44` (`feat(web): alerts on two pages, newest
invoices first, no assistant`).

That implementation is no longer in the tree — it was deleted at the cutover — so
these fixtures are what remains of it, and they are why the Rust core is trusted:
`crates/core/tests/golden.rs` asserts, value for value, that every function answers
what the reference answered. Where a fixture is too large to commit (a sweep over a
million formatter inputs, a decade of calendar days), it carries the bounds and a
SHA-256 of the results instead, so neither side can quietly change the range.

| File | What it pins |
| --- | --- |
| `money.json` | formatting, parsing, rounding to the cent, rate arithmetic |
| `dates.json` | civil-date arithmetic, business-day moves, quarters, month labels |
| `nif.json` | the modulus-11 check digit, classification, masking |
| `countries.json` | the EU-27, and what a country means for turnover |
| `rules.json` | pack loading, validation, the canonical form and its checksum, constants |
| `calendar.json` | the obligation calendar across profiles, dates and tracking starts |
| `conditions.json` | `applies_when`: which fields a rule reads, and the first failure |
| `profile.json` | drafts, merges with an existing profile, validation messages |
| `estimate.json` | invoices, period totals, Segurança Social, the reserve, IRS base |
| `flags.json` | the alert engine, and the pack's health |
| `pdf.json` | six PDFs as hex bytes, with the reader's rows and the parser's whole draft |

## Regenerating them

You cannot, from this tree. The generator was `tools/golden/emit.ts` and it imported
`src/core/`, both deleted at the cutover. To regenerate:

```bash
git checkout 8efcc44 -- src tools package.json tsconfig.json
npm ci
node tools/golden/emit.ts
git checkout -- .            # discard the reference implementation again
```

The fixtures must then all still pass. If one changes, the port has drifted from the
reference — which is exactly the fact the file exists to make visible, and the
question to answer is "which of the two is wrong", not "update the fixture".

## Why they are hex and not a corpus of PDFs

`pdf.json` carries each document's bytes as a hex string rather than ten committed
`.pdf` files, so that the bytes a test parses are provably the bytes the reference
parsed: both sides read the same string out of the same file. The documents
themselves are invented — a name, a NIF that happens to satisfy the check digit, an
address — and were built by `src/core/receipt-fixture.ts`, which is also in the
history.
