# Rule pack PT 2026 — research notes and sources

Compiled: **2026-09-20**. Every URL below was retrieved on that date with an HTTP fetch (or,
where marked, appeared in web search results and was used only as a lead). Where a source is a
PDF, the text was extracted locally with `pdftotext`; the extraction is quoted, not paraphrased
from memory.

Tiering: `official` = AT / Segurança Social / Diário da República / gov.pt / parliament archive.
`secondary` = OCC, DECO, law firms, press — used to corroborate, never as the sole basis for a
figure that the law fixes.

Units used in the pack: money in **integer euro cents**; rates, percentages and coefficients in
**integer basis points** (1 % = 100). IAS multiples are expressed the same way (`150` = 1.5× IAS,
`1200` = 12× IAS).

---

## 1. AT 2026 calendars (declarative and payment)

- "Declaração mod. 3 e respetivos anexos": day **30** in JUN.
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/calendario_fiscal/Pages/Quadro_res_Decl_2026.aspx>
- "Comunicação dos elementos das faturas ou a sua inexistência", per month (JAN→DEZ):
  **9 (nota b), 5, 5, 8 (nota c), 8 (nota d), 5, 6, 31, 7, 6, 5, 7**. Same URL.
- IVA declaração periódica, **regime mensal**: 20, 20, 20, 20, 20, **22**, 20, *(none)*, **21**, 20, 20, **21**. Same URL.
- IVA declaração periódica, **regime trimestral**: **(FEV) 20, (MAI) 20, (SET) 21, (NOV) 20**. Same URL.
- IVA **declaração recapitulativa, envio mensal** (full row, not truncated): 20, 20, 20, 20, 20, **22**, 20,
  **31**, **21**, 20, 20, **21**. Same URL.
- IVA **declaração recapitulativa, envio trimestral**: (JAN) 20, (ABR) 20, (JUL) 20, (OUT) 20. Same URL.
- "Informação empresarial e simplificada" (IES): day **15** in JUL. Same URL.
- "Declaração mod. 22 para contribuintes com período de tributação coincidente com o ano civil":
  day **30** in JUN, with **nota f)**. Same URL.
- IRS "Pagamentos por conta": **JUL 20, SET 21, DEZ 21**.
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/calendario_fiscal/Pages/Quadro_res_Pag_2026.aspx>
- IRS-IRC "Pagamentos das importâncias retidas na fonte": 20, 20, 20, 20, 20, **22**, 20, **31**, **21**, 20, 20, **21**. Same URL.
- IVA pagamento, **regime mensal**: 26, 25, 25, **27**, 25, 25, **27**, *(none)*, 25, **26**, 25, **28**. Same URL.
- IVA pagamento, **regime trimestral**: (FEV) 25, (MAI) 25, (SET) 25, (NOV) 25. Same URL.
- Notes as published:
  - **a)** in months ending on a weekend or holiday, the obligation may be met on the next business day (both calendars).
  - Declarative calendar: **b)** Despacho n.º 166/2025 – XXV, de 22/12 · **c)** Despacho n.º 40/2026 – XXV, de 26/03 ·
    **d)** Despacho n.º 55/2026 – XXV, de 28/04 · **e)** Despacho n.º 68/2026 – XXV, de 12/05 · **f)** Despacho n.º 81/2026 – XXV, de 17/06.
  - Payment calendar: **b)** payment flexibility under art. 16.º-C of DL 125/2021 (added by DL 85/2022) ·
    **c)** Despacho n.º 68/2026 · **d)** Despacho n.º 81/2026.
  - Only the Despachos 68/2026 and 81/2026 are hyperlinked in the page HTML
    (`.../Despachos_SEAF/Documents/Despacho-SEAF-68-2026-XXV.pdf`, `...-81-2026-XXV.pdf`).

### Correction to the briefing (important)

The brief stated that notes **(c)** and **(d)** of the declarative calendar are Despacho 68/2026 and
Despacho 81/2026. That is **not** what the page says. Read in order, the declarative page notes are
b = 166/2025, **c = 40/2026 (26/03)**, **d = 55/2026 (28/04)**, e = 68/2026 (12/05), f = 81/2026 (17/06).
Consequently the extensions on the invoice-communication dates are:

| date (2026) | extension | reference |
| --- | --- | --- |
| 9 JAN | day 5 → 9 | Despacho n.º 166/2025 – XXV, de 22/12 |
| 8 ABR | day 5 → 8 | **Despacho n.º 40/2026 – XXV, de 26/03** |
| 8 MAI | day 5 → 8 | **Despacho n.º 55/2026 – XXV, de 28/04** |

Despacho 81/2026 (17/06) is note **f)**, attached to the **Modelo 22** row (30 JUN). Despacho 68/2026
(12/05) is note e) on the declarative page and note c) on the payment page, but the page does not
show which row it belongs to; it is therefore **not attached to any obligation** in the pack.

---

## 2. IVA — special exemption (art. 53.º CIVA)

- Conditions, cumulatively: registered office/domicile in national territory; **does not carry out
  export operations or connected activities**; did not estimate turnover above **15 000 €** by the
  end of the year in which activity starts. A taxpayer with organised accounting, or carrying out
  imports, or the Annex E operations, can still benefit.
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/Folhetos_informativos/Documents/Inicio_atividade.pdf>
  (footnote: art. 53.º CIVA, wording of DL 35/2025, de 24/03, and Ofício Circulado n.º 25062, de 2025-03-26)
- No annualisation in the start year, national domicile required, export operations excluded — also
  stated in the AT FAQ on art. 53.º:
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/questoes_frequentes/pages/faqs-01001.aspx>
- **Transition thresholds** (same AT folheto, "Regime Especial de Isenção do art.º 53.º" section):
  - turnover above 15 000 € but below **18 750 €** in a year → *declaração de alterações* within 15
    business days from the last day of that year; normal regime from 1 January of the next year;
  - **18 750 €** exceeded during the current year → IVA must be charged from that moment.
  These thresholds are **not** encoded in the JSON (the structure has no key for them) — see Unresolved.

## 3. IVA — rates

- **23 % / 13 % / 6 %**, mainland (art. 18.º n.º 1 CIVA, wording of Lei 55-A/2010):
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/civa_rep/Pages/iva18.aspx>
- Art. 18.º n.º 3 allows the Azores and Madeira legislative assemblies to set lower rates; the pack's
  rates are mainland only.

## 4. IVA — periodic regime, declaration and payment deadlines

- Art. 41.º CIVA (wording of DL 49/2025, in force 1 July 2025):
  - a) **monthly**: until day 20 of the 2nd month following the operations, for taxpayers with
    turnover **≥ 650 000 €** in the previous calendar year;
  - b) **quarterly**: until day 20 of the 2nd month following the quarter, for taxpayers with turnover
    **< 650 000 €**;
  - n.º 10: the June (monthly) and 2nd-quarter (quarterly) declarations are sent **until 20 September**.
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/civa_rep/Pages/iva41.aspx>
- Payment: day **25** of the 2nd month following (monthly) / following the quarter (quarterly); the
  June/2nd-quarter payment runs until 25 September. Same AT folheto as §2.
- Declaração recapitulativa: sent until day 20 of the month following the operations (same folheto).
- The AT folheto does not state the CIVA article for the *payment* deadline, so it is left unverified
  in the pack (see Unresolved).

## 5. IRS — simplified regime

- Ceiling for mandatory organised accounting: **200 000 €** of gross category B income in the
  immediately preceding tax period (art. 28.º n.º 2 CIRS). Cessation of the simplified regime only
  after exceeding it in two consecutive periods, or in one period by more than 25 % (n.º 6).
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs28.aspx>
- Coefficients (art. 31.º n.º 1 CIRS, current wording):
  - a) **0,15** — sale of goods and products, crypto-asset operations (except d)), restaurant/beverage
    and hotel-like activities (except local accommodation in a house/apartment);
  - b) **0,75** — income from professional activities specifically listed in the table of art. 151.º;
  - c) **0,35** — income from provisions of services not covered above;
  - d) 0,95; e) 0,30; f) 0,10; g) 1; h) 0,50.
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs31.aspx>
  (corroborated by the OCC guide, §10)
- **Art. 31.º n.º 13 — the "15 %" rule as it actually stands:** the deduction resulting from the
  coefficients of b) and c) is *partially conditional* on documented expenses. A positive difference
  is **added back to taxable income**, equal to 15 % of gross income from those service provisions
  minus the sum of: the specific deduction (art. 25.º n.º 1 a)) or social-security contributions if
  higher; staff costs; rents of property used in the activity; 1,5 % of the rateable value of property
  used in the activity (4 % for hotel/local-accommodation property); other expenses on goods and
  services related to the activity that are on invoices communicated to the AT; imports and
  intra-community acquisitions. n.º 14: expenses in c), d) and e) only partly allocated to the activity
  count at 25 %. n.º 15 a): invoices must be allocated in the Portal das Finanças **by the end of
  February of the following year**. Same URL.
  **There is no €1 500 cap and no "15 % deduction" in the current text** — that framing comes from
  older secondary summaries; the current mechanism is the add-back described above. The cap key is
  therefore `null` in the pack.
- Start-of-activity coefficient relief (not part of the SS "startup regime"): art. 31.º n.º 10 reduces
  the b), c) and f) coefficients by **50 % in the year activity starts and 25 % in the following year**,
  provided the taxpayer has no category A or H income in those periods; not applicable if activity
  ceased less than five years earlier (n.º 11). Same URL.

## 6. IRS — withholding on category B (2026)

- Art. 101.º n.º 1 CIRS (current text, includes DL 97/2026, de 20/05):
  - a) **16,5 %** — category B income under art. 3.º n.º 1 c) (intellectual/industrial property and
    information about experience acquired in the industrial, commercial or scientific sector, when
    received by the original holder);
  - b) **23 %** — income from professional activities listed in the table of art. 151.º (wording of
    Lei 45-A/2024);
  - c) **11,5 %** — other category B income under art. 3.º n.º 1 b) and n.º 2 g) and i) (i.e. other
    service provisions, isolated acts);
  - d) 20 % (EBF art. 58.º-A); e) 25 % and f) 10 % (category F).
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs101.aspx>
- Art. 3.º n.º 1 CIRS (to identify which income is which): a) commercial, industrial, agricultural,
  forestry or livestock; b) any provision of services on own account (including scientific, artistic or
  technical); c) intellectual/industrial property.
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs3.aspx>
- **Non-residents: 25 %**, definitive withholding on employment income and *all* business and
  professional income, including isolated acts (art. 71.º n.º 4 a)); n.º 5 exempts, for monthly income
  from a single entity, the part up to the national minimum wage.
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs71.aspx>
- The AT folheto repeats the four rates (11,5 % / 16,5 % / 20 % / 23 %) and adds the trigger: withholding
  is required if the previous year's income exceeded **15 000 €**, or from the moment the current year's
  income exceeds that value.
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/Folhetos_informativos/Documents/Inicio_atividade.pdf>
- OCC guide (Aug 2026) corroborates 23 % / 11,5 % / 16,5 % for TI. <https://www.occ.pt/sites/default/files/public/2026-08/Guia_pratico_ago_2026a.pdf>

## 7. IRS — Modelo 3 and payments on account

- Modelo 3: filed by electronic transmission **from 1 April to 30 June**, whether or not the day is a
  business day (art. 60.º n.º 1 CIRS).
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs60.aspx>
- Payments on account: three payments, **until day 20 of July, September and December**; total is 65 %
  of the formula amount; not payable if below 50 € (art. 102.º n.ºs 1–3).
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs102.aspx>

## 8. Invoicing communication, SAF-T (PT), e-fatura validation

- **Communication of invoice elements: until day 5 of the following month** (or the non-existence of
  invoices). **Dispensed if the taxpayer uses the Portal das Finanças invoice-issuing system**
  (deemed to be art. 4.º-A of DL 28/2019, added by DL 49/2025, per the folheto footnote).
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/Folhetos_informativos/Documents/Inicio_atividade.pdf>
- Legal framework: DL 198/2012, art. 3.º n.º 1 lists the channels — a) real-time transmission
  integrated in invoicing software, b) normalised file based on **SAF-T (PT)**, c) direct entry in the
  Portal das Finanças, d) other electronic route; n.º 3 requires taxpayers who must produce the
  SAF-T (PT) file to choose a) or b).
  Text extracted from the parliament archive copy (wording up to DL 71/2013):
  <https://app.parlamento.pt/OE2015/DL198_2012_a3.pdf>
  **Caveat:** that copy's art. 3.º n.º 2 still says day **25** of the following month; the day-5 rule
  practised and published by the AT today comes from the AT folheto above. The pack records day 5 and
  flags the discrepancy.
- **e-fatura validation** (allocation of invoices of the previous year): AT calendar shows FEB with
  note a) and day 2 in MAR. 28 February 2026 is a Saturday, hence 2 March. The underlying legal rule
  for activity expenses is art. 31.º n.º 15 a) CIRS ("until the end of February of the following
  year"), with art. 78.º-B applying *mutatis mutandis*.
  <https://info.portaldasfinancas.gov.pt/pt/apoio_contribuinte/calendario_fiscal/Pages/Quadro_res_Decl_2026.aspx>

## 9. Document retention

- **10 civil years** for records and supporting documents (art. 52.º n.º 1 CIVA, wording of DL 49/2025,
  in force 1 July 2025 — this changed the previous 4-year rule).
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/civa_rep/Pages/iva52.aspx>
- **4 years** for the obligation to prove the elements of IRS declarations (art. 128.º n.º 3 CIRS).
  <https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/codigos_tributarios/cirs_rep/Pages/irs128.aspx>
- Both are recorded in the pack; the app should keep documents for the longer (10-year) period to
  cover IVA.

## 10. Segurança Social — contribution parameters

From the **OCC Guia Prático, August 2026** (PDF extracted locally):
<https://www.occ.pt/sites/default/files/public/2026-08/Guia_pratico_ago_2026a.pdf>

- Applicable framework: CRC (Lei 110/2009, de 16/09), arts. 132.º–173.º; Decreto Regulamentar
  1-A/2011 (republished by DR 6/2018); Despacho 599/2019 (minimum contribution/base values).
- **General contribution rate 21,4 %** (arts. 134.º n.º 1 b) and 168.º CRC; arts. 54.º-A and 54.º-B
  DR 1-A/2011). A **25,2 %** rate applies only to ENI, EIRL holders and their spouses who carry out
  *exclusively* commercial or industrial activity.
- **Relevant income: 70 % of service provisions, 20 % of production and sale of goods** (art. 162.º
  CRC), for the quarterly regime; under organised accounting the relevant income is the taxable profit
  of the previous calendar year, if higher.
- **Monthly contribution base**: one twelfth of the quarterly relevant income (one third of the
  quarterly relevant income per month), **between 1,5 × IAS and 12 × IAS**; minimum contribution of
  20,00 €. A ± 25 % option in 5 % steps exists in the quarterly regime (art. 164.º CRC), not in
  organised accounting.
- Excluded from relevant income: self-consumption electricity/small renewable units, rents, local
  accommodation in a house/apartment, investment subsidies, capital gains, intellectual/industrial
  property income — the last three may be included by option.
- **IAS 2026 = 537,13 €**, fixed by Portaria n.º 480-A/2025/1, de 30 de dezembro (the guide names the
  portaria; the value is corroborated by Garrigues):
  <https://www.garrigues.com/pt/pt-PT/news/ias-e-atualizado-eu-53713-em-2026> ·
  <https://diariodarepublica.pt/dr/detalhe/portaria/480-a-2025-993056222> (page returned HTTP 200 but
  **no machine-readable text**, so the value was not read directly from the diploma).
- Corroboration of 21,4 % and of the 70 %/20 % split: DECO Proteste, 23/03/2026
  <https://www.deco.proteste.pt/dinheiro/impostos/noticias/recibos-verdes-obrigacoes-trabalhadores-independentes-seguranca-social>

## 11. Segurança Social — deadlines

- **Declaração trimestral**: filed in the Segurança Social Direta **until the last day of January,
  April, July and October**. In January the October–December income is declared, in April January–March,
  in July April–June, in October July–September. Elements may be replaced **until the 15th day after
  the deadline**. OCC guide, same URL as §10; DECO states the same last-day rule.
- **Payment**: monthly, **between the 10th and 20th of the month following** the month it relates to.
  OCC guide (same URL) and DECO (same URL).
- **Declaração anual** (art. 151.º-A CRC): filed **during January** of the following year; the
  obligation applies to anyone who had to file at least one quarterly declaration (art. 57.º-B n.º 4
  DR 1-A/2011). OCC guide, same URL.
- Taxpayers who determine the contribution base from taxable profit (organised accounting) **have no
  quarterly declaration and no ±25 % option**; pensioners exempt under art. 157.º n.º 1 b) and c) are
  also dispensed. OCC guide, same URL.

## 12. Segurança Social — start of activity

- **It is not an exemption.** OCC guide, verbatim: *"Não é uma isenção. O enquadramento existe, mas os
  seus efeitos só se produzem no 1.º dia do 12.º mês posterior ao início de atividade."* (same URL as §10)
- If activity ceases within the first 12 months, counting is suspended and resumes on the 1st day of
  the month activity restarts — but only if that happens within the 12 months following the cessation.
  Same URL.
- **No phased reductions after the 12 months were found in any source consulted.** The briefing's
  "12 months exemption, then reductions" appears to conflate this SS deferral with the IRS coefficient
  relief of art. 31.º n.º 10 CIRS (50 % in the start year, 25 % in the following year — see §5).

---

## Corrections and additions to the briefing

1. Note letters on the declarative calendar were mis-assigned (see §1): April/May extensions are
   Despacho **40/2026** and **55/2026**, not 68/2026 and 81/2026.
2. The "Declaração recapitulativa (envio mensal)" row was not truncated after SET 21 — the full 2026
   row is 20, 20, 20, 20, 20, 22, 20, **31**, 21, 20, 20, 21. The **31 August** date has no explanatory
   note and no identified legal basis.
3. The "15 % documented expenses" rule has **no €1 500 cap** in the current art. 31.º n.º 13 CIRS; the
   mechanism is an add-back to taxable income. (Cap left `null`.)
4. There is **no SS exemption/reduction ladder** for new independent workers; there is a 12-month
   deferral of the effects of the enquadramento.
5. IVA declaration for June and the 2nd quarter is due **20 September** (art. 41.º n.º 10 CIVA), which
   is why August has no monthly declaration in the AT calendar.
6. Document retention for IVA is now **10 years** (DL 49/2025), not 4 — while the IRS proof obligation
   remains 4 years.
7. Additional obligations encoded beyond the required list: `iva.recapitulativa.trimestral`,
   `ss.declaracaoAnual`, `iva.declaracaoAlteracoes`.
8. `iva.periodicRegime.quarterlyCeiling` / `monthlyAbove` = **65 000 000 cents (650 000 €)** — art. 41.º
   n.º 1 CIVA, both thresholds at the same value, with the quarterly regime unavailable above it.

## Unresolved / needs human verification

Every `null` left in the pack, and why:

| Field | Value | Why it is null / unresolved |
| --- | --- | --- |
| `constants.irs.simplifiedRegime.documentedExpensesDeductionCap` | `null` | Art. 31.º n.º 13 CIRS sets **no monetary cap** in the current wording (verified text). Secondary summaries still quote a €1 500 cap from an older wording — a human must confirm before any cap is coded. |
| All Segurança Social `officialDates2026` arrays (`ss.declaracaoTrimestral` holds computed dates; `ss.contribuicoesMensais` and `ss.declaracaoAnual` are empty) | empty / computed | The Segurança Social publishes no annual calendar equivalent to the AT's. The Guia Prático PDF URLs on seg-social.pt returned **404** and the `/documents/10152/...` paths **redirect to the Segurança Social Direta login**, so the official guide could not be read. SS facts rest on the OCC Guia Prático (Aug 2026) plus DECO. |
| `constants.socialSecurity.startupReductions` | `[]` | No reduction ladder (e.g. 50 % then 25 %) was found in any source. Only the 12-month deferral is documented. The briefing's premise is unsupported. |
| `constants.socialSecurity.iasMonthly` | 53713 (set) | Value corroborated by OCC (which cites Portaria 480-A/2025/1) and Garrigues, but the Diário da República page returned no machine-readable text, so it was not read in the diploma itself. |
| `constants.socialSecurity.baseMinMultipleIas` / `baseMaxMultipleIas` | 150 / 1200 | OCC states 1,5 × IAS to 12 × IAS. The CRC article number and the €20,00 minimum-contribution rule (Despacho 599/2019, cited by OCC) were not read in the primary text. |
| exact CRC article for the declaração trimestral | not encoded | OCC cites art. 151.º-A for the annual declaration, implying art. 151.º for the quarterly one, but this was not confirmed. |
| `modelo22` general legal deadline | not encoded | Only the 2026 official date (30 JUN, prorogated by Despacho 81/2026) is recorded. The AT page consulted for the CIRC uses pre-2009 article numbering (its "art. 112.º" is *Modalidades de pagamento*), so the general deadline could not be verified. |
| `ies` `legalBasis` | `[]` | Only the official 2026 date (15 JUL) was verified; the CIRC/ordinance basis was not confirmed. |
| `iva.pagamento.*` `legalBasis` | only the DL 125/2021 flexibility note | The CIVA article fixing the day-25 payment deadline was not confirmed; the deadline rests on the AT folheto and the 2026 calendar. |
| `irs.retencoesPagamento` | `verification: "partial"` | Rates verified in art. 101.º; the article fixing the delivery deadline (day 20 of the following month) was not confirmed in a code text. |
| `iva.recapitulativa` 31 AGO 2026 | recorded, unexplained | The date is in the official calendar with no note; the legal basis (a despacho or a rule specific to the recapitulativa) was not identified. |
| Despacho n.º 68/2026 (12/05) | not attached | Note e) on the declarative page and note c) on the payment page, but no row/marker was found in the published tables. PDF not retrieved (only the linked URL). |
| `saft.mensal` | `verification: "partial"` | The calendar has no separate SAF-T row; its 2026 dates were transposed from the invoice-communication row (same day-5 deadline). Whether real-time webservice communicators are dispensed from the monthly SAF-T file — and whether the Portal das Finanças dispensa (which *is* in the AT folheto) covers it — needs confirmation. Also, DL 198/2012 art. 3.º n.º 2 in the copy consulted still reads day 25. |
| withholding on intellectual/industrial property (16,5 %) and EBF art. 58.º-A (20 %) | not encoded | The required structure has only three withholding keys. Documented here for a future pack version. |
| Azores/Madeira IVA rates | not encoded | Pack records mainland 23/13/6 only (CIVA art. 18.º n.º 3 allows regional rates). |
| art. 53.º transition thresholds 15 000 € / 18 750 € | not encoded | Real and officially published (AT folheto), but the structure has no key; should be added before the app implements the exemption. |
| ±25 % relevant-income option (art. 164.º CRC) | not encoded | Documented in §10; no key in the required structure. |
| non-resident 25 % withholding | encoded | Art. 71.º n.º 4 a) read in the official text, but application to concrete cases (treaty relief, permanent establishment) needs human review. |
| `efatura.validacaoDespesas` art. 78.º-B | referenced only | Read only by remission from art. 31.º n.º 15 a); the article itself was not fetched. |
