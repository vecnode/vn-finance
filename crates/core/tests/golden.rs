//! Golden parity with the TypeScript reference implementation.
//!
//! Each fixture under `tests/golden/` was produced by `tools/golden/emit.ts` from
//! the reference `src/core/`, and this file asserts that the Rust port answers
//! every one of those inputs identically. Two kinds of case:
//!
//! - **samples**, so a failure names the value that broke;
//! - **digests**, a SHA-256 over a sweep whose bounds travel in the JSON, so the
//!   corpus covers a million formatter inputs and a decade of calendar days
//!   without a four-megabyte fixture, and neither side can quietly change the
//!   range.
//!
//! Regenerate the fixtures with `node tools/golden/emit.ts`. They are committed, so
//! CI needs no Node.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use vnf_core::calendar::{build_agenda, upcoming};
use vnf_core::conditions::{FieldLabels, explain_failure, test_condition};
use vnf_core::countries::{TurnoverScope, is_eu_country, scope_of};
use vnf_core::dates;
use vnf_core::estimate::{
    Invoice, default_invoice_terms, estimate_irs_simplified_base, estimate_reserve, estimate_social_security,
    invoice_totals, period_totals, quarter_report,
};
use vnf_core::flags::{FlagInput, FlagSummary, compute_flags, pack_health_flags, summarise_flags};
use vnf_core::money::{
    apply_bp, format_bp_as_coefficient, format_bp_as_percent, format_eur, parse_eur_to_cents, split_evenly,
};
use vnf_core::nif::{TaxNumberKind, classify_pt_tax_number, is_valid_pt_tax_number, mask_nif};
use vnf_core::profile::{
    IRS_REGIMES, IVA_REGIMES, ProfileDraft, ProfileProblem, build_profile, default_cae,
    missing_profile_inputs, normalise_imported_profile, validate_profile,
};
use vnf_core::rules::{FreshnessStatus, constant, declares_obligation, freshness, load_embedded};
use vnf_core::types::{
    AgendaOptions, CaeEntry, Condition, DueDateSource, InstanceStatus, ObligationInstance, ProblemLevel,
    TaxProfile, Verification,
};

type Problem = Box<dyn std::error::Error>;

// ---------------------------------------------------------------------------
// money
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct MoneyFixture {
    parse: Vec<ParseCase>,
    parse_rejects: Vec<String>,
    format: Vec<FormatCase>,
    format_digest: RangeDigest,
    parse_digest: RangeDigest,
    apply: Vec<ApplyCase>,
    apply_digest: ApplyDigest,
    split: Vec<SplitCase>,
    percent: Vec<RateCase>,
    percent_digest: RangeDigest,
    coefficient: Vec<RateCase>,
    coefficient_digest: RangeDigest,
}

#[derive(Deserialize)]
struct ParseCase {
    input: String,
    cents: i64,
}

#[derive(Deserialize)]
struct FormatCase {
    cents: i64,
    text: String,
}

#[derive(Deserialize)]
struct RangeDigest {
    from: i64,
    to: i64,
    sha256: String,
}

#[derive(Deserialize)]
struct ApplyCase {
    amount: i64,
    bp: i64,
    cents: i64,
}

#[derive(Deserialize)]
struct ApplyDigest {
    amount_from: i64,
    amount_to: i64,
    amount_step: i64,
    bps: Vec<i64>,
    sha256: String,
}

#[derive(Deserialize)]
struct SplitCase {
    total: i64,
    parts: u32,
    parts_cents: Vec<i64>,
}

#[derive(Deserialize)]
struct RateCase {
    bp: i64,
    text: String,
}

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The canonical text form of an amount, exactly as the emitter writes it.
fn amount_text(cents: i64) -> String {
    let magnitude = cents.unsigned_abs();
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{},{:02}", magnitude / 100, magnitude % 100)
}

#[test]
fn money_parses_what_the_reference_accepts() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    for case in &fixture.parse {
        assert_eq!(
            parse_eur_to_cents(&case.input),
            Ok(case.cents),
            "parsing {:?}",
            case.input
        );
    }
    Ok(())
}

#[test]
fn money_refuses_what_the_reference_refuses() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    for input in &fixture.parse_rejects {
        assert!(
            parse_eur_to_cents(input).is_err(),
            "{input:?} must be refused, as the reference refuses it"
        );
    }
    Ok(())
}

#[test]
fn money_formats_like_the_reference() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    for case in &fixture.format {
        assert_eq!(format_eur(case.cents), case.text, "formatting {}", case.cents);
    }
    Ok(())
}

#[test]
fn the_formatter_matches_a_million_inputs() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    let digest = &fixture.format_digest;
    let mut text = String::new();
    for cents in digest.from..=digest.to {
        text.push_str(&format!("{cents}{}", format_eur(cents)));
        text.push('\n');
    }
    assert_eq!(
        sha256_hex(&text),
        digest.sha256,
        "formatEur sweep {}..={}",
        digest.from,
        digest.to
    );
    Ok(())
}

#[test]
fn the_parser_matches_a_million_amounts() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    let digest = &fixture.parse_digest;
    let mut text = String::new();
    for cents in digest.from..=digest.to {
        let input = amount_text(cents);
        let parsed = parse_eur_to_cents(&input)?;
        text.push_str(&format!("{input}=>{parsed}"));
        text.push('\n');
    }
    assert_eq!(sha256_hex(&text), digest.sha256, "parseEurToCents sweep");
    Ok(())
}

#[test]
fn rates_apply_like_the_reference() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    for case in &fixture.apply {
        assert_eq!(
            apply_bp(case.amount, case.bp),
            case.cents,
            "applyBp({}, {})",
            case.amount,
            case.bp
        );
    }
    let digest = &fixture.apply_digest;
    let mut text = String::new();
    let mut amount = digest.amount_from;
    while amount <= digest.amount_to {
        for bp in &digest.bps {
            text.push_str(&format!("{amount},{bp},{}\n", apply_bp(amount, *bp)));
        }
        amount += digest.amount_step;
    }
    assert_eq!(sha256_hex(&text), digest.sha256, "applyBp grid");
    Ok(())
}

#[test]
fn splitting_like_the_reference() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    for case in &fixture.split {
        assert_eq!(
            split_evenly(case.total, case.parts),
            Ok(case.parts_cents.clone()),
            "splitEvenly({}, {})",
            case.total,
            case.parts
        );
    }
    Ok(())
}

#[test]
fn rates_display_like_the_reference() -> Result<(), Problem> {
    let fixture: MoneyFixture = serde_json::from_str(include_str!("golden/money.json"))?;
    for case in &fixture.percent {
        assert_eq!(format_bp_as_percent(case.bp), case.text, "percent of {}", case.bp);
    }
    for case in &fixture.coefficient {
        assert_eq!(
            format_bp_as_coefficient(case.bp),
            case.text,
            "coefficient of {}",
            case.bp
        );
    }
    let mut percent_text = String::new();
    let mut coefficient_text = String::new();
    for bp in fixture.percent_digest.from..=fixture.percent_digest.to {
        percent_text.push_str(&format!("{bp},{}\n", format_bp_as_percent(bp)));
        coefficient_text.push_str(&format!("{bp},{}\n", format_bp_as_coefficient(bp)));
    }
    assert_eq!(
        sha256_hex(&percent_text),
        fixture.percent_digest.sha256,
        "percent sweep"
    );
    assert_eq!(
        sha256_hex(&coefficient_text),
        fixture.coefficient_digest.sha256,
        "coefficient sweep"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// dates
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DatesFixture {
    samples: Vec<DateSample>,
    sweep_digest: SweepDigest,
    between: Vec<BetweenCase>,
    quarters: Vec<SpanCase>,
    months: Vec<SpanCase>,
    month_year: Vec<MonthYearCase>,
    quarter_labels: Vec<QuarterLabelCase>,
    rejects: Vec<String>,
}

#[derive(Deserialize)]
struct DateSample {
    iso: String,
    parts: Parts,
    days_in_month: u32,
    day_of_week: u32,
    is_weekend: bool,
    add_days: Vec<AddDaysCase>,
    add_months: Vec<AddMonthsCase>,
    format_pt_date: String,
}

#[derive(Deserialize)]
struct Parts {
    year: i64,
    month: u32,
    day: u32,
}

#[derive(Deserialize)]
struct AddDaysCase {
    days: i64,
    iso: String,
}

#[derive(Deserialize)]
struct AddMonthsCase {
    months: i64,
    iso: String,
}

#[derive(Deserialize)]
struct SweepDigest {
    from: String,
    to: String,
    sha256: String,
}

#[derive(Deserialize)]
struct BetweenCase {
    from: String,
    to: String,
    days: i64,
}

#[derive(Deserialize)]
struct SpanCase {
    start: String,
    end: String,
    year: i64,
    quarter: Option<i64>,
    month: Option<i64>,
}

#[derive(Deserialize)]
struct MonthYearCase {
    month: i64,
    year: i64,
    text: String,
}

#[derive(Deserialize)]
struct QuarterLabelCase {
    quarter: i64,
    year: i64,
    text: String,
}

#[test]
fn dates_project_like_the_reference() -> Result<(), Problem> {
    let fixture: DatesFixture = serde_json::from_str(include_str!("golden/dates.json"))?;
    for case in &fixture.samples {
        let parts = dates::parse_iso(&case.iso)?;
        assert_eq!(
            (parts.year, parts.month, parts.day),
            (case.parts.year, case.parts.month, case.parts.day),
            "parts of {}",
            case.iso
        );
        assert_eq!(dates::days_in_month(parts.year, parts.month), case.days_in_month);
        assert_eq!(
            dates::day_of_week(&case.iso)?,
            case.day_of_week,
            "weekday of {}",
            case.iso
        );
        assert_eq!(
            dates::is_weekend(&case.iso)?,
            case.is_weekend,
            "weekend of {}",
            case.iso
        );
        assert_eq!(dates::format_pt_date(&case.iso)?, case.format_pt_date);
        for shift in &case.add_days {
            assert_eq!(
                dates::add_days(&case.iso, shift.days)?,
                shift.iso,
                "{} + {} days",
                case.iso,
                shift.days
            );
        }
        for shift in &case.add_months {
            assert_eq!(
                dates::add_months(&case.iso, shift.months)?,
                shift.iso,
                "{} + {} months",
                case.iso,
                shift.months
            );
        }
    }
    Ok(())
}

#[test]
fn the_calendar_matches_a_decade_of_days() -> Result<(), Problem> {
    let fixture: DatesFixture = serde_json::from_str(include_str!("golden/dates.json"))?;
    let digest = &fixture.sweep_digest;
    let mut text = String::new();
    let mut iso = digest.from.clone();
    while iso <= digest.to {
        text.push_str(&format!(
            "{iso}|{}|{}|{}|{}|{}|{}\n",
            dates::day_of_week(&iso)?,
            dates::is_weekend(&iso)?,
            dates::add_days(&iso, 1)?,
            dates::add_months(&iso, 1)?,
            dates::add_months(&iso, -1)?,
            dates::format_pt_date(&iso)?,
        ));
        iso = dates::add_days(&iso, 1)?;
    }
    assert_eq!(
        sha256_hex(&text),
        digest.sha256,
        "calendar sweep {}..={}",
        digest.from,
        digest.to
    );
    Ok(())
}

#[test]
fn day_counts_match_the_reference() -> Result<(), Problem> {
    let fixture: DatesFixture = serde_json::from_str(include_str!("golden/dates.json"))?;
    for case in &fixture.between {
        assert_eq!(
            dates::days_between(&case.from, &case.to)?,
            case.days,
            "days between {} and {}",
            case.from,
            case.to
        );
    }
    Ok(())
}

#[test]
fn periods_match_the_reference() -> Result<(), Problem> {
    let fixture: DatesFixture = serde_json::from_str(include_str!("golden/dates.json"))?;
    for case in &fixture.quarters {
        let quarter = case.quarter.unwrap_or(0);
        let span = dates::quarter_range(case.year, quarter)?;
        assert_eq!((span.start, span.end), (case.start.clone(), case.end.clone()));
    }
    for case in &fixture.months {
        let month = case.month.unwrap_or(0);
        let span = dates::month_range(case.year, month)?;
        assert_eq!((span.start, span.end), (case.start.clone(), case.end.clone()));
    }
    Ok(())
}

#[test]
fn display_strings_match_the_reference() -> Result<(), Problem> {
    let fixture: DatesFixture = serde_json::from_str(include_str!("golden/dates.json"))?;
    for case in &fixture.month_year {
        assert_eq!(dates::format_pt_month_year(case.month, case.year)?, case.text);
    }
    for case in &fixture.quarter_labels {
        assert_eq!(dates::quarter_label(case.year, case.quarter), case.text);
    }
    Ok(())
}

#[test]
fn dates_refuse_what_the_reference_refuses() -> Result<(), Problem> {
    let fixture: DatesFixture = serde_json::from_str(include_str!("golden/dates.json"))?;
    for input in &fixture.rejects {
        assert!(dates::parse_iso(input).is_err(), "{input:?} must be refused");
    }
    assert!(dates::quarter_range(2026, 0).is_err());
    assert!(dates::quarter_range(2026, 5).is_err());
    assert!(dates::month_range(2026, 0).is_err());
    assert!(dates::month_range(2026, 13).is_err());
    Ok(())
}

// ---------------------------------------------------------------------------
// nif
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct NifFixture {
    valid: Vec<String>,
    invalid: Vec<String>,
    classify: Vec<ClassifyCase>,
    mask: Vec<MaskCase>,
}

#[derive(Deserialize)]
struct ClassifyCase {
    nif: String,
    kind: Option<String>,
}

#[derive(Deserialize)]
struct MaskCase {
    nif: String,
    masked: String,
}

/// Map the reference's `'NIF' | 'NIPC' | null` onto the Rust enum.
fn kind_of(label: Option<&str>) -> Option<TaxNumberKind> {
    match label {
        Some("NIF") => Some(TaxNumberKind::Nif),
        Some("NIPC") => Some(TaxNumberKind::Nipc),
        _ => None,
    }
}

#[test]
fn tax_numbers_match_the_reference() -> Result<(), Problem> {
    let fixture: NifFixture = serde_json::from_str(include_str!("golden/nif.json"))?;
    for valid in &fixture.valid {
        assert!(is_valid_pt_tax_number(valid), "{valid} must be accepted");
    }
    for invalid in &fixture.invalid {
        assert!(!is_valid_pt_tax_number(invalid), "{invalid} must be refused");
    }
    for case in &fixture.classify {
        assert_eq!(
            classify_pt_tax_number(&case.nif),
            kind_of(case.kind.as_deref()),
            "classifying {}",
            case.nif
        );
    }
    for case in &fixture.mask {
        assert_eq!(mask_nif(&case.nif), case.masked, "masking {}", case.nif);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// countries
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CountriesFixture {
    scope: Vec<ScopeCase>,
    is_eu: Vec<IsEuCase>,
}

#[derive(Deserialize)]
struct ScopeCase {
    input: String,
    scope: String,
}

#[derive(Deserialize)]
struct IsEuCase {
    input: String,
    is_eu: bool,
}

fn scope_named(name: &str) -> TurnoverScope {
    match name {
        "national" => TurnoverScope::National,
        "eu" => TurnoverScope::Eu,
        _ => TurnoverScope::NonEu,
    }
}

#[test]
fn client_countries_match_the_reference() -> Result<(), Problem> {
    let fixture: CountriesFixture = serde_json::from_str(include_str!("golden/countries.json"))?;
    for case in &fixture.scope {
        assert_eq!(
            scope_of(&case.input),
            scope_named(&case.scope),
            "scope of {:?}",
            case.input
        );
    }
    for case in &fixture.is_eu {
        assert_eq!(
            is_eu_country(&case.input),
            case.is_eu,
            "is_eu of {:?}",
            case.input
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// rules
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RulesFixture {
    checksum: String,
    year: i64,
    pack_version: String,
    obligation_ids: Vec<String>,
    summary: SummaryCase,
    problems: Vec<ProblemCase>,
    freshness: Vec<FreshnessCase>,
    declares: Vec<DeclareCase>,
    constant_reads: Vec<ConstantRead>,
}

#[derive(Deserialize, PartialEq, Debug)]
struct SummaryCase {
    pack_version: String,
    year: i64,
    checksum: String,
    sources: usize,
    official_sources: usize,
    obligations: usize,
    verified: usize,
    partial: usize,
    unverified: usize,
    stale: usize,
    event_driven: usize,
    todo: Vec<String>,
    null_constants: Vec<String>,
}

#[derive(Deserialize, PartialEq, Debug)]
struct ProblemCase {
    level: String,
    path: String,
    message: String,
}

#[derive(Deserialize)]
struct FreshnessCase {
    today: String,
    status: String,
    message: String,
}

#[derive(Deserialize)]
struct DeclareCase {
    id: String,
    declared: bool,
}

#[derive(Deserialize)]
struct ConstantRead {
    path: String,
    value: Value,
}

fn level_text(level: ProblemLevel) -> &'static str {
    match level {
        ProblemLevel::Error => "error",
        ProblemLevel::Warning => "warning",
    }
}

fn freshness_text(status: FreshnessStatus) -> &'static str {
    match status {
        FreshnessStatus::Current => "current",
        FreshnessStatus::Stale => "stale",
        FreshnessStatus::Future => "future",
    }
}

#[test]
fn the_pack_matches_the_reference() -> Result<(), Problem> {
    let fixture: RulesFixture = serde_json::from_str(include_str!("golden/rules.json"))?;
    let loaded = load_embedded()?;

    assert_eq!(loaded.pack.year, fixture.year);
    assert_eq!(loaded.pack.pack_version, fixture.pack_version);
    assert_eq!(
        loaded.checksum, fixture.checksum,
        "the checksum must be byte-identical, or every stored figure loses its provenance"
    );

    let ids: Vec<String> = loaded
        .pack
        .obligations
        .iter()
        .map(|rule| rule.id.clone())
        .collect();
    assert_eq!(ids, fixture.obligation_ids, "the obligation ids and their order");

    let summary = vnf_core::rules::summarise_pack(&loaded);
    let ours = SummaryCase {
        pack_version: summary.pack_version,
        year: summary.year,
        checksum: summary.checksum,
        sources: summary.sources,
        official_sources: summary.official_sources,
        obligations: summary.obligations,
        verified: summary.verified,
        partial: summary.partial,
        unverified: summary.unverified,
        stale: summary.stale,
        event_driven: summary.event_driven,
        todo: summary.todo,
        null_constants: summary.null_constants,
    };
    assert_eq!(ours, fixture.summary, "the pack summary");

    let problems: Vec<ProblemCase> = loaded
        .problems
        .iter()
        .map(|problem| ProblemCase {
            level: level_text(problem.level).to_owned(),
            path: problem.path.clone(),
            message: problem.message.clone(),
        })
        .collect();
    assert_eq!(problems, fixture.problems, "every validator problem, in order");

    for case in &fixture.freshness {
        let state = freshness(&loaded.pack, &case.today)?;
        assert_eq!(
            freshness_text(state.status),
            case.status,
            "freshness of {}",
            case.today
        );
        assert_eq!(state.message, case.message, "message for {}", case.today);
    }

    for case in &fixture.declares {
        assert_eq!(
            declares_obligation(&loaded.pack, &case.id),
            case.declared,
            "declares {}",
            case.id
        );
    }

    for case in &fixture.constant_reads {
        let path: Vec<&str> = case.path.split('.').collect();
        let ours = constant(&loaded.pack, &path).cloned().unwrap_or(Value::Null);
        assert_eq!(ours, case.value, "constant {}", case.path);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// calendar
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CalendarFixture {
    cases: Vec<CalendarCase>,
}

#[derive(Deserialize)]
struct CalendarCase {
    name: String,
    profile: TaxProfile,
    year: i64,
    today: String,
    due_soon_days: i64,
    completed_ids: Vec<String>,
    track_from: Option<String>,
    holidays: Vec<String>,
    counts: Counts,
    ids: Vec<String>,
    digest: String,
    window: WindowCase,
}

#[derive(Deserialize, PartialEq, Debug)]
struct Counts {
    total: usize,
    not_applicable: usize,
    overdue: usize,
    due_soon: usize,
    future: usize,
    done: usize,
    untracked: usize,
    official: usize,
    derived: usize,
    provisional: usize,
}

#[derive(Deserialize, PartialEq, Debug)]
struct WindowCase {
    horizon_days: i64,
    actionable: Vec<String>,
    overdue: Vec<String>,
}

fn status_text(status: InstanceStatus) -> &'static str {
    match status {
        InstanceStatus::Future => "future",
        InstanceStatus::DueSoon => "due_soon",
        InstanceStatus::Overdue => "overdue",
        InstanceStatus::Done => "done",
        InstanceStatus::Untracked => "untracked",
        InstanceStatus::NotApplicable => "not_applicable",
    }
}

fn source_text(source: DueDateSource) -> &'static str {
    match source {
        DueDateSource::Official => "official",
        DueDateSource::Derived => "derived",
    }
}

fn verification_text(state: Verification) -> &'static str {
    match state {
        Verification::Verified => "verified",
        Verification::Partial => "partial",
        Verification::Unverified => "unverified",
        Verification::Stale => "stale",
    }
}

/// One instance as a single line, in exactly the shape the emitter writes.
fn instance_line(instance: &ObligationInstance) -> String {
    let checklist: Vec<&str> = instance
        .checklists
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        instance.id,
        status_text(instance.status),
        instance.due_date,
        source_text(instance.due_date_source),
        instance.adjusted_for_weekend,
        instance.provisional,
        instance.period_label,
        instance.period_start.as_deref().unwrap_or(""),
        instance.period_end.as_deref().unwrap_or(""),
        verification_text(instance.verification),
        instance.discrepancies.join(" ¶ "),
        instance.not_applicable_reason.as_deref().unwrap_or(""),
        checklist.join(" ¶ "),
    )
}

fn count_of(instances: &[ObligationInstance], status: InstanceStatus) -> usize {
    instances
        .iter()
        .filter(|instance| instance.status == status)
        .count()
}

#[test]
fn the_calendar_matches_the_reference() -> Result<(), Problem> {
    let fixture: CalendarFixture = serde_json::from_str(include_str!("golden/calendar.json"))?;
    let pack = load_embedded()?;
    assert!(fixture.cases.len() >= 16, "the corpus must stay wide");

    for case in &fixture.cases {
        let options = AgendaOptions {
            year: case.year,
            today: case.today.clone(),
            due_soon_days: Some(case.due_soon_days),
            completed_ids: case.completed_ids.clone(),
            track_from: case.track_from.clone(),
            holidays: case.holidays.clone(),
        };
        let instances = build_agenda(&pack.pack, &case.profile, &options)
            .map_err(|error| format!("{}: {error}", case.name))?;

        let ids: Vec<String> = instances.iter().map(|instance| instance.id.clone()).collect();
        assert_eq!(ids, case.ids, "the ids of every instance — {}", case.name);

        let counts = Counts {
            total: instances.len(),
            not_applicable: count_of(&instances, InstanceStatus::NotApplicable),
            overdue: count_of(&instances, InstanceStatus::Overdue),
            due_soon: count_of(&instances, InstanceStatus::DueSoon),
            future: count_of(&instances, InstanceStatus::Future),
            done: count_of(&instances, InstanceStatus::Done),
            untracked: count_of(&instances, InstanceStatus::Untracked),
            official: instances
                .iter()
                .filter(|instance| instance.due_date_source == DueDateSource::Official)
                .count(),
            derived: instances
                .iter()
                .filter(|instance| instance.due_date_source == DueDateSource::Derived)
                .count(),
            provisional: instances.iter().filter(|instance| instance.provisional).count(),
        };
        assert_eq!(counts, case.counts, "the counts — {}", case.name);

        let lines: Vec<String> = instances.iter().map(instance_line).collect();
        let expected = sha256_hex(&format!("{}\n", lines.join("\n")));
        assert_eq!(
            expected, case.digest,
            "every field of every instance — {}",
            case.name
        );

        let window = upcoming(&instances, &case.today, case.window.horizon_days)?;
        let ours = WindowCase {
            horizon_days: window.horizon_days,
            actionable: window
                .actionable
                .iter()
                .map(|instance| instance.id.clone())
                .collect(),
            overdue: window
                .overdue
                .iter()
                .map(|instance| instance.id.clone())
                .collect(),
        };
        assert_eq!(ours, case.window, "the dashboard window — {}", case.name);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// conditions
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ConditionsFixture {
    profiles: BTreeMap<String, TaxProfile>,
    cases: Vec<ConditionCase>,
}

#[derive(Deserialize)]
struct ConditionCase {
    profile: String,
    condition: Condition,
    applies: bool,
    reason: Option<String>,
}

#[test]
fn conditions_match_the_reference() -> Result<(), Problem> {
    let fixture: ConditionsFixture = serde_json::from_str(include_str!("golden/conditions.json"))?;
    assert!(
        fixture.cases.len() >= 300,
        "every field crossed with every operator"
    );
    for case in &fixture.cases {
        let profile = fixture
            .profiles
            .get(&case.profile)
            .ok_or("a profile must be in the fixture")?;
        assert_eq!(
            test_condition(profile, &case.condition),
            case.applies,
            "{:?} against {}",
            case.condition,
            case.profile
        );
        assert_eq!(
            explain_failure(profile, core::slice::from_ref(&case.condition), FieldLabels::Raw),
            case.reason,
            "the reason for {:?} against {}",
            case.condition,
            case.profile
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// profile
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ProfileFixture {
    default_cae: CaeEntry,
    iva_regimes: Vec<String>,
    irs_regimes: Vec<String>,
    builds: Vec<BuildCase>,
    imports: Vec<ImportCase>,
    missing: Vec<MissingCase>,
    validation: Vec<ValidationCase>,
}

#[derive(Deserialize)]
struct BuildCase {
    name: String,
    draft: ProfileDraft,
    current: Option<TaxProfile>,
    coefficient_bp: Option<i64>,
    profile: Option<TaxProfile>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ImportCase {
    name: String,
    input: Value,
    profile: Option<TaxProfile>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct MissingCase {
    name: String,
    profile: TaxProfile,
    inputs: Vec<String>,
}

#[derive(Deserialize)]
struct ValidationCase {
    name: String,
    profile: TaxProfile,
    problems: Vec<ProfileProblem>,
}

/// The wire names of an enum, for comparing against a fixture's string list.
fn names_of<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| match serde_json::to_value(value) {
            Ok(Value::String(name)) => Some(name),
            _ => None,
        })
        .collect()
}

#[test]
fn the_profile_builder_matches_the_reference() -> Result<(), Problem> {
    let fixture: ProfileFixture = serde_json::from_str(include_str!("golden/profile.json"))?;

    assert_eq!(default_cae(), fixture.default_cae, "the CAE the interface offers");
    assert_eq!(names_of(&IVA_REGIMES), fixture.iva_regimes);
    assert_eq!(names_of(&IRS_REGIMES), fixture.irs_regimes);
    assert_eq!(fixture.builds.len(), 8, "the corpus must stay wide");

    for case in &fixture.builds {
        match build_profile(&case.draft, case.current.as_ref(), case.coefficient_bp) {
            Ok(profile) => {
                let expected = case
                    .profile
                    .as_ref()
                    .ok_or_else(|| format!("{}: expected a refusal, got a profile", case.name))?;
                assert_eq!(&profile, expected, "the profile for {}", case.name);
            }
            Err(error) => {
                let expected = case
                    .error
                    .as_ref()
                    .ok_or_else(|| format!("{}: expected a profile, got {error}", case.name))?;
                assert_eq!(error.to_string(), *expected, "the refusal for {}", case.name);
            }
        }
    }
    Ok(())
}

#[test]
fn the_profile_import_matches_the_reference() -> Result<(), Problem> {
    let fixture: ProfileFixture = serde_json::from_str(include_str!("golden/profile.json"))?;
    assert!(fixture.imports.len() >= 9);
    for case in &fixture.imports {
        match normalise_imported_profile(&case.input) {
            Ok(profile) => {
                let expected = case
                    .profile
                    .as_ref()
                    .ok_or_else(|| format!("{}: expected a refusal, got a profile", case.name))?;
                assert_eq!(&profile, expected, "the imported profile for {}", case.name);
            }
            Err(error) => {
                let expected = case
                    .error
                    .as_ref()
                    .ok_or_else(|| format!("{}: expected a profile, got {error}", case.name))?;
                assert_eq!(error.to_string(), *expected, "the refusal for {}", case.name);
            }
        }
    }
    Ok(())
}

#[test]
fn the_missing_inputs_and_validation_match_the_reference() -> Result<(), Problem> {
    let fixture: ProfileFixture = serde_json::from_str(include_str!("golden/profile.json"))?;
    for case in &fixture.missing {
        assert_eq!(
            missing_profile_inputs(&case.profile),
            case.inputs,
            "the missing inputs for {}",
            case.name
        );
    }
    // The corpus avoids the start-up exemption on purpose: the reference reads the
    // clock for that one warning, so it is covered by a unit test with a date.
    for case in &fixture.validation {
        assert_eq!(
            validate_profile(&case.profile, "2026-09-20"),
            case.problems,
            "the problems for {}",
            case.name
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// estimates
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct EstimateFixture {
    invoices: Vec<Invoice>,
    invoice_totals: Vec<InvoiceTotalsCase>,
    default_terms: Vec<TermsCase>,
    period_totals: Vec<PeriodCase>,
    social_security: Vec<SocialSecurityCase>,
    quarters: Vec<QuarterCase>,
    irs_base: Vec<IrsCase>,
    reserve: Vec<ReserveCase>,
}

#[derive(Deserialize)]
struct InvoiceTotalsCase {
    invoice: Invoice,
    totals: Value,
}

#[derive(Deserialize)]
struct TermsCase {
    #[serde(default)]
    profile: Option<TaxProfile>,
    country: String,
    terms: Value,
}

#[derive(Deserialize)]
struct PeriodCase {
    start: String,
    end: String,
    totals: Value,
}

#[derive(Deserialize)]
struct SocialSecurityCase {
    service_income_cents: i64,
    estimate: Value,
}

#[derive(Deserialize)]
struct QuarterCase {
    year: i64,
    quarter: i64,
    report: Value,
}

#[derive(Deserialize)]
struct IrsCase {
    year: i64,
    eligible_expenses_cents: i64,
    profile: TaxProfile,
    estimate: Value,
}

#[derive(Deserialize)]
struct ReserveCase {
    as_of: String,
    low_bp: i64,
    high_bp: i64,
    estimate: Value,
}

#[test]
fn the_estimates_match_the_reference() -> Result<(), Problem> {
    let fixture: EstimateFixture = serde_json::from_str(include_str!("golden/estimate.json"))?;
    let pack = load_embedded()?;

    for case in &fixture.invoice_totals {
        let ours = serde_json::to_value(invoice_totals(&case.invoice))?;
        assert_eq!(ours, case.totals, "the totals of invoice {}", case.invoice.number);
    }
    assert_eq!(
        fixture.default_terms.len(),
        15,
        "three profiles by five countries"
    );
    for case in &fixture.default_terms {
        let ours = serde_json::to_value(default_invoice_terms(
            &pack.pack,
            case.profile.as_ref(),
            &case.country,
        ))?;
        assert_eq!(ours, case.terms, "the default terms for {}", case.country);
    }
    for case in &fixture.period_totals {
        let ours = serde_json::to_value(period_totals(&fixture.invoices, &case.start, &case.end))?;
        assert_eq!(ours, case.totals, "the period {}..{}", case.start, case.end);
    }
    for case in &fixture.social_security {
        let ours = serde_json::to_value(estimate_social_security(&pack.pack, case.service_income_cents, 3))?;
        assert_eq!(
            ours, case.estimate,
            "Segurança Social on {}",
            case.service_income_cents
        );
    }
    for case in &fixture.quarters {
        let report = quarter_report(&pack.pack, &fixture.invoices, case.year, case.quarter)?;
        let ours = serde_json::to_value(report)?;
        assert_eq!(
            ours, case.report,
            "the report for {}-Q{}",
            case.year, case.quarter
        );
    }
    for case in &fixture.irs_base {
        let ours = serde_json::to_value(estimate_irs_simplified_base(
            &pack.pack,
            &case.profile,
            &fixture.invoices,
            case.year,
            case.eligible_expenses_cents,
        ))?;
        assert_eq!(
            ours, case.estimate,
            "the IRS base with {} documented",
            case.eligible_expenses_cents
        );
    }
    for case in &fixture.reserve {
        let ours = serde_json::to_value(estimate_reserve(
            &pack.pack,
            &fixture.invoices,
            &case.as_of,
            case.low_bp,
            case.high_bp,
        ))?;
        assert_eq!(ours, case.estimate, "the reserve as of {}", case.as_of);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// flags
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FlagsFixture {
    cases: Vec<FlagCase>,
    pack_health: Vec<PackHealthCase>,
}

#[derive(Deserialize)]
struct FlagCase {
    name: String,
    profile: TaxProfile,
    invoices: Vec<Invoice>,
    today: String,
    documented: Vec<String>,
    due_soon_days: i64,
    flags: Value,
    summary: FlagSummary,
}

#[derive(Deserialize)]
struct PackHealthCase {
    today: String,
    flags: Value,
}

#[test]
fn the_flags_match_the_reference() -> Result<(), Problem> {
    let fixture: FlagsFixture = serde_json::from_str(include_str!("golden/flags.json"))?;
    let pack = load_embedded()?;
    assert!(fixture.cases.len() >= 13, "the corpus must stay wide");

    for case in &fixture.cases {
        let instances = build_agenda(
            &pack.pack,
            &case.profile,
            &AgendaOptions {
                year: 2026,
                today: case.today.clone(),
                due_soon_days: Some(case.due_soon_days),
                completed_ids: Vec::new(),
                track_from: None,
                holidays: Vec::new(),
            },
        )?;
        let flags = compute_flags(FlagInput {
            pack: &pack.pack,
            profile: &case.profile,
            invoices: &case.invoices,
            instances: &instances,
            today: case.today.clone(),
            documented_obligation_ids: case.documented.clone(),
            due_soon_days: Some(case.due_soon_days),
        })?;
        let ours = serde_json::to_value(&flags)?;
        assert_eq!(ours, case.flags, "the flag list for {}", case.name);
        assert_eq!(
            summarise_flags(&flags),
            case.summary,
            "the summary for {}",
            case.name
        );
    }

    for case in &fixture.pack_health {
        let ours = serde_json::to_value(pack_health_flags(&pack.pack, &case.today)?)?;
        assert_eq!(ours, case.flags, "the pack health on {}", case.today);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// pdf and receipt
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PdfFixture {
    cases: Vec<PdfCase>,
}

#[derive(Deserialize)]
struct PdfCase {
    name: String,
    #[serde(rename = "bytesHex")]
    bytes_hex: String,
    pdf: PdfExpectation,
    draft: Value,
}

#[derive(Deserialize)]
struct PdfExpectation {
    #[serde(rename = "pageCount")]
    page_count: usize,
    rows: Vec<String>,
    text: String,
    problems: Vec<String>,
}

fn from_hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            let high = char::from(*pair.first()?).to_digit(16)?;
            let low = char::from(*pair.get(1)?).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()
        })
        .collect()
}

/// The reader and the parser, on the same bytes the reference read.
///
/// The draft is compared whole rather than field by field: a divergence anywhere is a
/// divergence, and choosing which fields to compare would be choosing which mistakes to
/// notice.
#[test]
fn the_reference_documents_are_read_identically() -> Result<(), Problem> {
    let fixture: PdfFixture = serde_json::from_str(include_str!("golden/pdf.json"))?;
    assert!(fixture.cases.len() >= 6, "the corpus must stay wide");

    for case in &fixture.cases {
        let bytes = from_hex(&case.bytes_hex);
        assert!(
            !bytes.is_empty(),
            "{}: the bytes did not survive the fixture",
            case.name
        );

        let pdf = vnf_core::pdf::extract_pdf_text(&bytes);
        assert_eq!(pdf.page_count, case.pdf.page_count, "{}: page count", case.name);
        assert_eq!(pdf.rows, case.pdf.rows, "{}: rows", case.name);
        assert_eq!(pdf.text, case.pdf.text, "{}: text", case.name);
        assert_eq!(pdf.problems, case.pdf.problems, "{}: problems", case.name);

        let draft = vnf_core::receipt::parse_receipt(&pdf);
        let ours = serde_json::to_value(&draft)?;
        assert_eq!(ours, case.draft, "{}: the parsed draft", case.name);
    }
    Ok(())
}
