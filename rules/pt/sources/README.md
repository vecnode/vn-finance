# The source corpus

The documents the rule pack cites, kept so that a citation can be checked on a
machine with no network. This folder is **evidence**, not input: nothing here is
compiled into the binary, nothing here runs, and no rate is ever derived from it. The
pack (`../2026.json`) remains the only place a figure is computed.

`fetch.json` is the record of how these bytes arrived: the URL each came from, the
date it was retrieved, its size, its SHA-256, and whether the body is stored here.

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

## What happens next (F1, not built yet)

1. Each `sources[]` entry in the pack gains `file` and `sha256`, and the rules that
   rest on it gain `citations` naming a passage instead of only a source id.
2. `core::sources::verify` fails when a hash does not match, a quoted passage is not
   in the stored document, or a `verified` rule has no local copy at all.
3. The Regras page shows, per obligation, which documents back it and whether the
   quote matched.

Until then this folder is inert: a person may read it, and the application does not.
