# The Portuguese domain

What the application has to know about a self-employed professional in Portugal,
and — just as importantly — **how sure we are of each fact**.

## How to read this document

Every factual claim carries one of three marks, and the mark is mirrored in the
rule pack by each obligation's `verification` field:

- **VERIFIED** — retrieved from an official source (Portal das Finanças, Diário
  da República, AT folhetos), with the URL recorded in the pack.
- **PARTIAL** — the deadline or figure is confirmed from the authority's own
  published calendar, but the article that establishes it was not read in its
  consolidated text. The pack marks these `partial`, and the agenda prints `?`
  beside them.
- **TO CONFIRM** — stated by corroborating secondary sources (OCC *Guia
  Prático*, DECO, Garrigues) but not read in the primary text. The pack records
  the value *and* says so.

Nothing in the second or third category is presented to the user as settled. The
convention exists because a personal tax tool has exactly one serious failure
mode: being confidently wrong. Where a value is missing entirely, the
calculation **refuses to run** rather than guessing — see
`estimateIrsSimplifiedBase` for the worked example.

---

## 1. The identity of the activity

| Concept | What it means | Status |
| --- | --- | --- |
| **CAE** | *Classificação Portuguesa das Atividades Económicas*. The activity code(s) registered with the AT. A programmer is typically `62010` (atividades de programação informática), with `62020` (consultoria em informática) as a secondary code. | VERIFIED as a code list; the codes for a given taxpayer come from their own declaração de início |
| **CIRS categoria B** | Income from independent activity — the *recibo verde*. Without an open category B activity there is nothing to invoice. | VERIFIED |
| **Início de atividade** | The declaration that opens the activity and fixes the CAE, the IVA regime and the estimated turnover. It is the source of truth for everything downstream. | VERIFIED |
| **Regime de IRS** | *Simplificado* (default) or *contabilidade organizada* (mandatory above the ceiling). | Ceiling: **€200 000** (CIRS art. 28.º) — PARTIAL |

The application holds this as a `TaxProfile` and validates it against real
constraints rather than mere formatting. Two of those constraints are errors, not
warnings, because getting them wrong is a real exposure: an art. 53.º IVA
exemption combined with declared export operations, and the same exemption
combined with non-resident status.

### The figures the application refuses to guess

Two numbers drive everything about IVA and neither is defaulted or inferred:

- **The declared IVA regime** (`isento_art53`, `trimestral` or `mensal`). `vnfin
  init` refuses to run without it. It is the enquadramento of your *declaração de
  início de atividade*, and guessing it would silently change every downstream
  obligation.
- **Turnover in national territory for the previous civil year**, which is what
  the art. 53.º ceiling actually measures. Until you supply it, the application
  does not evaluate the exemption at all; it says which check it cannot run. The
  same applies to the current year's estimate, which is what the live
  transition-zone alert watches.

Work outside Portugal is not a flag on the profile but a property of each invoice:
every invoice carries its client's country, and income is split into **national
territory**, **EU** and **third countries**, because only the first counts towards
the art. 53.º ceiling while the other two change the VAT treatment, the VIES
registration and the *declaração recapitulativa*.

---

## 2. IVA (VAT)

**Rates (VERIFIED, CIVA art. 18.º n.º 1, mainland):** normal 23%, intermediate
13%, reduced 6%. The autonomous regions may apply reduced rates under art. 18.º
n.º 3; those are not in the pack, so an Azores or Madeira profile is out of scope
and the pack says so.

**Regime especial de isenção — artigo 53.º CIVA (VERIFIED, retrieved
2026-09-20):**

- Conditions: the taxpayer must (i) carry out **no export operations or connected
  activities**, and (ii) not have exceeded **€15 000** of annual turnover *in
  national territory* in the previous civil year.
- Requires domicile or seat in national territory; it does not apply to taxpayers
  without national domicile even with a permanent establishment.
- In the year of commencement the estimated turnover is **not annualised**: it is
  the predicted value from the start date to the end of the civil year.
- It does not apply to occasional operations (*ato isolado*) or to intra-community
  supplies of new means of transport.
- **Exit transition (documented, not coded):** according to the AT's *início de
  atividade* folheto, exceeding €15 000 while staying below €18 750 requires a
  *declaração de alterações* within 15 working days of the end of that year, with
  the normal regime applying from 1 January; exceeding €18 750 during the year
  brings IVA forward to that moment. The pack records this in its `todo` because
  the schema has no field for a two-threshold transition yet.
- Source: Portal das Finanças FAQ *IVA > Enquadramento Legal > Reg. Especial
  Isenção Art 53º* and Ofício Circulado n.º 25062 (DSIVA, 2025-03-26).

The practical consequence: **the €15 000 ceiling is a number to watch all year**,
not a checkbox. The panel shows the distance to it and warns early, because
crossing it changes the invoicing model, not just the amount.

**Periodic regimes:** quarterly below the ceiling set by CIVA art. 41.º —
recorded in the pack as **€650 000** (PARTIAL) — and monthly above it.

**Reverse charge and intra-community services:** services to a business in another
member state are generally not subject to Portuguese IVA (the client
self-assesses), require a valid VIES registration, and bring the *declaração
recapitulativa* into play. Income from clients outside Portugal does not count
towards the art. 53.º national-territory ceiling, which is why the profile tracks
client country per invoice rather than a single "foreign" flag.

---

## 3. Retenção na fonte (withholding at source)

Clients established in Portugal withhold a percentage of the invoice and pay it
to the State; you receive the net and the withholding is credited against your
final IRS assessment.

| Situation | Rate | Status |
| --- | --- | --- |
| Professional services, resident (CIRS art. 101.º) | **23%** | VERIFIED |
| Other category B income, resident | **11,5%** | VERIFIED |
| Non-resident (CIRS art. 71.º n.º 4 a)) | **25%** | VERIFIED |
| Intellectual or industrial property | 16,5% | NOT CODED — the schema holds three rates; recorded in the pack's `todo` |
| Income under art. 58.º-A EBF | 20% | NOT CODED — same reason |

Intra-community and export invoices do not suffer Portuguese withholding. The
application models this per invoice (`retentionBp`, defaulted from the client's
country and editable) and reconciles the total withheld against the year, because
a mismatch between withheld and declared is one of the most common causes of an
IRS surprise.

The **deadline** for delivering the withheld amounts (day 20 of the following
month) comes from the AT's published payment calendar (PARTIAL: the article that
fixes it was not read in a consolidated text).

---

## 4. IRS — the simplified regime

Under the simplified regime you do not deduct real expenses from real income. A
**coefficient** is applied to gross income to obtain the taxable base.

| Parameter | Value | Status |
| --- | --- | --- |
| Coefficient, professional activities of Tabela 4 | **0,75** | VERIFIED (CIRS art. 31.º n.º 1; Tabela do art. 151.º) |
| Coefficient, other service income | **0,35** | VERIFIED (CIRS art. 31.º n.º 1) |
| Turnover ceiling for mandatory organised accounting | **€200 000** | PARTIAL (CIRS art. 28.º) |

### The documented-expenses rule is easy to get backwards

This is the correction the research forced on the design, and it is worth stating
precisely, because the widespread summary — "15% of expenses, capped at €1 500" —
is **not what the current text says**.

**CIRS art. 31.º n.º 13 has no monetary cap.** The mechanism is an **add-back**:

```
addition to taxable income = max(0, 15% × gross service income − eligible documented expenses)
taxable income            = (gross income × coefficient) + addition
```

So the law *presumes* expenses equal to 15% of your gross service income; if you
can document less than that, the shortfall is added back and you pay on more. If
you document more, nothing is added. The €1 500 figure that circulates in
secondary summaries is not in the current text.

The engine implements the add-back
(`estimateIrsSimplifiedBase`), reports the reference, the documented amount and
the addition separately, and states the assumption out loud in its output.

Eligible expenses are the listed categories: specific deduction, staff costs,
rents, 1,5%/4% of the VPT of properties, other expenses communicated to the AT,
and imports and intra-community acquisitions. The application takes the eligible
total as an input rather than trying to infer it from a ledger, because
eligibility is a judgement.

### The rest of the year

- **Pagamentos por conta** — three payments on account, published in 2026 as
  **20/07, 21/09 and 21/12**. The three dates encode the legal rule (day 20) plus
  two weekend shifts: 20 September 2026 and 20 December 2026 are both Sundays.
  Not exigible when the amount is below €50 (CIRS art. 102.º n.º 2).
- **Modelo 3** with **Anexo B** (simplified regime) and **Anexo SS**, plus
  **Anexo J** for foreign income. The 2026 deadline is **30 June** (VERIFIED), and
  the rule is expressed as a window — 1 April to 30 June — of which the end is the
  deadline.

**The application does not compute the tax.** Applying the art. 68.º brackets
requires the year's rate table with its brackets and the solidarity surcharge.
Until that table is part of the rule pack, the estimate stops at taxable income
and says so. Producing a "your tax is €X" figure from an incomplete model is
exactly the failure this project exists to avoid.

---

## 5. Segurança Social

An independent worker contributes on a *relevant income* that is a share of gross
income, declared quarterly and paid monthly.

| Parameter | Value used | Status |
| --- | --- | --- |
| Contribution rate | **21,4%** | PARTIAL (OCC *Guia Prático*, Aug 2026; corroborated by DECO) |
| Relevant income share, services | **70%** | PARTIAL (same) |
| Relevant income share, goods | **20%** | PARTIAL (same) |
| Base of incidence | 1,5 × IAS to 12 × IAS | PARTIAL (same, via Despacho n.º 599/2019 for the €20 minimum) |
| IAS in force | **€537,13** | PARTIAL — Portaria n.º 480-A/2025/1, corroborated by OCC and Garrigues; the Diário da República page returned no machine-readable text, so it was not read in the diploma |
| Quarterly declaration | Last day of **January, April, July and October** | PARTIAL (OCC; in January the October–December income is declared) |
| Monthly payment window | **10th to 20th** of the following month | PARTIAL (OCC + DECO) |
| Start of activity | **12 months, and it is not an exemption** | PARTIAL (OCC, verbatim below) |

**The startup period is a deferral, not an exemption.** The OCC guide is explicit:
the enrolment exists, but its effects only begin on the first day of the twelfth
month after the activity starts. There is **no phased reduction ladder** (the
"50% then 25%" figure that circulates is not supported by the sources consulted),
and the application records no reductions at all rather than inventing them. This
is the second correction the research forced.

The Segurança Social sources are secondary because `seg-social.pt` could not be
read during the research: the *Guia Prático* PDFs return 404 and the document
paths redirect to the Segurança Social Direta login. That is stated in the pack's
`todo` rather than hidden.

The engine shows the arithmetic as a chain the user can follow, and splits the
quarterly contribution into three instalments that **sum exactly** to the total:

```
19 400,00 € services → 13 580,00 € relevant income (70%) → 2 906,12 € (21,4%)
                     → 968,71 € + 968,71 € + 968,70 €
```

Two things are deliberately **not** implemented, because implementing them needs
inputs the pack does not have: capping the base at the legal minimum/maximum
(needs the IAS and the base rules) and the worker's ±25% option on relevant income
(CRC art. 164.º — noted in the pack, not coded).

---

## 6. Documents to keep

The pack carries a `documentsToKeep` list per obligation, so the reminder is
actionable rather than generic. The standing duty covers issued invoices and
*faturas-recibo* with their ATCUD/QR requirements, monthly SAF-T (PT) invoicing
files, payment proofs for IVA, withholding and Segurança Social, the quarterly
Segurança Social declarations, and contracts, proposals and statements of work.

**Retention periods differ by tax, and the longest wins for invoices:**

- **10 civil years** for IVA records — CIVA art. 52.º n.º 1, as amended by
  Decreto-Lei n.º 49/2025, in force since 1 July 2025. (It used to be four years;
  secondary sources that still say four are out of date.)
- **4 years** for the duty to prove the elements of an IRS declaration — CIRS
  art. 128.º n.º 3.

The vault records both, and defaults to the longer period for invoicing records.

---

## 7. The 2026 calendar as published

Retrieved from the Portal das Finanças *Resumo anual — obrigações declarativas /
de pagamento em 2026* on **2026-09-20**. IMI, IUC, IRC and stamp duty are out of
scope for this profile.

### Declarative — day of the obligation

| Obligation | JAN | FEV | MAR | ABR | MAI | JUN | JUL | AGO | SET | OUT | NOV | DEZ |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Modelo 3 IRS + anexos | | | | | | **30** | | | | | | |
| Comunicação de faturas (e-fatura) | *9* | 5 | 5 | *8* | *8* | 5 | 6 | **31** | 7 | 6 | 5 | 7 |
| Validação de faturas do ano anterior | | *a)* | 2 | | | | | | | | | |
| Agregado familiar | | *a)* | 2 | | | | | | | | | |
| IVA — declaração periódica, regime mensal | 20 | 20 | 20 | 20 | 20 | **22** | 20 | — | **21** | 20 | 20 | **21** |
| IVA — declaração recapitulativa, envio mensal | 20 | 20 | 20 | 20 | 20 | 22 | 20 | 31 | 21 | 20 | 20 | 21 |

### Payment — last day for payment

| Obligation | JAN | FEV | MAR | ABR | MAI | JUN | JUL | AGO | SET | OUT | NOV | DEZ |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| IRS — pagamentos por conta | | | | | | | **20** | | **21** | | | **21** |
| Retenções na fonte — pagamento | 20 | 20 | 20 | 20 | 20 | 22 | 20 | 31 | 21 | 20 | 20 | 21 |
| IVA — regime mensal | **26** | 25 | 25 | **27** | 25 | 25 | **27** | — | 25 | **26** | 25 | **28** |
| IVA — regime trimestral | | 25 | | | 25 | | | | 25 | | 25 | |

**Notes, quoted because they explain the odd-looking dates:**

- **a)** *"Nos meses que terminam em fim de semana ou feriado, a obrigação pode
  ser cumprida até ao dia útil seguinte."* — obligations falling at a weekend or
  on a holiday may be met on the next business day.
- **b)** Payment flexibilisation, art. 16.º-C of Decreto-Lei n.º 125/2021 (added
  by Decreto-Lei n.º 85/2022).
- On the **declarative** page the despacho letters run **b)** Despacho n.º
  166/2025 (22/12), **c)** Despacho n.º 40/2026 (26/03), **d)** Despacho n.º
  55/2026 (28/04), **e)** Despacho n.º 68/2026 (12/05), **f)** Despacho n.º
  81/2026 (17/06).
- On the **payment** page the letters are different: **c)** Despacho n.º 68/2026
  and **d)** Despacho n.º 81/2026. Conflating the two pages' lettering is an easy
  mistake and the pack records both mappings.
- Despacho n.º 68/2026 could not be attached to a specific obligation and is
  therefore recorded in the pack's `todo` rather than applied to a date.

### Four lessons this table taught the design

1. The **declaration** deadline (day 20 of the second following month) and the
   **payment** deadline (day 25/26/27/28) are different obligations on different
   dates in the same month. They are modelled as two rules, not one.
2. Quarterly IVA is paid in **February, May, September and November** — not in the
   month following each quarter. And the **second quarter's declaration is due
  20 September** (CIVA art. 41.º n.º 10), which is precisely why there is no
   monthly-declaration deadline published for August.
3. There is **no published IVA monthly declaration deadline in August 2026**. A
   rule-derived engine would happily invent 20 August. This engine uses the legal
   rule, marks the date `derived`, and lists the year's published dates in the
   discrepancy so a human can see the candidate.
4. Several published days are the **weekend shift** of the legal day: 22 June
   (20 June is a Saturday), 21 September and 21 December. Others are **despacho
   extensions**: 8 April and 8 May, and 31 August for the July communication. The
   engine distinguishes the two and explains which one it found.

---

## 8. Verification status, at a glance

| Area | Verified | Partial | To confirm / not coded |
| --- | --- | --- | --- |
| art. 53.º ceiling and conditions | ✔ | | transition thresholds 15 000/18 750 not coded |
| IVA rates (mainland) | ✔ | | Azores/Madeira rates |
| 2026 published dates (declarative + payment) | ✔ | | |
| Modelo 3 deadline | ✔ (30 June) | | |
| e-fatura communication dates | ✔ | | |
| Withholding rates | ✔ (23 / 11,5 / 25%) | | 16,5% IP and 20% EBF not coded |
| Simplified-regime coefficients | ✔ (0,75 / 0,35) | | |
| art. 31.º n.º 13 add-back mechanism | ✔ (no cap) | | |
| Organised-accounting ceiling | | ✔ (€200 000) | |
| IVA quarterly ceiling | | ✔ (€650 000) | |
| Segurança Social rate and shares | | ✔ (21,4% / 70% / 20%) | |
| IAS 2026 | | ✔ (€537,13) | |
| Segurança Social deadlines | | ✔ | |
| SS startup period (12 months, deferral) | | ✔ | no phased reduction ladder exists per sources |
| SS base caps and ±25% option | | | not coded |
| IVA payment deadline article | | ✔ | |
| Retention payment deadline article | | ✔ | |
| Document retention (10 y IVA / 4 y IRS) | ✔ | | |
| Monthly SAF-T deadline | | ✔ (transposed from the invoice-communication row) | |

The pack itself is the authority on all of this: `vnfin rules` lists every rule
with its legal basis and source, `vnfin doctor` reports the verification counts,
and the agenda prints `?` beside anything that is not fully verified.

## Sources

- Portal das Finanças — *Resumo anual, obrigações declarativas em 2026*:
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/calendario_fiscal/Pages/Quadro_res_Decl_2026.aspx>
- Portal das Finanças — *Resumo anual, obrigações de pagamento em 2026*:
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/calendario_fiscal/Pages/Quadro_res_Pag_2026.aspx>
- Portal das Finanças — *IVA, Regime Especial de Isenção art. 53.º*:
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/questoes_frequentes/pages/faqs-01001.aspx>
- Portal das Finanças — *Agenda Fiscal*:
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/calendario_fiscal/Pages/obrigacoes.aspx>

Retrieved 2026-09-20. Per-rule sources, with retrieval dates and a verification
flag for each, are in `src/rules/pt/2026.json`; the research report, including
every field left null and why, is `docs/research/2026-sources.md`.
