# The source corpus

The documents the rule pack cites, kept so that a citation can be checked on a
machine with no network. This folder is **evidence**, not input: nothing here is
compiled into the application, no rate is ever derived from it, and the pack
(`../2026.json`) remains the only place a figure is computed.

`fetch.json` is the record of how these bytes arrived: the URL each came from, the
date it was retrieved, its size, its SHA-256, and whether the body is stored here.
The prose record of how each figure was *found* is
[`docs/research/2026-sources.md`](../../../docs/research/2026-sources.md); this folder
is the bytes that record was written from.

## What is here, and what is not

| | |
| --- | --- |
| Stored in full | the 18 official documents — the AT calendar pages, the CIRS, CIVA and RITI articles, the two SEAF *despachos*, the AT folheto, the Diário da República page |
| Recorded only | `occ-guia-ti-2026`, `deco-ti-ss-2026`, `garrigues-ias-2026` — commentary, read on the retrieval date and not redistributed |
| Recorded as unavailable | `at-faq-art53-iva` returned 404 on 2026-11-18 |

Official texts are excluded from copyright by the *Código do Direito de Autor*
(art. 8.º), so the law itself is kept whole. A professional association's guide and a
commercial publisher's article are not, so for those the repository keeps the address,
the date and the hash, and F1 quotes the passage a value rests on instead of copying
the page.

## What the collection found

These are facts about the sources, not opinions about the law. They are the reason
the corpus is worth having.

- **One citation is dead.** `at-faq-art53-iva` — the FAQ for the art. 53.º exemption
  — answers 404. The pack cites it for the 15 000 EUR ceiling and the
  export-operations condition. A live URL is not a source; a stored copy is.
- **The IAS is still not read in the diploma.** `dr-portaria-480a-2025` returns a
  2.3 kB script shell, which is what the pack's own note says it found. The value
  (537,13 EUR) is corroborated by the OCC guide and Garrigues, both secondary. This
  is exactly the `constants.socialSecurity.iasMonthly` todo, unresolved.
- **Two gaps closed.** The *Despachos* n.º 68/2026 and n.º 81/2026 — which the pack
  records as "PDF não recolhido" — now download and are stored. F2 can bind them to
  the dates they extend.
- **The articles carry their text.** The AT article pages are SharePoint pages with
  the article inline, not script-only. `at-cirs-art101b` contains, verbatim,
  `sempre que o montante de cada retenção seja inferior a € 25`, which is what
  `constants.irs.withholding.art101b.minimumRetentionCents` (2500) rests on.
- **Extraction is not free.** The same page writes that sentence with `&#58;`
  entities, inline styles and hard line breaks inside the paragraph. F1's matcher has
  to decode entities and collapse whitespace before it compares, and it must keep
  accents: the law is Portuguese.

## How a citation is checked

F1 is built. For every source the pack **binds** — it carries a `file` and a
`sha256` — the application does three things at startup:

1. reads the stored bytes from `rules/pt/sources/`, beside the pack;
2. hashes them and compares with the `sha256` in the pack, which is the only thing
   that notices a document that changed after it was read;
3. extracts the text (tags stripped, entities decoded, whitespace collapsed) and
   looks for every `passages[].quote` the pack declares for that source.

The result is on the Regras page twice over: per source — `conferida`, `em falta`,
`sem hash`, `não confere`, or `sem cópia` for the sources the pack never promised to
keep — and per obligation, where a rule whose passage was found says `citação
conferida`. A quote that is not found, or a hash that moved, is an error that names
the passage and the file; `cargo xtask ci` fails on it too, through
`crates/core/tests/corpus.rs`, which runs this check over the shipped pack and this
folder.

## What is not built yet

- **Constants do not carry citations.** `citations` exists on obligations, so a rule
  says which passage it rests on. A constant — the €25 retention threshold, the IAS,
  the coefficients — is still bound to a *document* (`sourceIds`) and not to a
  passage. Its value is verified by the hash and by whatever passage that source
  declares, but the pack does not say "this number comes from this sentence". That is
  a schema change for a later version.
- **Four of the 22 sources have no copy here**: the three secondary publications
  (recorded, not redistributed) and the FAQ that answers 404. Their rows say
  `sem cópia`, and no rule claims to be verified against one.
- **Only eight passages are quoted so far**, from five articles. The other thirteen
  bound documents are hash-verified but not yet read by a machine. Adding a passage is
  two lines of pack and one `citations` entry; the yearly rollover that will want them
  all is F2, and it is not written yet.

Until someone adds them, this folder is inert beyond the hash check: a person may
read it, and the application only proves that what it holds is what was retrieved.
