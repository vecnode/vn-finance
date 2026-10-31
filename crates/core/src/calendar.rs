//! The obligation engine.
//!
//! Turns a rule pack plus a taxpayer profile into the concrete list of things that
//! are due in a given year. Three properties matter more than anything else:
//!
//! - **Published dates beat derived dates.** If the authority has published a date
//!   for the year, that date is used, even when it disagrees with the general legal
//!   rule (deadlines move for weekends, holidays and despachos de prorrogação). When
//!   the two cannot be reconciled, the instance carries a discrepancy naming both,
//!   so a human checks instead of trusting.
//! - **Derivation knows the shapes real deadlines have.** A day of the month, the
//!   last day of a month, a window whose end is the deadline, a payment window, or a
//!   duty triggered by an event: each is derived according to what it means, and the
//!   derivation says when it had to assume something.
//! - **Nothing is invented.** When next year's calendar is not published yet, the
//!   derived dates are marked `provisional` and the interface says so.
//!
//! The engine is pure: the same inputs give the same output, with no clock, no
//! network and no model. `today` is a parameter.

use crate::conditions::{FieldLabels, applies_to, explain_failure};
use crate::dates;
use crate::types::{
    AgendaOptions, ChecklistItem, DeadlineKind, DeadlineRule, DueDateSource, InstanceStatus, IsoDate,
    ObligationInstance, ObligationKind, ObligationRule, OfficialDate, RulePack, TaxProfile,
};

/// One period of an obligation, before its date is resolved.
#[derive(Debug, Clone)]
struct PeriodSeed {
    period_start: IsoDate,
    period_end: IsoDate,
    period_label: String,
    due_year: i64,
    due_month: i64,
    /// True when the seed came straight from a published date and needs no checking.
    pinned: bool,
    /// An explanation attached to a pinned date, e.g. a merged weekend shift.
    pre_note: Option<String>,
}

/// What the rule says about the day, and what it had to assume.
struct LegalDate {
    day: Option<i64>,
    notes: Vec<String>,
}

/// A due date and how it was arrived at.
struct ResolvedDate {
    due_date: IsoDate,
    source: DueDateSource,
    adjusted_for_weekend: bool,
    discrepancies: Vec<String>,
}

/// Build `YYYY-MM-DD` from arithmetic values, clamping what a pack could get wrong.
fn iso(year: i64, month: i64, day: i64) -> IsoDate {
    let month = u32::try_from(month).unwrap_or(1);
    let day = u32::try_from(day).unwrap_or(1);
    dates::make_iso(year, month, day)
}

/// How many days the month of a seed has.
fn month_length(due_year: i64, due_month: i64) -> i64 {
    let month = u32::try_from(due_month).unwrap_or(1);
    i64::from(dates::days_in_month(due_year, month))
}

/// Rough day distance between two month/day pairs inside one year.
fn month_day_distance(left: &OfficialDate, right: &OfficialDate) -> i64 {
    (right.month - left.month) * 31 + (right.day - left.day)
}

// ---------------------------------------------------------------------------
// Published-date clustering
// ---------------------------------------------------------------------------

/// The positions of the published dates that survive clustering.
///
/// Several published dates for ONE annual obligation can mean two different things:
/// instalments paid months apart (IRS pagamentos por conta is published as three
/// dates), or one deadline plus the business-day shift that follows it (the e-fatura
/// validation publishes 28 February AND 2 March 2026, because 28 February is a
/// Saturday).
///
/// Dates within a week of each other are therefore treated as a single deadline,
/// keeping the later — the last day you may actually comply. Dates further apart are
/// instalments. Without this, a weekend shift would be shown as a second payment the
/// person does not owe.
///
/// Indices rather than clones, so the annual-seed merge can tell "this very date"
/// apart from "another date with the same value".
fn cluster_indices(entries: &[OfficialDate]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|left, right| {
        (entries[*left].month, entries[*left].day).cmp(&(entries[*right].month, entries[*right].day))
    });
    let mut clusters: Vec<usize> = Vec::new();
    for index in order {
        match clusters.last() {
            Some(previous) if month_day_distance(&entries[*previous], &entries[index]) <= 7 => {
                if let Some(last) = clusters.last_mut() {
                    *last = index;
                }
            }
            _ => clusters.push(index),
        }
    }
    clusters
}

/// The published dates of one obligation, clustered into deadlines.
#[must_use]
pub fn cluster_published_dates(entries: &[OfficialDate]) -> Vec<OfficialDate> {
    cluster_indices(entries)
        .into_iter()
        .map(|index| entries[index].clone())
        .collect()
}

/// The published dates for a year, empty when the pack has none.
fn published_for(rule: &ObligationRule, year: i64) -> Vec<OfficialDate> {
    rule.official_dates
        .get(&year.to_string())
        .cloned()
        .unwrap_or_default()
}

/// The months a rule fell due in, taken from the most recent published year.
fn pattern_months(rule: &ObligationRule) -> Vec<i64> {
    let mut years: Vec<i64> = rule
        .official_dates
        .keys()
        .filter_map(|key| key.parse::<i64>().ok())
        .collect();
    years.sort_unstable_by(|left, right| right.cmp(left));
    let Some(latest) = years.first() else {
        return Vec::new();
    };
    cluster_published_dates(&published_for(rule, *latest))
        .iter()
        .map(|date| date.month)
        .collect()
}

// ---------------------------------------------------------------------------
// Deriving the legal date from the deadline rule
// ---------------------------------------------------------------------------

/// What the rule says about the day, given the month the period falls due in.
///
/// A `None` day means the rule genuinely does not fix one.
fn resolve_legal_day(deadline: Option<&DeadlineRule>, due_year: i64, due_month: i64) -> LegalDate {
    let Some(deadline) = deadline else {
        return LegalDate {
            day: None,
            notes: Vec::new(),
        };
    };
    match deadline.kind {
        DeadlineKind::DayOfMonth | DeadlineKind::FixedDate => LegalDate {
            day: deadline.day,
            notes: Vec::new(),
        },
        DeadlineKind::Range => {
            let mut notes = Vec::new();
            if let (Some(month_from), Some(day_from), Some(month_to), Some(day_to)) = (
                deadline.month_from,
                deadline.day_from,
                deadline.month_to,
                deadline.day_to,
            ) {
                notes.push(format!(
                    "Janela de cumprimento: de {} a {}. A data apresentada é o fim da janela.",
                    dates::format_pt_date(&iso(due_year, month_from, day_from)).unwrap_or_default(),
                    dates::format_pt_date(&iso(due_year, month_to, day_to)).unwrap_or_default(),
                ));
            }
            LegalDate {
                day: deadline.day_to.or(deadline.day),
                notes,
            }
        }
        DeadlineKind::LastDayOfMonth | DeadlineKind::MonthOfYear => LegalDate {
            day: Some(month_length(due_year, due_month)),
            notes: vec!["O pacote de regras não fixa o dia; foi usado o último dia do mês.".to_owned()],
        },
        DeadlineKind::DayRange => {
            let notes = match (deadline.day_from, deadline.day_to) {
                (Some(from), Some(to)) => vec![format!(
                    "Janela de pagamento entre os dias {from} e {to}. A data apresentada é o fim da \
                     janela."
                )],
                _ => Vec::new(),
            };
            LegalDate {
                day: deadline.day_to,
                notes,
            }
        }
        DeadlineKind::DaysAfterEvent => LegalDate {
            day: None,
            notes: vec!["Obrigação dependente de evento: não tem prazo fixo no calendário.".to_owned()],
        },
        DeadlineKind::RetentionYears => LegalDate {
            day: None,
            notes: Vec::new(),
        },
    }
}

/// The months an annual obligation falls due in, when the year is unpublished.
fn annual_due_months(rule: &ObligationRule) -> Vec<i64> {
    if let Some(explicit) = rule
        .deadline_rule
        .as_ref()
        .and_then(|rule| rule.month.or(rule.month_to))
    {
        return vec![explicit];
    }
    let pattern = pattern_months(rule);
    if pattern.is_empty() { vec![6] } else { pattern }
}

// ---------------------------------------------------------------------------
// Period enumeration
// ---------------------------------------------------------------------------

fn month_seeds(rule: &ObligationRule, target_year: i64) -> Result<Vec<PeriodSeed>, dates::DateError> {
    let lag = rule.periodicity.lag_months.unwrap_or(2);
    let mut seeds = Vec::with_capacity(12);
    for month in 1..=12_i64 {
        let range = dates::month_range(target_year, month)?;
        let shifted = dates::parse_iso(&dates::add_months(&range.start, lag)?)?;
        seeds.push(PeriodSeed {
            period_start: range.start,
            period_end: range.end,
            period_label: dates::format_pt_month_year(month, target_year)?,
            due_year: shifted.year,
            due_month: i64::from(shifted.month),
            pinned: false,
            pre_note: None,
        });
    }
    Ok(seeds)
}

fn quarter_seeds(rule: &ObligationRule, target_year: i64) -> Result<Vec<PeriodSeed>, dates::DateError> {
    let lag = rule.periodicity.lag_months.unwrap_or(2);
    let mut seeds = Vec::with_capacity(4);
    for quarter in 1..=4_i64 {
        let range = dates::quarter_range(target_year, quarter)?;
        let shifted = dates::parse_iso(&dates::add_months(&range.end, lag)?)?;
        seeds.push(PeriodSeed {
            period_start: range.start,
            period_end: range.end,
            period_label: dates::quarter_label(target_year, quarter),
            due_year: shifted.year,
            due_month: i64::from(shifted.month),
            pinned: false,
            pre_note: None,
        });
    }
    Ok(seeds)
}

/// An annual obligation that falls due in `target_year` reports on the **previous**
/// civil year: the Modelo 3 delivered in June 2026 reports 2025 income.
///
/// Instalments are handled here: when the published calendar holds several dates
/// months apart, each becomes its own instance, because each one is separately
/// payable and separately late.
fn annual_seeds(rule: &ObligationRule, target_year: i64) -> Result<Vec<PeriodSeed>, dates::DateError> {
    let period_year = target_year - 1;
    let period_start = iso(period_year, 1, 1);
    let period_end = iso(period_year, 12, 31);
    let published = published_for(rule, target_year);
    let clusters = cluster_indices(&published);

    if clusters.len() > 1 {
        let mut seeds = Vec::with_capacity(clusters.len());
        for (position, index) in clusters.iter().enumerate() {
            let entry = &published[*index];
            let label = match &entry.period_label {
                Some(label) => label.clone(),
                None if clusters.len() == 3 => {
                    format!(
                        "{}.º pagamento por conta (rendimentos de {period_year})",
                        position + 1
                    )
                }
                None => format!(
                    "prestação {} de {} (rendimentos de {period_year})",
                    position + 1,
                    clusters.len()
                ),
            };
            seeds.push(PeriodSeed {
                period_start: period_start.clone(),
                period_end: period_end.clone(),
                period_label: label,
                due_year: target_year,
                due_month: entry.month,
                pinned: true,
                pre_note: None,
            });
        }
        return Ok(seeds);
    }

    if let Some(index) = clusters.first() {
        let cluster = &published[*index];
        let merged: Vec<&OfficialDate> = published
            .iter()
            .enumerate()
            .filter(|(position, date)| *position != *index && month_day_distance(date, cluster) <= 7)
            .map(|(_, date)| date)
            .collect();
        if !merged.is_empty() {
            let merged_text = merged
                .iter()
                .map(|date| {
                    dates::format_pt_date(&iso(target_year, date.month, date.day)).unwrap_or_default()
                })
                .collect::<Vec<String>>()
                .join(", ");
            return Ok(vec![PeriodSeed {
                period_start,
                period_end,
                period_label: format!("rendimentos de {period_year}"),
                due_year: target_year,
                due_month: cluster.month,
                pinned: true,
                pre_note: Some(format!(
                    "Prazo legal de {merged_text}; a AT admite o cumprimento até {}.",
                    dates::format_pt_date(&iso(target_year, cluster.month, cluster.day)).unwrap_or_default()
                )),
            }]);
        }
    }

    let months = annual_due_months(rule);
    let count = months.len();
    Ok(months
        .iter()
        .enumerate()
        .map(|(index, month)| PeriodSeed {
            period_start: period_start.clone(),
            period_end: period_end.clone(),
            period_label: if count > 1 {
                format!("{}.ª prestação (rendimentos de {period_year})", index + 1)
            } else {
                format!("rendimentos de {period_year}")
            },
            due_year: target_year,
            due_month: *month,
            pinned: false,
            pre_note: None,
        })
        .collect())
}

fn continuous_seed(target_year: i64) -> PeriodSeed {
    PeriodSeed {
        period_start: iso(target_year, 1, 1),
        period_end: iso(target_year, 12, 31),
        period_label: format!("todo o ano {target_year}"),
        due_year: target_year,
        due_month: 12,
        pinned: false,
        pre_note: None,
    }
}

fn enumerate_periods(rule: &ObligationRule, target_year: i64) -> Result<Vec<PeriodSeed>, dates::DateError> {
    use crate::types::PeriodicityType;
    match rule.periodicity.kind {
        PeriodicityType::Monthly => month_seeds(rule, target_year),
        PeriodicityType::Quarterly => quarter_seeds(rule, target_year),
        PeriodicityType::Annual => annual_seeds(rule, target_year),
        PeriodicityType::Continuous => Ok(vec![continuous_seed(target_year)]),
        // An event-driven duty has no date. It is deliberately absent from the agenda
        // rather than given an invented one; the Diagnóstico page lists it.
        PeriodicityType::EventDriven => Ok(Vec::new()),
    }
}

// ---------------------------------------------------------------------------
// Resolving one due date
// ---------------------------------------------------------------------------

/// Resolve the date a seed falls due on.
fn resolve_due_date(
    rule: &ObligationRule,
    seed: &PeriodSeed,
    holidays: &[IsoDate],
) -> Result<ResolvedDate, dates::DateError> {
    let mut discrepancies: Vec<String> = Vec::new();
    let published = published_for(rule, seed.due_year);
    let matched = published
        .iter()
        .find(|entry| entry.month == seed.due_month)
        .cloned();
    let last_day = month_length(seed.due_year, seed.due_month);
    let clamp_day = |day: i64| day.min(last_day);

    if seed.pinned
        && let Some(matched) = &matched
    {
        let mut notes: Vec<String> = Vec::new();
        if let Some(note) = &matched.note {
            notes.push(note.clone());
        }
        if let Some(note) = &seed.pre_note {
            notes.push(note.clone());
        }
        return Ok(ResolvedDate {
            due_date: iso(seed.due_year, seed.due_month, clamp_day(matched.day)),
            source: DueDateSource::Official,
            adjusted_for_weekend: false,
            discrepancies: notes,
        });
    }

    let legal = resolve_legal_day(rule.deadline_rule.as_ref(), seed.due_year, seed.due_month);

    let Some(legal_day) = legal.day else {
        if let Some(matched) = &matched {
            let notes = match &matched.note {
                Some(note) => vec![note.clone()],
                None => Vec::new(),
            };
            return Ok(ResolvedDate {
                due_date: iso(seed.due_year, seed.due_month, clamp_day(matched.day)),
                source: DueDateSource::Official,
                adjusted_for_weekend: false,
                discrepancies: notes,
            });
        }
        let mut notes = legal.notes;
        notes.push(
            "Sem dia fixado e sem data publicada: foi usado o último dia do mês como data \
             provisória."
                .to_owned(),
        );
        return Ok(ResolvedDate {
            due_date: iso(seed.due_year, seed.due_month, last_day),
            source: DueDateSource::Derived,
            adjusted_for_weekend: false,
            discrepancies: notes,
        });
    };

    let legal_date = iso(seed.due_year, seed.due_month, clamp_day(legal_day));
    let business_date = dates::next_business_day(&legal_date, holidays)?;

    let Some(matched) = matched else {
        if !published.is_empty() {
            let published_text = cluster_published_dates(&published)
                .iter()
                .map(|entry| {
                    dates::format_pt_date(&iso(seed.due_year, entry.month, entry.day)).unwrap_or_default()
                })
                .collect::<Vec<String>>()
                .join(", ");
            discrepancies.push(format!(
                "A Agenda Fiscal de {} não publica qualquer prazo desta obrigação para {}. Datas \
                 publicadas nesse ano: {published_text}. A data apresentada foi calculada pela regra \
                 geral e deve ser confirmada na fonte citada.",
                seed.due_year,
                dates::format_pt_month_year(seed.due_month, seed.due_year).unwrap_or_default(),
            ));
        }
        let mut notes = legal.notes;
        notes.extend(discrepancies);
        return Ok(ResolvedDate {
            due_date: business_date.clone(),
            source: DueDateSource::Derived,
            adjusted_for_weekend: business_date != legal_date,
            discrepancies: notes,
        });
    };

    let published_date = iso(seed.due_year, seed.due_month, clamp_day(matched.day));
    let is_plain_rule = published_date == legal_date;
    let is_weekend_shift = published_date == business_date;

    if !is_plain_rule && !is_weekend_shift {
        discrepancies.push(format!(
            "Data publicada ({}) não coincide com a regra geral ({}) nem com o adiamento para o dia \
             útil seguinte ({}). Foi usada a data publicada.",
            dates::format_pt_date(&published_date).unwrap_or_default(),
            dates::format_pt_date(&legal_date).unwrap_or_default(),
            dates::format_pt_date(&business_date).unwrap_or_default(),
        ));
    }
    if is_weekend_shift && !is_plain_rule && dates::is_weekend(&legal_date)? {
        discrepancies.push(format!(
            "A data legal ({}) cai em fim de semana; a AT admite o cumprimento até {}.",
            dates::format_pt_date(&legal_date).unwrap_or_default(),
            dates::format_pt_date(&business_date).unwrap_or_default(),
        ));
    }
    if let Some(extension) = &matched.extension {
        discrepancies.push(format!("Prorrogação aplicada: {}.", extension.reason));
    }
    if let Some(note) = &matched.note
        && !discrepancies.contains(note)
    {
        discrepancies.push(note.clone());
    }
    for note in legal.notes {
        if !discrepancies.contains(&note) {
            discrepancies.push(note);
        }
    }

    Ok(ResolvedDate {
        due_date: published_date,
        source: DueDateSource::Official,
        adjusted_for_weekend: is_weekend_shift && !is_plain_rule,
        discrepancies,
    })
}

fn checklist_for(rule: &ObligationRule) -> Vec<ChecklistItem> {
    rule.documents_to_keep
        .iter()
        .map(|label| ChecklistItem {
            label: label.clone(),
            done: false,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The agenda
// ---------------------------------------------------------------------------

/// Build the agenda for one year.
///
/// Pure: same inputs, same output, no clock, no network, no model.
///
/// # Errors
///
/// [`dates::DateError`] when `options.today`, `options.track_from` or a pack date is
/// not an ISO date. A malformed date is refused rather than guessed at.
pub fn build_agenda(
    pack: &RulePack,
    profile: &TaxProfile,
    options: &AgendaOptions,
) -> Result<Vec<ObligationInstance>, dates::DateError> {
    let year = options.year;
    let due_soon_days = options.due_soon_days.unwrap_or(30);
    let mut instances: Vec<ObligationInstance> = Vec::new();

    for rule in &pack.obligations {
        if !applies_to(profile, &rule.applies_when) {
            instances.push(ObligationInstance {
                id: format!("{}@na", rule.id),
                rule_id: rule.id.clone(),
                kind: rule.kind,
                authority: rule.authority,
                tax: rule.tax.clone(),
                title: rule.title.clone(),
                period_label: "—".to_owned(),
                period_start: None,
                period_end: None,
                due_date: iso(year, 12, 31),
                due_date_source: DueDateSource::Derived,
                adjusted_for_weekend: false,
                provisional: false,
                status: InstanceStatus::NotApplicable,
                not_applicable_reason: explain_failure(profile, &rule.applies_when, FieldLabels::Portuguese),
                verification: rule.verification,
                verify_note: rule.verify_note.clone(),
                legal_basis: rule.legal_basis.clone(),
                source_ids: rule.source_ids.clone(),
                portal_url: rule.portal_url.clone(),
                documents_to_keep: rule.documents_to_keep.clone(),
                penalty_note: rule.penalty_note.clone(),
                discrepancies: Vec::new(),
                checklists: Vec::new(),
            });
            continue;
        }

        for seed in enumerate_periods(rule, year)? {
            let resolved = resolve_due_date(rule, &seed, &options.holidays)?;
            let due_date = resolved.due_date;
            let provisional = seed.due_year != pack.year;
            let id = format!("{}@{}", rule.id, due_date);

            let status = if options.completed_ids.contains(&id) {
                InstanceStatus::Done
            } else {
                let delta = dates::days_between(&options.today, &due_date)?;
                let base = if delta < 0 {
                    InstanceStatus::Overdue
                } else if delta <= due_soon_days {
                    InstanceStatus::DueSoon
                } else {
                    InstanceStatus::Future
                };
                // A saving/keeping obligation is never "late": it is a standing duty.
                if rule.kind == ObligationKind::Save {
                    InstanceStatus::Future
                } else {
                    base
                }
            };
            // Anything due before the person started tracking is history, not a miss.
            let status = match &options.track_from {
                Some(start) if status != InstanceStatus::Done && due_date.as_str() < start.as_str() => {
                    InstanceStatus::Untracked
                }
                _ => status,
            };

            instances.push(ObligationInstance {
                id,
                rule_id: rule.id.clone(),
                kind: rule.kind,
                authority: rule.authority,
                tax: rule.tax.clone(),
                title: rule.title.clone(),
                period_label: seed.period_label,
                period_start: Some(seed.period_start),
                period_end: Some(seed.period_end),
                due_date,
                due_date_source: resolved.source,
                adjusted_for_weekend: resolved.adjusted_for_weekend,
                provisional,
                status,
                not_applicable_reason: None,
                verification: rule.verification,
                verify_note: rule.verify_note.clone(),
                legal_basis: rule.legal_basis.clone(),
                source_ids: rule.source_ids.clone(),
                portal_url: rule.portal_url.clone(),
                documents_to_keep: rule.documents_to_keep.clone(),
                penalty_note: rule.penalty_note.clone(),
                discrepancies: resolved.discrepancies,
                checklists: checklist_for(rule),
            });
        }
    }

    instances.sort_by(|left, right| {
        left.due_date
            .cmp(&right.due_date)
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
    Ok(instances)
}

/// What is due within a window, and what is already late.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadlineWindow {
    /// The window asked for.
    pub horizon_days: i64,
    /// Instances still open, due between today and the horizon.
    pub actionable: Vec<ObligationInstance>,
    /// Instances past their date.
    pub overdue: Vec<ObligationInstance>,
}

/// Filter an agenda into the window the dashboard shows.
///
/// # Errors
///
/// [`dates::DateError`] when `today` or an instance's date is not an ISO date.
pub fn upcoming(
    instances: &[ObligationInstance],
    today: &str,
    horizon_days: i64,
) -> Result<DeadlineWindow, dates::DateError> {
    let mut actionable = Vec::new();
    let mut overdue = Vec::new();
    for instance in instances {
        if instance.status == InstanceStatus::Overdue {
            overdue.push(instance.clone());
        }
        if matches!(
            instance.status,
            InstanceStatus::NotApplicable | InstanceStatus::Done | InstanceStatus::Untracked
        ) {
            continue;
        }
        let delta = dates::days_between(today, &instance.due_date)?;
        if (0..=horizon_days).contains(&delta) {
            actionable.push(instance.clone());
        }
    }
    Ok(DeadlineWindow {
        horizon_days,
        actionable,
        overdue,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::load_embedded;
    use crate::types::{
        ActivityProfile, CaeEntry, CaeRole, IrsChoice, IrsRegime, IvaChoice, IvaRegime, SsChoice,
        Verification,
    };

    fn profile() -> TaxProfile {
        TaxProfile {
            nif: "123456789".to_owned(),
            name: "Pessoa Exemplo".to_owned(),
            resident_pt: true,
            is_company: false,
            activity: ActivityProfile {
                category_b: true,
                start_date: "2019-04-01".to_owned(),
                cae: vec![CaeEntry {
                    code: "62010".to_owned(),
                    description: "Atividades de programação informática".to_owned(),
                    role: CaeRole::Principal,
                }],
                turnover_previous_year_cents: None,
                turnover_current_year_expected_cents: None,
                intra_community_operations: false,
                intra_community_operations_above_50k: None,
                exports: false,
                has_employees: false,
                uses_certified_invoicing_software: true,
                uses_at_webservice: false,
            },
            tracking_start: None,
            iva: IvaChoice {
                regime: IvaRegime::Trimestral,
            },
            irs: IrsChoice {
                regime: IrsRegime::Simplificado,
                coefficient_bp: 7500,
            },
            ss: SsChoice {
                startup_exemption_active: false,
                first_activity_date: None,
            },
        }
    }

    fn agenda(
        profile: &TaxProfile,
        today: &str,
    ) -> Result<Vec<ObligationInstance>, Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        Ok(build_agenda(
            &pack.pack,
            profile,
            &AgendaOptions {
                year: 2026,
                today: today.to_owned(),
                due_soon_days: None,
                completed_ids: Vec::new(),
                track_from: None,
                holidays: Vec::new(),
            },
        )?)
    }

    #[test]
    fn the_smoke_profile_produces_the_reference_agenda() -> Result<(), Box<dyn std::error::Error>> {
        // The bar this port has to clear, taken from the reference implementation:
        // 74 instances, 7 not applicable, 45 from a published date, 29 derived.
        let instances = agenda(&profile(), "2026-09-20")?;
        assert_eq!(instances.len(), 74, "total instances");
        let count = |status: InstanceStatus| {
            instances
                .iter()
                .filter(|instance| instance.status == status)
                .count()
        };
        assert_eq!(count(InstanceStatus::NotApplicable), 7, "not applicable");
        let official = instances
            .iter()
            .filter(|instance| instance.due_date_source == DueDateSource::Official)
            .count();
        let derived = instances
            .iter()
            .filter(|instance| instance.due_date_source == DueDateSource::Derived)
            .count();
        assert_eq!((official, derived), (45, 29), "official and derived dates");
        Ok(())
    }

    #[test]
    fn every_rule_in_the_smoke_profile_appears_the_expected_number_of_times()
    -> Result<(), Box<dyn std::error::Error>> {
        let instances = agenda(&profile(), "2026-09-20")?;
        let expected: [(&str, usize); 19] = [
            ("ss.declaracaoAnual", 1),
            ("efatura.comunicacaoFaturas", 12),
            ("saft.mensal", 12),
            ("irs.retencoesPagamento", 12),
            ("ss.contribuicoesMensais", 12),
            ("efatura.validacaoDespesas", 1),
            ("ss.declaracaoTrimestral", 4),
            ("iva.dp.trimestral", 4),
            ("iva.pagamento.trimestral", 4),
            ("irs.modelo3", 1),
            ("irs.pagamentosPorConta", 3),
            ("guardar.documentos", 1),
            ("ies", 1),
            ("modelo22", 1),
            ("iva.dp.mensal", 1),
            ("iva.pagamento.mensal", 1),
            ("iva.recapitulativa", 1),
            ("iva.recapitulativa.trimestral", 1),
            ("iva.recapitulativa.mensalPorVolume", 1),
        ];
        for (rule_id, count) in expected {
            let actual = instances
                .iter()
                .filter(|instance| instance.rule_id == rule_id)
                .count();
            assert_eq!(actual, count, "rule {rule_id}");
        }
        Ok(())
    }

    #[test]
    fn a_weekend_deadline_moves_to_the_monday_and_says_why() -> Result<(), Box<dyn std::error::Error>> {
        // The pack publishes 28 February 2026 (a Saturday) AND 2 March for the
        // e-fatura validation. The two are one deadline, the later is the one shown,
        // and the instance explains that the authority allows the Monday.
        let instances = agenda(&profile(), "2026-09-20")?;
        let shifted = instances
            .iter()
            .find(|instance| instance.rule_id == "efatura.validacaoDespesas")
            .ok_or("the e-fatura validation must be in the agenda")?;
        assert_eq!(shifted.due_date, "2026-03-02");
        assert_eq!(shifted.due_date_source, DueDateSource::Official);
        assert_eq!(shifted.status, InstanceStatus::Overdue);
        let explanation = shifted.discrepancies.join(" | ");
        assert!(
            explanation.contains("28/02/2026") && explanation.contains("admite o cumprimento"),
            "the shift must be explained, was: {explanation}"
        );
        Ok(())
    }

    #[test]
    fn a_completed_obligation_is_done_and_stays_out_of_the_window() -> Result<(), Box<dyn std::error::Error>>
    {
        let pack = load_embedded()?;
        let first = build_agenda(
            &pack.pack,
            &profile(),
            &AgendaOptions {
                year: 2026,
                today: "2026-09-20".to_owned(),
                due_soon_days: None,
                completed_ids: Vec::new(),
                track_from: None,
                holidays: Vec::new(),
            },
        )?;
        let target = first
            .iter()
            .find(|instance| instance.status == InstanceStatus::Overdue)
            .ok_or("the fixture must have an overdue instance")?
            .id
            .clone();
        let after = build_agenda(
            &pack.pack,
            &profile(),
            &AgendaOptions {
                year: 2026,
                today: "2026-09-20".to_owned(),
                due_soon_days: None,
                completed_ids: vec![target.clone()],
                track_from: None,
                holidays: Vec::new(),
            },
        )?;
        let marked = after
            .iter()
            .find(|instance| instance.id == target)
            .ok_or("the completed instance must still be listed")?;
        assert_eq!(marked.status, InstanceStatus::Done);
        let window = upcoming(&after, "2026-09-20", 90)?;
        assert!(
            !window.actionable.iter().any(|instance| instance.id == target),
            "a done obligation is not actionable"
        );
        Ok(())
    }

    #[test]
    fn anything_due_before_tracking_started_is_history_not_a_miss() -> Result<(), Box<dyn std::error::Error>>
    {
        let pack = load_embedded()?;
        let instances = build_agenda(
            &pack.pack,
            &profile(),
            &AgendaOptions {
                year: 2026,
                today: "2026-09-20".to_owned(),
                due_soon_days: None,
                completed_ids: Vec::new(),
                track_from: Some("2026-09-01".to_owned()),
                holidays: Vec::new(),
            },
        )?;
        let untracked = instances
            .iter()
            .filter(|instance| instance.status == InstanceStatus::Untracked)
            .count();
        assert!(untracked > 0, "dates before the start are untracked");
        assert!(
            instances.iter().all(|instance| {
                instance.status != InstanceStatus::Overdue || instance.due_date.as_str() >= "2026-09-01"
            }),
            "nothing before the tracking start may be overdue"
        );
        Ok(())
    }

    #[test]
    fn an_unpublished_year_is_provisional_and_still_scheduled() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let instances = build_agenda(
            &pack.pack,
            &profile(),
            &AgendaOptions {
                year: 2027,
                today: "2027-03-01".to_owned(),
                due_soon_days: None,
                completed_ids: Vec::new(),
                track_from: None,
                holidays: Vec::new(),
            },
        )?;
        assert!(!instances.is_empty());
        assert!(
            instances
                .iter()
                .filter(|instance| instance.status != InstanceStatus::NotApplicable)
                .all(|instance| instance.provisional),
            "every date in an unpublished year is provisional"
        );
        // A period that ends near the year end falls due in the next one, so the
        // window is the requested year and the one after it — never the pack's year.
        assert!(
            instances
                .iter()
                .all(|instance| instance.due_date.as_str() >= "2027-01-01"
                    && instance.due_date.as_str() <= "2028-12-31"),
            "no date may fall back into the pack's own year"
        );
        Ok(())
    }

    #[test]
    fn the_agenda_is_byte_stable_across_runs() -> Result<(), Box<dyn std::error::Error>> {
        let first = agenda(&profile(), "2026-09-20")?;
        let second = agenda(&profile(), "2026-09-20")?;
        assert_eq!(first, second, "the engine must be deterministic");
        Ok(())
    }

    #[test]
    fn instances_are_sorted_by_date_then_rule() -> Result<(), Box<dyn std::error::Error>> {
        let instances = agenda(&profile(), "2026-09-20")?;
        for pair in instances.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            assert!(
                (&left.due_date, &left.rule_id) <= (&right.due_date, &right.rule_id),
                "{} must not come before {}",
                left.id,
                right.id
            );
        }
        Ok(())
    }

    #[test]
    fn a_not_applicable_instance_carries_its_reason() -> Result<(), Box<dyn std::error::Error>> {
        // An exempt taxpayer gets no periodic IVA declarations, and the row says why.
        let mut exempt = profile();
        exempt.iva.regime = IvaRegime::IsentoArt53;
        let instances = agenda(&exempt, "2026-09-20")?;
        let quarterly = instances
            .iter()
            .find(|instance| instance.rule_id == "iva.dp.trimestral")
            .ok_or("the exempt taxpayer must still see the rule")?;
        assert_eq!(quarterly.status, InstanceStatus::NotApplicable);
        let reason = quarterly.not_applicable_reason.clone().unwrap_or_default();
        assert!(reason.contains("regime de IVA"), "reason was: {reason}");
        Ok(())
    }

    #[test]
    fn clustering_keeps_instalments_apart_and_merges_a_weekend_shift() {
        let weekly = |month: i64, day: i64| OfficialDate {
            month,
            day,
            period_label: None,
            extension: None,
            note: None,
        };
        // Three instalments months apart stay three.
        let instalments = vec![weekly(7, 20), weekly(9, 20), weekly(12, 20)];
        assert_eq!(cluster_published_dates(&instalments).len(), 3);
        // A Saturday deadline and the following Monday merge into one, keeping Monday.
        let shifted = vec![weekly(2, 28), weekly(3, 2)];
        let merged = cluster_published_dates(&shifted);
        assert_eq!(merged.len(), 1);
        assert_eq!((merged[0].month, merged[0].day), (3, 2));
    }

    #[test]
    fn a_malformed_today_is_refused_rather_than_guessed() -> Result<(), Box<dyn std::error::Error>> {
        let pack = load_embedded()?;
        let result = build_agenda(
            &pack.pack,
            &profile(),
            &AgendaOptions {
                year: 2026,
                today: "hoje".to_owned(),
                due_soon_days: None,
                completed_ids: Vec::new(),
                track_from: None,
                holidays: Vec::new(),
            },
        );
        assert!(result.is_err(), "an unreadable today must be refused");
        Ok(())
    }

    #[test]
    fn verification_states_survive_onto_the_instance() -> Result<(), Box<dyn std::error::Error>> {
        let instances = agenda(&profile(), "2026-09-20")?;
        assert!(
            instances
                .iter()
                .any(|instance| instance.verification == Verification::Partial),
            "the pack has partly verified rules and the agenda must carry that"
        );
        Ok(())
    }
}
