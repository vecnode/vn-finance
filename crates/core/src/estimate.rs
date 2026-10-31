//! Calculations: invoices, IVA, Segurança Social and the simplified-regime IRS base.
//!
//! The rule here is that every function **shows its work and refuses to guess**.
//! When a rate is missing from the rule pack, the function returns zero *and* a
//! stated limitation rather than a plausible-looking number. A wrong number in a
//! tax tool is worse than a missing one, because the person cannot tell the
//! difference.
//!
//! Missing constants come from [`crate::rules::constant_bp`], which returns `None`
//! for a value the pack leaves empty. No function here falls back to a rate it was
//! not given.

use serde::{Deserialize, Serialize};

use crate::countries::{TurnoverScope, scope_of};
use crate::dates;
use crate::money::{BasisPoints, Cents, apply_bp, format_eur, split_evenly};
use crate::rules::constant_bp;
use crate::types::{IsoDate, IvaRegime, RulePack, TaxProfile};

/// Where an invoice stands in its own life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceStatus {
    /// Issued and awaiting payment.
    Issued,
    /// Paid.
    Paid,
    /// Issued but not yet due.
    Pending,
}

/// How VAT was treated on an invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VatTreatment {
    /// Portuguese VAT charged.
    IvaPt,
    /// Reverse charge: the EU client self-assesses.
    AutoliquidacaoUe,
    /// Exempt under art. 53.º of the CIVA.
    IsentoArt53,
    /// An export outside the European Union.
    Exportacao,
}

impl VatTreatment {
    /// The phrase the interface uses for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::IvaPt => "IVA português",
            Self::AutoliquidacaoUe => "autoliquidação pelo cliente (UE)",
            Self::IsentoArt53 => "isento pelo art. 53.º",
            Self::Exportacao => "exportação",
        }
    }
}

/// One invoice, as the ledger stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    /// Stable identifier, generated when it is recorded.
    pub id: String,
    /// The sequential document number the invoicing rules require.
    pub number: String,
    /// Date of issue.
    pub date: IsoDate,
    /// Who the invoice was issued to.
    pub client_name: String,
    /// The client's tax number, when there is one.
    pub client_nif: Option<String>,
    /// The client's country, as a code or as the document printed it.
    pub client_country: String,
    /// What was invoiced.
    pub description: String,
    /// The taxable base, in cents.
    pub base_cents: Cents,
    /// The IVA rate applied, in basis points.
    pub iva_rate_bp: BasisPoints,
    /// How IVA was treated.
    pub vat_treatment: VatTreatment,
    /// The withholding rate, in basis points.
    pub retention_bp: BasisPoints,
    /// The document's ATCUD, when it has one.
    pub atcud: Option<String>,
    /// Where the invoice stands.
    pub status: InvoiceStatus,
    /// Whether a receipt for it is archived in the vault.
    pub payment_proof_in_vault: bool,
}

/// Whose money each amount on an invoice is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceTotals {
    /// The taxable base.
    pub base_cents: Cents,
    /// The IVA charged.
    pub iva_cents: Cents,
    /// The IRS withheld by the client.
    pub retention_cents: Cents,
    /// What the client actually owes: base + IVA − retention.
    pub net_receivable_cents: Cents,
}

/// Compute what is whose on one invoice.
#[must_use]
pub fn invoice_totals(invoice: &Invoice) -> InvoiceTotals {
    let iva_cents = apply_bp(invoice.base_cents, invoice.iva_rate_bp);
    let retention_cents = apply_bp(invoice.base_cents, invoice.retention_bp);
    InvoiceTotals {
        base_cents: invoice.base_cents,
        iva_cents,
        retention_cents,
        net_receivable_cents: invoice.base_cents + iva_cents - retention_cents,
    }
}

/// Whether an invoice falls inside a date window. ISO strings compare as dates.
fn in_range(invoice: &Invoice, start: &str, end: &str) -> bool {
    invoice.date.as_str() >= start && invoice.date.as_str() <= end
}

/// The default tax terms for a new invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceTerms {
    /// How IVA should be treated.
    pub vat_treatment: VatTreatment,
    /// Which rate to charge.
    pub iva_rate_bp: BasisPoints,
    /// Which withholding rate to apply.
    pub retention_bp: BasisPoints,
}

/// Sensible terms for a new invoice, taken from the rule pack and the profile.
///
/// This is a small piece of law-adjacent judgement that three surfaces need, so it
/// lives here, tested, rather than being re-derived in each of them. Getting it
/// wrong produces an invoice that contradicts the alert engine — which is exactly
/// what happened when the retention default was written as a literal 25 %, and
/// again when the pack's non-resident rate was mistaken for "a non-resident client
/// withholds from you".
#[must_use]
pub fn default_invoice_terms(pack: &RulePack, profile: Option<&TaxProfile>, country: &str) -> InvoiceTerms {
    let scope = scope_of(country);

    if scope != TurnoverScope::National {
        // No Portuguese VAT is charged, and a client outside Portugal does not
        // withhold Portuguese IRS. The pack's non-resident rate applies in the other
        // direction: to income paid *to* a non-resident by a Portuguese payer.
        return InvoiceTerms {
            vat_treatment: if scope == TurnoverScope::Eu {
                VatTreatment::AutoliquidacaoUe
            } else {
                VatTreatment::Exportacao
            },
            iva_rate_bp: 0,
            retention_bp: 0,
        };
    }

    // An art. 53.º exempt taxpayer charges no VAT at all, and says so on the
    // invoice. Retention is independent of the IVA exemption and still applies.
    let exempt = profile.is_some_and(|profile| profile.iva.regime == IvaRegime::IsentoArt53);
    InvoiceTerms {
        vat_treatment: if exempt {
            VatTreatment::IsentoArt53
        } else {
            VatTreatment::IvaPt
        },
        iva_rate_bp: if exempt {
            0
        } else {
            constant_bp(pack, &["iva", "rates", "normal"]).unwrap_or(0)
        },
        retention_bp: constant_bp(pack, &["irs", "withholding", "professionalServicesResident"]).unwrap_or(0),
    }
}

/// Totals for one date window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodTotals {
    /// Summed taxable base.
    pub base_cents: Cents,
    /// Summed IVA charged to clients.
    pub iva_liquidado_cents: Cents,
    /// Summed IRS withheld by clients.
    pub retencao_sofrida_cents: Cents,
    /// How many invoices fell in the window.
    pub invoice_count: usize,
}

/// Totals for any date window; the building block for every period report.
#[must_use]
pub fn period_totals(invoices: &[Invoice], start: &str, end: &str) -> PeriodTotals {
    let mut totals = PeriodTotals {
        base_cents: 0,
        iva_liquidado_cents: 0,
        retencao_sofrida_cents: 0,
        invoice_count: 0,
    };
    for invoice in invoices {
        if !in_range(invoice, start, end) {
            continue;
        }
        let one = invoice_totals(invoice);
        totals.base_cents += one.base_cents;
        totals.iva_liquidado_cents += one.iva_cents;
        totals.retencao_sofrida_cents += one.retention_cents;
        totals.invoice_count += 1;
    }
    totals
}

/// Segurança Social for one quarter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsContributionEstimate {
    /// Gross income from services in the period.
    pub service_income_cents: Cents,
    /// The share of it that is relevant, in basis points.
    pub relevant_income_share_bp: BasisPoints,
    /// The relevant income itself.
    pub relevant_income_cents: Cents,
    /// The contribution rate, in basis points.
    pub contribution_rate_bp: BasisPoints,
    /// The contribution owed for the quarter.
    pub contribution_cents: Cents,
    /// The contribution settled in three monthly instalments.
    pub instalments_cents: Vec<Cents>,
    /// What could not be checked, in Portuguese.
    pub limitations: Vec<String>,
}

/// Segurança Social for one quarter, for a worker whose income is services:
/// relevant income is a share of gross service income, the contribution is a rate
/// applied to it, and it is paid in three monthly instalments.
///
/// `instalments` is how many parts the contribution is split into; zero is treated
/// as one rather than as an empty plan.
#[must_use]
pub fn estimate_social_security(
    pack: &RulePack,
    service_income_cents: Cents,
    instalments: u32,
) -> SsContributionEstimate {
    let mut limitations: Vec<String> = Vec::new();
    let share_bp = constant_bp(pack, &["socialSecurity", "relevantIncomeShareServices"]);
    let rate_bp = constant_bp(pack, &["socialSecurity", "contributionRate"]);

    if share_bp.is_none() {
        limitations.push("A percentagem de rendimento relevante não está no pacote de regras.".to_owned());
    }
    if rate_bp.is_none() {
        limitations.push("A taxa contributiva não está no pacote de regras.".to_owned());
    }
    if constant_bp(pack, &["socialSecurity", "iasMonthly"]).is_none() {
        limitations.push(
            "O valor do IAS não está no pacote de regras: não é possível verificar se a base de \
             incidência atinge o mínimo ou o máximo legal."
                .to_owned(),
        );
    }

    let relevant_income_cents = match share_bp {
        Some(share) => apply_bp(service_income_cents, share),
        None => 0,
    };
    let contribution_cents = match rate_bp {
        Some(rate) => apply_bp(relevant_income_cents, rate),
        None => 0,
    };
    let parts = instalments.max(1);
    SsContributionEstimate {
        service_income_cents,
        relevant_income_share_bp: share_bp.unwrap_or(0),
        relevant_income_cents,
        contribution_rate_bp: rate_bp.unwrap_or(0),
        contribution_cents,
        instalments_cents: split_evenly(contribution_cents, parts).unwrap_or_default(),
        limitations,
    }
}

/// One quarter's report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarterReport {
    /// The civil year.
    pub year: i64,
    /// The quarter, 1–4.
    pub quarter: i64,
    /// First day of the quarter.
    pub start: IsoDate,
    /// Last day of the quarter.
    pub end: IsoDate,
    /// What was invoiced in it.
    pub totals: PeriodTotals,
    /// What Segurança Social is owed for it.
    pub social_security: SsContributionEstimate,
}

/// Build one quarter's report.
///
/// # Errors
///
/// [`dates::DateError`] when the quarter is outside 1–4.
pub fn quarter_report(
    pack: &RulePack,
    invoices: &[Invoice],
    year: i64,
    quarter: i64,
) -> Result<QuarterReport, dates::DateError> {
    let range = dates::quarter_range(year, quarter)?;
    let totals = period_totals(invoices, &range.start, &range.end);
    Ok(QuarterReport {
        year,
        quarter,
        start: range.start,
        end: range.end,
        totals,
        social_security: estimate_social_security(pack, totals.base_cents, 3),
    })
}

/// The taxable base under the simplified regime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrsSimplifiedEstimate {
    /// Gross income from services in the year.
    pub gross_service_income_cents: Cents,
    /// The coefficient applied, in basis points.
    pub coefficient_bp: BasisPoints,
    /// The base after the coefficient.
    pub taxable_from_coefficient_cents: Cents,
    /// Eligible expenses the person can document.
    pub eligible_expenses_cents: Cents,
    /// The reference the law compares expenses against.
    pub documented_expenses_reference_cents: Option<Cents>,
    /// The positive difference, **added** to the taxable income.
    pub addition_to_taxable_income_cents: Option<Cents>,
    /// The taxable income.
    pub taxable_income_cents: Option<Cents>,
    /// IRS already withheld by clients.
    pub withholding_cents: Cents,
    /// What the calculation assumed.
    pub assumptions: Vec<String>,
    /// What could not be computed, in Portuguese.
    pub limitations: Vec<String>,
}

/// The taxable base under the simplified regime.
///
/// The mechanism is easy to get backwards, which is why this function states it out
/// loud: art. 31.º n.º 13 CIRS does **not** deduct 15 % of your expenses. It
/// **presumes** expenses equal to 15 % of your gross service income, and then adds
/// back the positive difference between that reference and the expenses you can
/// actually document. Document less than 15 % and you pay on more; document more
/// and nothing is added.
///
/// Applying the IRS brackets needs the art. 68.º table for the year, so this
/// deliberately stops at taxable income. Without an income figure the honest output
/// is a base, not a tax.
#[must_use]
pub fn estimate_irs_simplified_base(
    pack: &RulePack,
    profile: &TaxProfile,
    invoices: &[Invoice],
    year: i64,
    eligible_expenses_cents: Cents,
) -> IrsSimplifiedEstimate {
    let start = format!("{year}-01-01");
    let end = format!("{year}-12-31");
    let totals = period_totals(invoices, &start, &end);
    let coefficient_bp = profile.irs.coefficient_bp;
    let rate_bp = constant_bp(
        pack,
        &["irs", "simplifiedRegime", "documentedExpensesDeductionRate"],
    );

    let mut limitations: Vec<String> = vec![
        "Estimativa para o regime simplificado. Não aplica as taxas do art. 68.º do CIRS: o rendimento \
         tributável apresentado não é o imposto a pagar."
            .to_owned(),
        "Não aplica deduções à coleta nem tributações autónomas. As retenções sofridas são creditadas no \
         IRS final, não descontadas neste rendimento tributável."
            .to_owned(),
        "Pressupõe que todas as faturas do ano são rendimento da categoria B e que as despesas indicadas \
         são elegíveis nos termos do art. 31.º do CIRS."
            .to_owned(),
    ];

    let taxable_from_coefficient_cents = apply_bp(totals.base_cents, coefficient_bp);

    let Some(rate_bp) = rate_bp else {
        limitations.push(
            "A percentagem de referência das despesas documentadas (art. 31.º n.º 13 do CIRS) não está \
             no pacote de regras: o rendimento tributável não foi calculado."
                .to_owned(),
        );
        return IrsSimplifiedEstimate {
            gross_service_income_cents: totals.base_cents,
            coefficient_bp,
            taxable_from_coefficient_cents,
            eligible_expenses_cents,
            documented_expenses_reference_cents: None,
            addition_to_taxable_income_cents: None,
            taxable_income_cents: None,
            withholding_cents: totals.retencao_sofrida_cents,
            assumptions: vec![format!(
                "Coeficiente aplicado: {coefficient_bp} pontos base, definido no perfil."
            )],
            limitations,
        };
    };

    let reference_cents = apply_bp(totals.base_cents, rate_bp);
    let addition_to_taxable_income_cents = (reference_cents - eligible_expenses_cents).max(0);
    let taxable_income_cents = taxable_from_coefficient_cents + addition_to_taxable_income_cents;

    IrsSimplifiedEstimate {
        gross_service_income_cents: totals.base_cents,
        coefficient_bp,
        taxable_from_coefficient_cents,
        eligible_expenses_cents,
        documented_expenses_reference_cents: Some(reference_cents),
        addition_to_taxable_income_cents: Some(addition_to_taxable_income_cents),
        taxable_income_cents: Some(taxable_income_cents),
        withholding_cents: totals.retencao_sofrida_cents,
        assumptions: vec![
            format!(
                "Coeficiente aplicado ao rendimento de serviços: {coefficient_bp} pontos base, \
                 definido no perfil."
            ),
            format!(
                "Referência de despesas: {}% dos rendimentos brutos de serviços = {}.",
                percent_text(rate_bp),
                format_eur(reference_cents)
            ),
            format!(
                "Despesas elegíveis documentadas: {}.",
                format_eur(eligible_expenses_cents)
            ),
        ],
        limitations,
    }
}

/// How much cash to keep aside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveEstimate {
    /// The low end of the range.
    pub low_cents: Cents,
    /// The high end.
    pub high_cents: Cents,
    /// What the range is a percentage of.
    pub basis: Vec<String>,
    /// What it does not replace, in Portuguese.
    pub limitations: Vec<String>,
}

/// How much cash to keep aside. Returns a **range** on purpose: the point of this
/// number is to be a reserve, not a prediction, and a single figure would imply a
/// precision the available data does not support.
#[must_use]
pub fn estimate_reserve(
    pack: &RulePack,
    invoices: &[Invoice],
    as_of: &str,
    low_bp: i64,
    high_bp: i64,
) -> ReserveEstimate {
    // The year of the as-of date, without parsing it: the window is a string range.
    let start = format!("{}-01-01", as_of.get(..4).unwrap_or(as_of));
    let totals = period_totals(invoices, &start, as_of);
    let mut limitations: Vec<String> = vec![
        format!(
            "Reserva calculada sobre {} de rendimento faturado até {as_of}.",
            format_eur(totals.base_cents)
        ),
        "Intervalo indicativo: não substitui o cálculo final do IRS nem o apuramento da Segurança \
         Social."
            .to_owned(),
    ];
    if constant_bp(
        pack,
        &["irs", "simplifiedRegime", "documentedExpensesDeductionRate"],
    )
    .is_none()
    {
        limitations.push(
            "A percentagem de referência das despesas documentadas (art. 31.º n.º 13 do CIRS) é \
             desconhecida, o que alarga o intervalo."
                .to_owned(),
        );
    }
    ReserveEstimate {
        low_cents: apply_bp(totals.base_cents, low_bp),
        high_cents: apply_bp(totals.base_cents, high_bp),
        basis: vec![format!(
            "{}% a {}% do rendimento faturado no ano",
            percent_text(low_bp),
            percent_text(high_bp)
        )],
        limitations,
    }
}

/// A basis-point rate as a percentage, written the way JavaScript writes the number.
///
/// `1500` → `15`, `2140` → `21.4`, `150` → `1.5`. The reference interpolates
/// `bp / 100` into a string, so the decimal is a point, not a comma.
fn percent_text(bp: i64) -> String {
    if bp % 100 == 0 {
        format!("{}", bp / 100)
    } else {
        format!("{}", bp as f64 / 100.0)
    }
}

/// The rate to display on an invoice warning, `2300` → `23`.
#[must_use]
pub fn rate_percent_rounded(bp: BasisPoints) -> String {
    format!("{}", (bp as f64 / 100.0).round())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::load_embedded;

    fn invoice(
        number: &str,
        date: &str,
        country: &str,
        base: Cents,
        rate: BasisPoints,
        retention: BasisPoints,
    ) -> Invoice {
        Invoice {
            id: format!("inv_{number}"),
            number: number.to_owned(),
            date: date.to_owned(),
            client_name: "Cliente Exemplo".to_owned(),
            client_nif: Some("501234560".to_owned()),
            client_country: country.to_owned(),
            description: "Consultoria".to_owned(),
            base_cents: base,
            iva_rate_bp: rate,
            vat_treatment: if rate > 0 {
                VatTreatment::IvaPt
            } else {
                VatTreatment::IsentoArt53
            },
            retention_bp: retention,
            atcud: None,
            status: InvoiceStatus::Issued,
            payment_proof_in_vault: false,
        }
    }

    #[test]
    fn invoice_totals_split_the_money_the_way_the_client_sees_it() {
        // 1 000,00 EUR at 23 % IVA and 25 % retention.
        let one = invoice("FR 1", "2026-04-12", "PT", 100_000, 2300, 2500);
        let totals = invoice_totals(&one);
        assert_eq!(totals.iva_cents, 23_000);
        assert_eq!(totals.retention_cents, 25_000);
        assert_eq!(totals.net_receivable_cents, 100_000 + 23_000 - 25_000);
    }

    #[test]
    fn a_quarter_reports_its_invoices_and_the_contribution() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let invoices = vec![
            invoice("FR 1", "2026-01-15", "PT", 100_000, 2300, 2500),
            invoice("FR 2", "2026-03-31", "PT", 50_000, 2300, 2500),
            invoice("FR 3", "2026-04-01", "PT", 999_999, 2300, 2500),
        ];
        let report = quarter_report(&pack.pack, &invoices, 2026, 1)?;
        assert_eq!(
            report.totals.invoice_count, 2,
            "the April invoice is in the next quarter"
        );
        assert_eq!(report.totals.base_cents, 150_000);
        assert_eq!(report.totals.iva_liquidado_cents, 34_500);
        assert_eq!(report.totals.retencao_sofrida_cents, 37_500);
        // 70 % of 1 500,00 EUR, then 21,4 %, in three instalments that sum exactly.
        let ss = &report.social_security;
        assert_eq!(ss.relevant_income_share_bp, 7000);
        assert_eq!(ss.relevant_income_cents, 105_000);
        assert_eq!(ss.contribution_rate_bp, 2140);
        assert_eq!(ss.contribution_cents, 22_470);
        assert_eq!(ss.instalments_cents.iter().sum::<Cents>(), 22_470);
        assert_eq!(ss.instalments_cents.len(), 3);
        Ok(())
    }

    #[test]
    fn the_irs_base_adds_back_the_shortfall_and_never_deducts() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let profile = crate::profile::create_default_profile(&crate::profile::ProfileInput {
            nif: "123456789".to_owned(),
            name: "Pessoa Exemplo".to_owned(),
            ..Default::default()
        })?;
        let invoices = vec![invoice("FR 1", "2026-05-01", "PT", 1_000_000, 2300, 2500)];

        // 15 % of 10 000,00 EUR is 1 500,00 EUR. Documenting nothing adds it all back.
        let nothing = estimate_irs_simplified_base(&pack.pack, &profile, &invoices, 2026, 0);
        assert_eq!(nothing.documented_expenses_reference_cents, Some(150_000));
        assert_eq!(nothing.addition_to_taxable_income_cents, Some(150_000));
        assert_eq!(nothing.taxable_from_coefficient_cents, 750_000);
        assert_eq!(nothing.taxable_income_cents, Some(900_000));

        // Documenting more than the reference adds nothing.
        let plenty = estimate_irs_simplified_base(&pack.pack, &profile, &invoices, 2026, 200_000);
        assert_eq!(plenty.addition_to_taxable_income_cents, Some(0));
        assert_eq!(plenty.taxable_income_cents, Some(750_000));
        Ok(())
    }

    #[test]
    fn the_reserve_is_a_range_and_says_what_it_is_a_range_of() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let invoices = vec![invoice("FR 1", "2026-05-01", "PT", 1_000_000, 2300, 2500)];
        let reserve = estimate_reserve(&pack.pack, &invoices, "2026-09-20", 2500, 3500);
        assert_eq!(reserve.low_cents, 250_000);
        assert_eq!(reserve.high_cents, 350_000);
        assert_eq!(
            reserve.basis,
            vec!["25% a 35% do rendimento faturado no ano".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn default_terms_follow_the_client_and_the_regime() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let normal = crate::profile::create_default_profile(&crate::profile::ProfileInput {
            nif: "123456789".to_owned(),
            name: "Pessoa Exemplo".to_owned(),
            ..Default::default()
        })?;
        let terms = default_invoice_terms(&pack.pack, Some(&normal), "PT");
        assert_eq!(terms.vat_treatment, VatTreatment::IvaPt);
        assert_eq!(terms.iva_rate_bp, 2300);
        // Whatever the pack says, not whatever a reader remembers: the pack is the
        // only place a rate is written down.
        let expected_retention = crate::rules::constant_bp(
            &pack.pack,
            &["irs", "withholding", "professionalServicesResident"],
        )
        .unwrap_or(-1);
        assert_eq!(terms.retention_bp, expected_retention);
        assert!(
            expected_retention > 0,
            "a resident professional does withhold something"
        );

        let germany = default_invoice_terms(&pack.pack, Some(&normal), "DE");
        assert_eq!(germany.vat_treatment, VatTreatment::AutoliquidacaoUe);
        assert_eq!(germany.iva_rate_bp, 0);
        assert_eq!(
            germany.retention_bp, 0,
            "a German client does not withhold Portuguese IRS"
        );

        let united_states = default_invoice_terms(&pack.pack, Some(&normal), "US");
        assert_eq!(united_states.vat_treatment, VatTreatment::Exportacao);

        let mut exempt = normal.clone();
        exempt.iva.regime = IvaRegime::IsentoArt53;
        let exempt_terms = default_invoice_terms(&pack.pack, Some(&exempt), "pt");
        assert_eq!(exempt_terms.vat_treatment, VatTreatment::IsentoArt53);
        assert_eq!(exempt_terms.iva_rate_bp, 0);
        assert_eq!(
            exempt_terms.retention_bp, expected_retention,
            "retention survives the exemption"
        );
        Ok(())
    }

    #[test]
    fn a_missing_rate_is_a_stated_limitation_not_a_zero_with_no_explanation() {
        // A pack with no social-security constants: the contribution is zero and the
        // reasons are named, rather than the function inventing a rate.
        let mut pack = match load_embedded() {
            Ok(loaded) => loaded.pack,
            Err(_) => return,
        };
        pack.constants = serde_json::json!({});
        let estimate = estimate_social_security(&pack, 1_000_000, 3);
        assert_eq!(estimate.contribution_cents, 0);
        assert!(
            estimate.limitations.len() >= 3,
            "share, rate and IAS are all missing"
        );
        assert!(
            estimate
                .limitations
                .iter()
                .any(|line| line.contains("taxa contributiva"))
        );
    }

    #[test]
    fn percentages_are_written_the_way_javascript_writes_them() {
        assert_eq!(percent_text(1500), "15");
        assert_eq!(percent_text(2140), "21.4");
        assert_eq!(percent_text(150), "1.5");
        assert_eq!(rate_percent_rounded(2300), "23");
        assert_eq!(
            rate_percent_rounded(2250),
            "23",
            "half rounds away from zero, as toFixed does"
        );
    }
}
