//! Red flags and missing to-dos.
//!
//! **Every flag here is computed in code.** None of them involves a language model,
//! and that is a design decision rather than an implementation detail:
//!
//! - "invoice X has no withholding" is arithmetic on two fields, not a judgement
//!   about language. Asking a model to find it would make the answer
//!   non-deterministic, unauditable and occasionally absent.
//! - The consequence is that the assistant never receives the person's data.
//!
//! Each flag states what was observed, why it matters, and cites the same legal
//! sources the rule pack cites, so a flag and a rule can be checked together.
//!
//! The flag list is ordered by severity and then by title. The reference's comparator
//! is not a total order (it returns `1` for equal titles); because every code has a
//! distinct title, the two orders agree on real data, and this one is a total order
//! so the sort is defined.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::countries::{TurnoverScope, scope_of};
use crate::dates;
use crate::estimate::{Invoice, rate_percent_rounded};
use crate::money::{Cents, format_eur};
use crate::rules::{FreshnessStatus, constant, constant_cents, freshness};
use crate::types::{IsoDate, ObligationInstance, RulePack, TaxProfile};

/// How loudly a flag speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlagSeverity {
    /// Something is wrong now.
    Urgent,
    /// Something to look at.
    Attention,
    /// A statement of fact.
    Info,
}

/// The closed vocabulary of flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlagCode {
    /// No previous-year turnover: the art. 53.º ceiling cannot be evaluated.
    Art53NoTurnoverInput,
    /// The art. 53.º exemption is active and within its ceiling.
    Art53ExemptionActive,
    /// The taxpayer could be exempt but is on the normal regime.
    Art53ExemptionAvailable,
    /// The art. 53.º regime looks inapplicable for the previous year's figure.
    Art53CeilingExceededHistory,
    /// National turnover is past 80 % of the ceiling.
    Art53MarginShrinking,
    /// National turnover is past the ceiling but inside the transition band.
    Art53InTransitionZone,
    /// National turnover is past even the transition band.
    Art53CeilingCrossed,
    /// EU invoices: reverse charge, VIES, recapitulativa.
    EuInvoicesReverseCharge,
    /// Clients outside the Union: no Portuguese VAT, outside the ceiling.
    NonEuClientsNoPtVat,
    /// Foreign invoicing the profile does not declare.
    ForeignClientsNotDeclared,
    /// Portuguese VAT charged to a foreign client.
    PtIvaChargedOnForeignInvoice,
    /// A resident client withheld nothing.
    RetentionMissingOnResident,
    /// A non-resident client withheld Portuguese IRS.
    RetentionChargedOnNonResident,
    /// An invoice older than 60 days with no receipt in the vault.
    MissingPaymentProof,
    /// The same invoice number used twice.
    DuplicateInvoiceNumber,
    /// Documents missing for a deadline that is close.
    MissingDocumentsBeforeDeadline,
    /// Obligations past their date.
    ObligationsOverdue,
    /// The pack is not fully verified.
    RulesNotFullyVerified,
    /// The pack does not cover the current year.
    RulePackStale,
    /// A constant is still a proposal nobody confirmed.
    ValueProposedByAiUnconfirmed,
}

/// One flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    /// Which flag this is.
    pub code: FlagCode,
    /// How loudly it speaks.
    pub severity: FlagSeverity,
    /// One line.
    pub title: String,
    /// What was observed and why it matters.
    pub detail: String,
    /// The articles it rests on.
    pub legal_basis: Vec<String>,
    /// The citations it rests on.
    pub source_ids: Vec<String>,
    /// The obligations it belongs to.
    pub rule_ids: Vec<String>,
    /// The invoices it refers to.
    pub invoice_numbers: Vec<String>,
}

/// Turnover split by where the client is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnoverByScope {
    /// Invoiced in Portugal.
    pub national: Cents,
    /// Invoiced to other member states.
    pub eu: Cents,
    /// Invoiced outside the Union.
    pub non_eu: Cents,
}

/// Turnover split by where the client is, which is what the art. 53.º ceiling
/// actually measures: national territory, not total invoicing.
///
/// # Errors
///
/// [`dates::DateError`] when an invoice date is not an ISO date.
pub fn turnover_by_scope(invoices: &[Invoice], year: i64) -> Result<TurnoverByScope, dates::DateError> {
    let mut totals = TurnoverByScope::default();
    for invoice in invoices {
        if dates::parse_iso(&invoice.date)?.year != year {
            continue;
        }
        match scope_of(&invoice.client_country) {
            TurnoverScope::National => totals.national += invoice.base_cents,
            TurnoverScope::Eu => totals.eu += invoice.base_cents,
            TurnoverScope::NonEu => totals.non_eu += invoice.base_cents,
        }
    }
    Ok(totals)
}

/// Everything the flag engine reads.
pub struct FlagInput<'a> {
    /// The rule pack.
    pub pack: &'a RulePack,
    /// The taxpayer.
    pub profile: &'a TaxProfile,
    /// The ledger.
    pub invoices: &'a [Invoice],
    /// The agenda.
    pub instances: &'a [ObligationInstance],
    /// The date the person believes it is today.
    pub today: IsoDate,
    /// Obligation ids that already have a document in the vault.
    pub documented_obligation_ids: Vec<String>,
    /// Days ahead that count as "due soon".
    pub due_soon_days: Option<i64>,
}

/// The legal basis and citations of one rule.
fn cite(pack: &RulePack, rule_id: &str) -> (Vec<String>, Vec<String>) {
    match pack.obligations.iter().find(|rule| rule.id == rule_id) {
        Some(rule) => (rule.legal_basis.clone(), rule.source_ids.clone()),
        None => (Vec::new(), Vec::new()),
    }
}

/// The source ids the pack attaches to the art. 53.º block.
fn art53_source_ids(pack: &RulePack) -> Vec<String> {
    match constant(pack, &["iva", "art53", "sourceIds"]) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Compute every flag the person's affairs raise.
///
/// # Errors
///
/// [`dates::DateError`] when `today` or an invoice date is not an ISO date.
pub fn compute_flags(input: FlagInput<'_>) -> Result<Vec<Flag>, dates::DateError> {
    let FlagInput {
        pack,
        profile,
        invoices,
        instances,
        today,
        documented_obligation_ids,
        due_soon_days,
    } = input;
    let mut flags: Vec<Flag> = Vec::new();
    let year = dates::parse_iso(&today)?.year;
    let due_soon_days = due_soon_days.unwrap_or(30);
    let documented: Vec<&str> = documented_obligation_ids.iter().map(String::as_str).collect();

    let ceiling = constant_cents(pack, &["iva", "art53", "ceiling"]);
    let declared_previous = profile.activity.turnover_previous_year_cents;
    let scopes = turnover_by_scope(invoices, year)?;
    let art53_sources = art53_source_ids(pack);
    let art53_basis = vec!["CIVA art. 53.º".to_owned()];

    let mut push = |flag: Flag| flags.push(flag);

    // -----------------------------------------------------------------------
    // art. 53.º CIVA — the exemption is a number to watch all year
    // -----------------------------------------------------------------------

    if declared_previous.is_none() {
        push(Flag {
            code: FlagCode::Art53NoTurnoverInput,
            severity: FlagSeverity::Attention,
            title: "Falta o volume de negócios do ano anterior".to_owned(),
            detail: "O enquadramento no regime especial de isenção do art. 53.º do CIVA depende do \
                     volume de negócios em território nacional do ano civil anterior. Enquanto esse \
                     valor não for indicado, a aplicação não avalia a isenção nem avisa quando o \
                     limite se aproxima. Regista-o com `vnfin init --turnover-ano-anterior <valor>` \
                     ou editando o perfil no cofre."
                .to_owned(),
            legal_basis: art53_basis.clone(),
            source_ids: art53_sources.clone(),
            rule_ids: Vec::new(),
            invoice_numbers: Vec::new(),
        });
    } else if let Some(ceiling) = ceiling {
        let declared = declared_previous.unwrap_or(0);
        let qualifies_historically = declared <= ceiling && !profile.activity.exports && profile.resident_pt;
        let exempt = profile.iva.regime == crate::types::IvaRegime::IsentoArt53;
        let margin_cents = ceiling - declared;

        if exempt && !qualifies_historically {
            let mut reasons: Vec<String> = Vec::new();
            if declared > ceiling {
                reasons.push(format!(
                    "o volume de negócios do ano anterior ({}) excedeu o limite",
                    format_eur(declared)
                ));
            }
            if profile.activity.exports {
                reasons.push("o perfil indica operações de exportação".to_owned());
            }
            if !profile.resident_pt {
                reasons.push("o perfil não tem domicílio em território nacional".to_owned());
            }
            push(Flag {
                code: FlagCode::Art53CeilingExceededHistory,
                severity: FlagSeverity::Urgent,
                title: "O regime do art. 53.º parece não ser aplicável".to_owned(),
                detail: format!(
                    "O perfil está enquadrado na isenção do art. 53.º, mas {}. Confirma o \
                     enquadramento com um contabilista certificado antes de emitir mais faturas sem \
                     IVA.",
                    reasons.join(" e ")
                ),
                legal_basis: art53_basis.clone(),
                source_ids: art53_sources.clone(),
                rule_ids: Vec::new(),
                invoice_numbers: Vec::new(),
            });
        } else if exempt {
            push(Flag {
                code: FlagCode::Art53ExemptionActive,
                severity: FlagSeverity::Info,
                title: "Isenção do art. 53.º ativa".to_owned(),
                detail: format!(
                    "Volume de negócios do ano anterior: {}, margem de {} até ao limite de {}. \
                     Faturação de {} em território nacional este ano.",
                    format_eur(declared),
                    format_eur(margin_cents),
                    format_eur(ceiling),
                    format_eur(scopes.national)
                ),
                legal_basis: art53_basis.clone(),
                source_ids: art53_sources.clone(),
                rule_ids: Vec::new(),
                invoice_numbers: Vec::new(),
            });
        } else if qualifies_historically {
            push(Flag {
                code: FlagCode::Art53ExemptionAvailable,
                severity: FlagSeverity::Info,
                title: "Pode ser elegível para a isenção do art. 53.º".to_owned(),
                detail: format!(
                    "O volume de negócios do ano anterior ({}) ficou {} abaixo do limite de {} e não \
                     há operações de exportação no perfil. O enquadramento atual é o regime normal. A \
                     mudança para o regime de isenção é uma decisão declarativa: confirma-a com um \
                     contabilista certificado.",
                    format_eur(declared),
                    format_eur(margin_cents),
                    format_eur(ceiling)
                ),
                legal_basis: art53_basis.clone(),
                source_ids: art53_sources.clone(),
                rule_ids: Vec::new(),
                invoice_numbers: Vec::new(),
            });
        }

        // The current year, watched live against the national-territory figure.
        if exempt {
            let transition_ceiling = (ceiling as f64 * 1.25).round() as i64;
            let eighty_percent = (ceiling as f64 * 0.8).round() as i64;
            if scopes.national > transition_ceiling {
                push(Flag {
                    code: FlagCode::Art53CeilingCrossed,
                    severity: FlagSeverity::Urgent,
                    title: "Limite do art. 53.º ultrapassado durante o ano".to_owned(),
                    detail: format!(
                        "Faturação em território nacional este ano: {}, acima de {}. Acima deste \
                         valor o IVA passa a ser liquidado a partir do momento em que foi excedido. \
                         Confirma o procedimento com um contabilista certificado.",
                        format_eur(scopes.national),
                        format_eur(transition_ceiling)
                    ),
                    legal_basis: art53_basis.clone(),
                    source_ids: art53_sources.clone(),
                    rule_ids: Vec::new(),
                    invoice_numbers: Vec::new(),
                });
            } else if scopes.national > ceiling {
                push(Flag {
                    code: FlagCode::Art53InTransitionZone,
                    severity: FlagSeverity::Urgent,
                    title: "Zona de transição do art. 53.º".to_owned(),
                    detail: format!(
                        "Faturação em território nacional este ano: {}, entre o limite de {} e {}. \
                         Nesta zona é entregue declaração de alterações nos 15 dias úteis seguintes ao \
                         fim do ano e o regime normal aplica-se a partir de 1 de janeiro seguinte.",
                        format_eur(scopes.national),
                        format_eur(ceiling),
                        format_eur(transition_ceiling)
                    ),
                    legal_basis: art53_basis.clone(),
                    source_ids: art53_sources.clone(),
                    rule_ids: Vec::new(),
                    invoice_numbers: Vec::new(),
                });
            } else if scopes.national > eighty_percent {
                push(Flag {
                    code: FlagCode::Art53MarginShrinking,
                    severity: FlagSeverity::Attention,
                    title: "Margem do art. 53.º a diminuir".to_owned(),
                    detail: format!(
                        "Faturação em território nacional este ano: {}, acima de 80% do limite de {}. \
                         Faltam {} para o limite.",
                        format_eur(scopes.national),
                        format_eur(ceiling),
                        format_eur(ceiling - scopes.national)
                    ),
                    legal_basis: art53_basis.clone(),
                    source_ids: art53_sources.clone(),
                    rule_ids: Vec::new(),
                    invoice_numbers: Vec::new(),
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // Work outside Portugal
    // -----------------------------------------------------------------------

    let (recap_basis, recap_sources) = cite(pack, "iva.recapitulativa");
    if scopes.eu > 0 {
        push(Flag {
            code: FlagCode::EuInvoicesReverseCharge,
            severity: FlagSeverity::Info,
            title: "Serviços a clientes da União Europeia".to_owned(),
            detail: format!(
                "Faturação a clientes da UE este ano: {}. Estas operações não levam IVA português (o \
                 cliente autoliquida), exigem um número de registo válido no VIES e entram na \
                 declaração recapitulativa. Confirma que os recibos indicam \"IVA - autoliquidação\" e \
                 que o VIES está válido.",
                format_eur(scopes.eu)
            ),
            legal_basis: recap_basis.clone(),
            source_ids: recap_sources.clone(),
            rule_ids: vec!["iva.recapitulativa".to_owned()],
            invoice_numbers: numbers_where(invoices, |invoice| {
                scope_of(&invoice.client_country) == TurnoverScope::Eu
            }),
        });
    }

    if scopes.non_eu > 0 {
        push(Flag {
            code: FlagCode::NonEuClientsNoPtVat,
            severity: FlagSeverity::Info,
            title: "Serviços a clientes fora da União Europeia".to_owned(),
            detail: format!(
                "Faturação a clientes de países terceiros este ano: {}. Estas prestações não são \
                 sujeitas a IVA português e não contam para o limite do art. 53.º, que se mede em \
                 território nacional. Guarda o comprovativo de que o cliente está estabelecido fora da \
                 UE.",
                format_eur(scopes.non_eu)
            ),
            legal_basis: vec!["CIVA art. 6.º (localização das prestações de serviços)".to_owned()],
            source_ids: recap_sources.clone(),
            rule_ids: Vec::new(),
            invoice_numbers: numbers_where(invoices, |invoice| {
                scope_of(&invoice.client_country) == TurnoverScope::NonEu
            }),
        });
    }

    let has_foreign_invoices = scopes.eu > 0 || scopes.non_eu > 0;
    if has_foreign_invoices && !profile.activity.intra_community_operations && !profile.activity.exports {
        push(Flag {
            code: FlagCode::ForeignClientsNotDeclared,
            severity: FlagSeverity::Attention,
            title: "Faturação ao estrangeiro não declarada no perfil".to_owned(),
            detail: "Existem faturas a clientes fora de Portugal mas o perfil não indica operações \
                     intracomunitárias nem exportações. O enquadramento no perfil tem de refletir a \
                     realidade, porque é ele que determina quais obrigações entram na agenda."
                .to_owned(),
            legal_basis: Vec::new(),
            source_ids: Vec::new(),
            rule_ids: vec![
                "iva.recapitulativa".to_owned(),
                "iva.dp.mensal".to_owned(),
                "iva.dp.trimestral".to_owned(),
            ],
            invoice_numbers: Vec::new(),
        });
    }

    let wrong_vat: Vec<&Invoice> = invoices
        .iter()
        .filter(|invoice| {
            scope_of(&invoice.client_country) != TurnoverScope::National && invoice.iva_rate_bp > 0
        })
        .collect();
    if !wrong_vat.is_empty() {
        let listed = wrong_vat
            .iter()
            .map(|invoice| {
                format!(
                    "{}: {}% tratado como {}",
                    invoice.number,
                    rate_percent_rounded(invoice.iva_rate_bp),
                    invoice.vat_treatment.label()
                )
            })
            .collect::<Vec<String>>()
            .join(", ");
        push(Flag {
            code: FlagCode::PtIvaChargedOnForeignInvoice,
            severity: FlagSeverity::Urgent,
            title: "IVA português cobrado em faturas ao estrangeiro".to_owned(),
            detail: format!(
                "{} fatura(s) a clientes fora de Portugal têm IVA português liquidado ({listed}). Numa \
                 prestação de serviços a um cliente da UE o IVA é devido no país do cliente e é o \
                 cliente que o autoliquida; a fatura não deve levar IVA português. Se o tratamento \
                 estiver errado, a fatura tem de ser corrigida, não apenas o registo.",
                wrong_vat.len()
            ),
            legal_basis: vec!["CIVA art. 6.º (localização das prestações de serviços)".to_owned()],
            source_ids: recap_sources.clone(),
            rule_ids: Vec::new(),
            invoice_numbers: numbers_where(invoices, |invoice| {
                scope_of(&invoice.client_country) != TurnoverScope::National && invoice.iva_rate_bp > 0
            }),
        });
    }

    // -----------------------------------------------------------------------
    // Per-invoice checks that are pure arithmetic
    // -----------------------------------------------------------------------

    let (retention_basis, retention_sources) = cite(pack, "irs.retencoesPagamento");
    let missing_retention: Vec<&Invoice> = invoices
        .iter()
        .filter(|invoice| {
            scope_of(&invoice.client_country) == TurnoverScope::National && invoice.retention_bp == 0
        })
        .collect();
    if !missing_retention.is_empty() {
        push(Flag {
            code: FlagCode::RetentionMissingOnResident,
            severity: FlagSeverity::Attention,
            title: "Faturas a clientes residentes sem retenção na fonte".to_owned(),
            detail: format!(
                "{} fatura(s) a clientes residentes não têm retenção na fonte ({}). Se o cliente é \
                 obrigado a reter, a retenção em falta tem de ser regularizada — o valor é creditado no \
                 IRS final e a diferença aparece no acerto. A taxa aplicável está no pacote de regras, \
                 verificada no art. 101.º do CIRS.",
                missing_retention.len(),
                missing_retention
                    .iter()
                    .map(|invoice| invoice.number.clone())
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
            legal_basis: retention_basis.clone(),
            source_ids: retention_sources.clone(),
            rule_ids: vec!["irs.retencoesPagamento".to_owned()],
            invoice_numbers: missing_retention
                .iter()
                .map(|invoice| invoice.number.clone())
                .collect(),
        });
    }

    let wrong_retention: Vec<&Invoice> = invoices
        .iter()
        .filter(|invoice| {
            scope_of(&invoice.client_country) != TurnoverScope::National && invoice.retention_bp > 0
        })
        .collect();
    if !wrong_retention.is_empty() {
        push(Flag {
            code: FlagCode::RetentionChargedOnNonResident,
            severity: FlagSeverity::Attention,
            title: "Retenção na fonte em faturas ao estrangeiro".to_owned(),
            detail: format!(
                "{} fatura(s) a clientes não residentes têm retenção na fonte ({}). Clientes fora de \
                 Portugal não fazem retenção de IRS português.",
                wrong_retention.len(),
                wrong_retention
                    .iter()
                    .map(|invoice| invoice.number.clone())
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
            legal_basis: retention_basis.clone(),
            source_ids: retention_sources.clone(),
            rule_ids: Vec::new(),
            invoice_numbers: wrong_retention
                .iter()
                .map(|invoice| invoice.number.clone())
                .collect(),
        });
    }

    let mut unpaid: Vec<&Invoice> = Vec::new();
    for invoice in invoices {
        if !invoice.payment_proof_in_vault && dates::days_between(&invoice.date, &today)? > 60 {
            unpaid.push(invoice);
        }
    }
    if !unpaid.is_empty() {
        push(Flag {
            code: FlagCode::MissingPaymentProof,
            severity: FlagSeverity::Attention,
            title: "Faturas sem comprovativo de pagamento".to_owned(),
            detail: format!(
                "{} fatura(s) emitidas há mais de 60 dias não têm comprovativo de recebimento no cofre \
                 ({}). O comprovativo é o que liga o rendimento declarado ao dinheiro que entrou.",
                unpaid.len(),
                unpaid
                    .iter()
                    .map(|invoice| invoice.number.clone())
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
            legal_basis: Vec::new(),
            source_ids: Vec::new(),
            rule_ids: Vec::new(),
            invoice_numbers: unpaid.iter().map(|invoice| invoice.number.clone()).collect(),
        });
    }

    // Duplicates, in the order the numbers first appear.
    let mut order: Vec<&str> = Vec::new();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for invoice in invoices {
        let entry = counts.entry(invoice.number.as_str()).or_insert(0);
        if *entry == 0 {
            order.push(invoice.number.as_str());
        }
        *entry += 1;
    }
    let duplicates: Vec<String> = order
        .iter()
        .filter(|number| counts.get(**number).copied().unwrap_or(0) > 1)
        .map(|number| (*number).to_owned())
        .collect();
    if !duplicates.is_empty() {
        push(Flag {
            code: FlagCode::DuplicateInvoiceNumber,
            severity: FlagSeverity::Urgent,
            title: "Números de fatura repetidos".to_owned(),
            detail: format!(
                "Há números de fatura usados mais de uma vez ({}). A numeração sequencial é uma \
                 obrigação de faturação e um número repetido é o tipo de erro que faz uma fatura ser \
                 recusada.",
                duplicates.join(", ")
            ),
            legal_basis: vec!["CIVA art. 36.º (requisitos de emissão e numeração)".to_owned()],
            source_ids: Vec::new(),
            rule_ids: Vec::new(),
            invoice_numbers: duplicates,
        });
    }

    // -----------------------------------------------------------------------
    // Obligations already in the agenda
    // -----------------------------------------------------------------------

    let overdue: Vec<&ObligationInstance> = instances
        .iter()
        .filter(|instance| instance.status == crate::types::InstanceStatus::Overdue)
        .collect();
    if !overdue.is_empty() {
        push(Flag {
            code: FlagCode::ObligationsOverdue,
            severity: FlagSeverity::Urgent,
            title: format!("{} obrigação(ões) em atraso", overdue.len()),
            detail: overdue
                .iter()
                .take(5)
                .map(|instance| format!("{}: {}", instance.due_date, instance.title))
                .collect::<Vec<String>>()
                .join("\n"),
            legal_basis: Vec::new(),
            source_ids: Vec::new(),
            rule_ids: overdue.iter().map(|instance| instance.rule_id.clone()).collect(),
            invoice_numbers: Vec::new(),
        });
    }

    let mut upcoming_with_documents: Vec<&ObligationInstance> = Vec::new();
    for instance in instances {
        if matches!(
            instance.status,
            crate::types::InstanceStatus::Done
                | crate::types::InstanceStatus::Untracked
                | crate::types::InstanceStatus::NotApplicable
        ) {
            continue;
        }
        if instance.documents_to_keep.is_empty() {
            continue;
        }
        let delta = dates::days_between(&today, &instance.due_date)?;
        if (0..=due_soon_days).contains(&delta) && !documented.contains(&instance.rule_id.as_str()) {
            upcoming_with_documents.push(instance);
        }
    }
    if !upcoming_with_documents.is_empty() {
        push(Flag {
            code: FlagCode::MissingDocumentsBeforeDeadline,
            severity: FlagSeverity::Attention,
            title: "Documentos em falta para prazos próximos".to_owned(),
            detail: upcoming_with_documents
                .iter()
                .take(4)
                .map(|instance| {
                    format!(
                        "{} — {}: falta um de {} documento(s) obrigatório(s) no cofre",
                        instance.due_date,
                        instance.title,
                        instance.documents_to_keep.len()
                    )
                })
                .collect::<Vec<String>>()
                .join("\n"),
            legal_basis: Vec::new(),
            source_ids: Vec::new(),
            rule_ids: upcoming_with_documents
                .iter()
                .map(|instance| instance.rule_id.clone())
                .collect(),
            invoice_numbers: Vec::new(),
        });
    }

    flags.sort_by(|left, right| {
        left.severity
            .cmp(&right.severity)
            .then_with(|| left.title.cmp(&right.title))
    });
    Ok(flags)
}

/// The numbers of the invoices a predicate selects, in ledger order.
fn numbers_where(invoices: &[Invoice], keep: impl Fn(&Invoice) -> bool) -> Vec<String> {
    invoices
        .iter()
        .filter(|invoice| keep(invoice))
        .map(|invoice| invoice.number.clone())
        .collect()
}

/// How many flags of each severity there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagSummary {
    /// Urgent flags.
    pub urgent: usize,
    /// Flags to look at.
    pub attention: usize,
    /// Informational flags.
    pub info: usize,
    /// All of them.
    pub total: usize,
}

/// Count the flags.
#[must_use]
pub fn summarise_flags(flags: &[Flag]) -> FlagSummary {
    FlagSummary {
        urgent: flags
            .iter()
            .filter(|flag| flag.severity == FlagSeverity::Urgent)
            .count(),
        attention: flags
            .iter()
            .filter(|flag| flag.severity == FlagSeverity::Attention)
            .count(),
        info: flags
            .iter()
            .filter(|flag| flag.severity == FlagSeverity::Info)
            .count(),
        total: flags.len(),
    }
}

/// Flags that belong to the pack's own health rather than to the person's affairs.
///
/// Kept separate so the Diagnóstico page can report them without mixing them into
/// the person's to-do list.
///
/// # Errors
///
/// [`dates::DateError`] when `today` is not an ISO date.
pub fn pack_health_flags(pack: &RulePack, today: &str) -> Result<Vec<Flag>, dates::DateError> {
    let mut flags: Vec<Flag> = Vec::new();

    let state = freshness(pack, today)?;
    if state.status != FreshnessStatus::Current {
        flags.push(Flag {
            code: FlagCode::RulePackStale,
            severity: if state.status == FreshnessStatus::Stale {
                FlagSeverity::Urgent
            } else {
                FlagSeverity::Info
            },
            title: if state.status == FreshnessStatus::Stale {
                "Pacote de regras desatualizado".to_owned()
            } else {
                "Pacote de regras de outro ano".to_owned()
            },
            detail: state.message,
            legal_basis: Vec::new(),
            source_ids: Vec::new(),
            rule_ids: Vec::new(),
            invoice_numbers: Vec::new(),
        });
    }

    if let Some(provenance) = &pack.constant_provenance {
        for (path, entry) in provenance {
            if entry.status != crate::types::ProvenanceStatus::Unverified {
                continue;
            }
            let source = entry.source == crate::types::ProvenanceSource::AiProposed;
            let origin = if source {
                "proposto pelo assistente"
            } else {
                "registado"
            };
            let url = match &entry.source_url {
                Some(url) => format!(" Fonte indicada: {url}"),
                None => String::new(),
            };
            flags.push(Flag {
                code: FlagCode::ValueProposedByAiUnconfirmed,
                severity: if source {
                    FlagSeverity::Urgent
                } else {
                    FlagSeverity::Attention
                },
                title: "Valor por confirmar no pacote de regras".to_owned(),
                detail: format!(
                    "{path}: valor {origin} em {} e ainda não confirmado por uma pessoa.{url} Enquanto \
                     não for confirmado na fonte, os cálculos que dependem dele não devem ser usados.",
                    entry.proposed_at
                ),
                legal_basis: Vec::new(),
                source_ids: Vec::new(),
                rule_ids: Vec::new(),
                invoice_numbers: Vec::new(),
            });
        }
    }

    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::build_agenda;
    use crate::estimate::{InvoiceStatus, VatTreatment};
    use crate::profile::{ProfileInput, create_default_profile};
    use crate::rules::load_embedded;
    use crate::types::{AgendaOptions, IvaRegime};

    fn invoice(number: &str, date: &str, country: &str, base: Cents, rate: i64, retention: i64) -> Invoice {
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

    fn profile() -> Option<TaxProfile> {
        create_default_profile(&ProfileInput {
            nif: "123456789".to_owned(),
            name: "Pessoa Exemplo".to_owned(),
            start_date: Some("2019-04-01".to_owned()),
            iva_regime: Some(IvaRegime::Trimestral),
            ..Default::default()
        })
        .ok()
    }

    #[test]
    fn with_no_data_the_only_complaint_is_the_missing_turnover() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let profile = profile().ok_or("profile")?;
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &profile,
            invoices: &[],
            instances: &[],
            today: "2026-09-20".to_owned(),
            documented_obligation_ids: Vec::new(),
            due_soon_days: None,
        })?;
        assert_eq!(
            flags.len(),
            1,
            "was: {:?}",
            flags.iter().map(|f| f.code).collect::<Vec<_>>()
        );
        assert_eq!(flags[0].code, FlagCode::Art53NoTurnoverInput);
        assert_eq!(flags[0].severity, FlagSeverity::Attention);
        Ok(())
    }

    #[test]
    fn an_active_exemption_reports_its_margin() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let mut profile = profile().ok_or("profile")?;
        profile.iva.regime = IvaRegime::IsentoArt53;
        profile.activity.turnover_previous_year_cents = Some(500_000);
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &profile,
            invoices: &[],
            instances: &[],
            today: "2026-09-20".to_owned(),
            documented_obligation_ids: Vec::new(),
            due_soon_days: None,
        })?;
        let active = flags
            .iter()
            .find(|flag| flag.code == FlagCode::Art53ExemptionActive)
            .ok_or("the exemption must be reported as active")?;
        assert_eq!(active.severity, FlagSeverity::Info);
        assert!(active.detail.contains("margem de"), "{}", active.detail);
        Ok(())
    }

    #[test]
    fn national_turnover_is_watched_against_the_transition_band() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let mut profile = profile().ok_or("profile")?;
        profile.iva.regime = IvaRegime::IsentoArt53;
        profile.activity.turnover_previous_year_cents = Some(500_000);
        let ceiling = constant_cents(&pack.pack, &["iva", "art53", "ceiling"]).ok_or("ceiling")?;
        // Just past the ceiling: the transition zone, not the hard crossing.
        let invoices = vec![invoice("FR 1", "2026-02-01", "PT", ceiling + 1_000, 0, 0)];
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &profile,
            invoices: &invoices,
            instances: &[],
            today: "2026-09-20".to_owned(),
            documented_obligation_ids: Vec::new(),
            due_soon_days: None,
        })?;
        assert!(
            flags
                .iter()
                .any(|flag| flag.code == FlagCode::Art53InTransitionZone)
        );
        assert!(
            !flags
                .iter()
                .any(|flag| flag.code == FlagCode::Art53CeilingCrossed)
        );
        Ok(())
    }

    #[test]
    fn foreign_invoicing_raises_the_reverse_charge_and_the_declaration()
    -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let profile = profile().ok_or("profile")?;
        let invoices = vec![invoice("FR 1", "2026-02-01", "DE", 100_000, 0, 0)];
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &profile,
            invoices: &invoices,
            instances: &[],
            today: "2026-09-20".to_owned(),
            documented_obligation_ids: Vec::new(),
            due_soon_days: None,
        })?;
        let codes: Vec<FlagCode> = flags.iter().map(|flag| flag.code).collect();
        assert!(codes.contains(&FlagCode::EuInvoicesReverseCharge));
        assert!(codes.contains(&FlagCode::ForeignClientsNotDeclared));
        assert!(codes.contains(&FlagCode::Art53NoTurnoverInput));
        Ok(())
    }

    #[test]
    fn flags_are_ordered_by_severity_then_title() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let profile = profile().ok_or("profile")?;
        let invoices = vec![
            invoice("FR 1", "2026-01-01", "DE", 100_000, 0, 0),
            invoice("FR 2", "2026-01-02", "PT", 100_000, 2300, 0),
        ];
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &profile,
            invoices: &invoices,
            instances: &[],
            today: "2026-09-20".to_owned(),
            documented_obligation_ids: Vec::new(),
            due_soon_days: None,
        })?;
        for pair in flags.windows(2) {
            assert!(
                (pair[0].severity, &pair[0].title) <= (pair[1].severity, &pair[1].title),
                "{} must not come before {}",
                pair[0].title,
                pair[1].title
            );
        }
        Ok(())
    }

    #[test]
    fn an_overdue_obligation_is_urgent_and_carries_its_rule_ids() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let profile = profile().ok_or("profile")?;
        let instances = build_agenda(
            &pack.pack,
            &profile,
            &AgendaOptions {
                year: 2026,
                today: "2026-09-20".to_owned(),
                due_soon_days: None,
                completed_ids: Vec::new(),
                track_from: None,
                holidays: Vec::new(),
            },
        )?;
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &profile,
            invoices: &[],
            instances: &instances,
            today: "2026-09-20".to_owned(),
            documented_obligation_ids: Vec::new(),
            due_soon_days: None,
        })?;
        let overdue = flags
            .iter()
            .find(|flag| flag.code == FlagCode::ObligationsOverdue)
            .ok_or("the agenda has overdue items")?;
        assert_eq!(overdue.severity, FlagSeverity::Urgent);
        assert!(!overdue.rule_ids.is_empty());
        assert_eq!(
            overdue.rule_ids.len(),
            instances
                .iter()
                .filter(|i| i.status == crate::types::InstanceStatus::Overdue)
                .count()
        );
        Ok(())
    }

    #[test]
    fn a_duplicate_number_is_reported_once_in_order() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let profile = profile().ok_or("profile")?;
        let invoices = vec![
            invoice("FR 9", "2026-01-01", "PT", 100_000, 2300, 2500),
            invoice("FR 1", "2026-01-02", "PT", 100_000, 2300, 2500),
            invoice("FR 9", "2026-01-03", "PT", 100_000, 2300, 2500),
        ];
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &profile,
            invoices: &invoices,
            instances: &[],
            today: "2026-09-20".to_owned(),
            documented_obligation_ids: Vec::new(),
            due_soon_days: None,
        })?;
        let duplicate = flags
            .iter()
            .find(|flag| flag.code == FlagCode::DuplicateInvoiceNumber)
            .ok_or("the repeated number must be reported")?;
        assert_eq!(duplicate.invoice_numbers, vec!["FR 9".to_owned()]);
        Ok(())
    }

    #[test]
    fn a_stale_pack_is_a_pack_health_flag_not_a_person_flag() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let current = pack_health_flags(&pack.pack, "2026-09-20")?;
        assert!(current.is_empty(), "the shipped pack is current for 2026");
        let stale = pack_health_flags(&pack.pack, "2027-01-15")?;
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].code, FlagCode::RulePackStale);
        assert_eq!(stale[0].severity, FlagSeverity::Urgent);
        let future = pack_health_flags(&pack.pack, "2025-12-31")?;
        assert_eq!(future[0].severity, FlagSeverity::Info);
        Ok(())
    }

    #[test]
    fn the_summary_counts_each_severity() {
        let flags = vec![
            Flag {
                code: FlagCode::ObligationsOverdue,
                severity: FlagSeverity::Urgent,
                title: "a".to_owned(),
                detail: String::new(),
                legal_basis: Vec::new(),
                source_ids: Vec::new(),
                rule_ids: Vec::new(),
                invoice_numbers: Vec::new(),
            },
            Flag {
                code: FlagCode::RulePackStale,
                severity: FlagSeverity::Info,
                title: "b".to_owned(),
                detail: String::new(),
                legal_basis: Vec::new(),
                source_ids: Vec::new(),
                rule_ids: Vec::new(),
                invoice_numbers: Vec::new(),
            },
        ];
        let summary = summarise_flags(&flags);
        assert_eq!(
            (summary.urgent, summary.attention, summary.info, summary.total),
            (1, 0, 1, 2)
        );
    }
}
