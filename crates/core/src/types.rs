//! The domain model.
//!
//! Everything the application knows about the taxpayer, about the law, and about
//! what is due. Two design commitments are visible here:
//!
//! - **The law is data.** Rates, deadlines and their citations live in a versioned
//!   rule pack, never in an `if`. When the law changes, a data file changes and
//!   every number keeps its provenance.
//! - **The engine is deterministic.** Given a profile and a rule pack it produces
//!   the same obligations every time, with no network, no model and no clock.
//!
//! The serde attributes are not cosmetic: these structs are also what the pack's
//! checksum is computed over, so a field that serialises under a different name,
//! or that omits itself when the reference implementation emits `null`, changes a
//! stored checksum. The golden fixture in `tests/golden/rules.json` is what keeps
//! that honest.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A calendar date, `YYYY-MM-DD`, always interpreted in Europe/Lisbon.
///
/// Kept as a string rather than a parsed struct on purpose: it is the shape the
/// rule pack, the vault and the JSON output all use, and ISO dates sort
/// lexicographically, so [`crate::dates::compare_iso`] is a string comparison. The
/// checks that matter live in [`crate::dates::parse_iso`], which is deliberately as
/// lenient as the reference implementation.
pub type IsoDate = String;

/// How a taxpayer's IVA regime was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IvaRegime {
    /// Exemption under art. 53.º of the CIVA.
    IsentoArt53,
    /// Normal regime, monthly periodic declaration.
    Mensal,
    /// Normal regime, quarterly periodic declaration.
    Trimestral,
}

/// How a taxpayer's IRS regime was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IrsRegime {
    /// The simplified regime.
    Simplificado,
    /// Organised accounting.
    Organizada,
}

/// What kind of thing an obligation asks of the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationKind {
    /// Submit a declaration.
    Declare,
    /// Pay something.
    Pay,
    /// Communicate data to an authority.
    Communicate,
    /// Keep a document.
    Save,
    /// Look at something.
    Review,
    /// Any other submission.
    Submit,
}

/// Which body imposes the obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Authority {
    /// Autoridade Tributária e Aduaneira.
    #[serde(rename = "AT")]
    At,
    /// Segurança Social.
    #[serde(rename = "SS")]
    Ss,
    /// Any other body.
    #[serde(rename = "OTHER")]
    Other,
}

/// How well established a rule is.
///
/// `Partial` exists because tax reality is not binary: a deadline can be confirmed
/// from the authority's published calendar while the article that establishes it
/// has not been read directly. Saying so is more useful than forcing the rule into
/// "verified" or discarding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// Confirmed against the cited source.
    Verified,
    /// Partly confirmed; `verify_note` says which part.
    Partial,
    /// Not confirmed.
    Unverified,
    /// The year moved on and the value was not re-checked.
    Stale,
}

/// How confident the engine is that an obligation applies to this taxpayer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    /// Due later than the "due soon" window.
    Future,
    /// Due within the window.
    DueSoon,
    /// Past its date and not marked done.
    Overdue,
    /// Marked done by the person.
    Done,
    /// Due before the person started keeping records here.
    ///
    /// Not "late", and never nagged about: a tool adopted in September did not miss
    /// the previous January.
    Untracked,
    /// The rule does not apply to this profile.
    NotApplicable,
}

/// The role a CAE code plays in a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaeRole {
    /// The main activity.
    Principal,
    /// A secondary activity.
    Secondary,
}

/// One CAE code with the description the person or the pack gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaeEntry {
    /// The five-digit code.
    pub code: String,
    /// What the code means, in Portuguese.
    pub description: String,
    /// Whether this is the main activity.
    pub role: CaeRole,
}

/// The activity itself, as declared in the início de atividade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityProfile {
    /// CIRS category B — independent activity. Without this there is no recibo verde.
    pub category_b: bool,
    /// When the activity began. The sentinel `1970-01-01` means "not declared".
    pub start_date: IsoDate,
    /// Declared CAE codes; never empty once a profile has been built.
    pub cae: Vec<CaeEntry>,
    /// Turnover in national territory in the previous civil year, as declared.
    ///
    /// Never defaulted: the art. 53.º CIVA exemption depends on this figure, and a
    /// tool that guesses it cannot be trusted to evaluate the exemption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turnover_previous_year_cents: Option<i64>,
    /// The person's own expectation for the current civil year.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turnover_current_year_expected_cents: Option<i64>,
    /// Intra-community supplies of services, triggering reverse charge and recapitulativa.
    pub intra_community_operations: bool,
    /// Declared, never inferred: whether those operations passed 50 000 EUR in a
    /// quarter or in any of the four previous ones.
    ///
    /// `None` and `Some(false)` are **not** the same thing: the pack tests
    /// `ne true`, so an unanswered question gets the base rule (quarterly) while a
    /// declared "no" is an answer. Nothing defaults this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intra_community_operations_above_50k: Option<bool>,
    /// Supplies of services to clients outside the European Union.
    pub exports: bool,
    /// Whether the taxpayer employs anybody.
    pub has_employees: bool,
    /// Whether invoicing goes through certified software.
    pub uses_certified_invoicing_software: bool,
    /// Whether invoices are communicated through the AT webservice.
    pub uses_at_webservice: bool,
}

/// The taxpayer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxProfile {
    /// The nine-digit tax number.
    pub nif: String,
    /// The name as declared.
    pub name: String,
    /// Tax residence in Portugal.
    ///
    /// Spelled `residentPT` on the wire, not `residentPt`: the vault and the
    /// reference write the initialism in capitals, and a profile read with the
    /// wrong key would silently lose a declaration.
    #[serde(rename = "residentPT")]
    pub resident_pt: bool,
    /// Whether this is a company rather than a natural person.
    pub is_company: bool,
    /// The date from which this application is expected to have tracked
    /// obligations. Everything due before it is history, not a missed deadline.
    ///
    /// Declared before `activity` so the file this build writes has the same key
    /// order as the reference's, which keeps a profile readable by both and makes a
    /// diff between the two writers empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_start: Option<IsoDate>,
    /// The activity.
    pub activity: ActivityProfile,
    /// The declared IVA regime.
    pub iva: IvaChoice,
    /// The declared IRS regime and coefficient.
    pub irs: IrsChoice,
    /// The Segurança Social situation.
    pub ss: SsChoice,
}

/// The IVA block of a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IvaChoice {
    /// The declared regime.
    pub regime: IvaRegime,
}

/// The IRS block of a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrsChoice {
    /// The declared regime.
    pub regime: IrsRegime,
    /// The simplified-regime coefficient, in basis points, applied to gross
    /// service income.
    pub coefficient_bp: i64,
}

/// The Segurança Social block of a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsChoice {
    /// Whether the first-activity contribution exemption is still running.
    pub startup_exemption_active: bool,
    /// When the activity began, for the exemption window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_activity_date: Option<IsoDate>,
}

// ---------------------------------------------------------------------------
// Conditions — a tiny declarative predicate language, deliberately not code
// ---------------------------------------------------------------------------

/// The complete vocabulary of fields a rule pack may ask about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionField {
    /// `iva.regime`
    #[serde(rename = "iva.regime")]
    IvaRegime,
    /// `irs.regime`
    #[serde(rename = "irs.regime")]
    IrsRegime,
    /// `profile.residentPT`
    #[serde(rename = "profile.residentPT")]
    ResidentPt,
    /// `profile.isCompany`
    #[serde(rename = "profile.isCompany")]
    IsCompany,
    /// `activity.categoryB`
    #[serde(rename = "activity.categoryB")]
    CategoryB,
    /// `activity.intraCommunityOperations`
    #[serde(rename = "activity.intraCommunityOperations")]
    IntraCommunityOperations,
    /// `activity.intraCommunityOperationsAbove50k`
    #[serde(rename = "activity.intraCommunityOperationsAbove50k")]
    IntraCommunityOperationsAbove50k,
    /// `activity.exports`
    #[serde(rename = "activity.exports")]
    Exports,
    /// `activity.hasEmployees`
    #[serde(rename = "activity.hasEmployees")]
    HasEmployees,
    /// `activity.usesCertifiedInvoicingSoftware`
    #[serde(rename = "activity.usesCertifiedInvoicingSoftware")]
    UsesCertifiedInvoicingSoftware,
    /// `activity.usesAtWebservice`
    #[serde(rename = "activity.usesAtWebservice")]
    UsesAtWebservice,
    /// `ss.startupExemptionActive`
    #[serde(rename = "ss.startupExemptionActive")]
    StartupExemptionActive,
}

/// The operators the condition language offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    /// Equal.
    Eq,
    /// Not equal — **true** when the field is absent, which is what gives an
    /// unanswered optional question the base rule.
    Ne,
    /// One of the listed values.
    In,
    /// Numerically greater.
    Gt,
    /// Numerically greater or equal.
    Gte,
    /// Numerically less.
    Lt,
    /// Numerically less or equal.
    Lte,
    /// Present and not null.
    Exists,
}

/// One predicate: a field, an operator and (except for `exists`) a value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// Which field of the profile to read.
    pub field: ConditionField,
    /// How to compare it.
    pub op: ConditionOp,
    /// What to compare it with. Absent for `exists`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

// ---------------------------------------------------------------------------
// Rule pack
// ---------------------------------------------------------------------------

/// Where a cited source sits in the hierarchy of authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceTier {
    /// Primary law or the authority's own publication.
    Official,
    /// Commentary, a professional body, a secondary compilation.
    Secondary,
}

/// One citation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSource {
    /// Stable identifier referenced by obligations.
    pub id: String,
    /// Primary or secondary.
    pub tier: SourceTier,
    /// The body that published it.
    pub authority: String,
    /// What it is called.
    pub title: String,
    /// Where it can be read.
    pub url: String,
    /// When it was retrieved.
    pub retrieved_at: String,
}

/// How often an obligation recurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodicityType {
    /// Every month.
    Monthly,
    /// Every quarter.
    Quarterly,
    /// Once a year.
    Annual,
    /// A standing duty with no date.
    Continuous,
    /// Triggered by an event rather than a calendar.
    EventDriven,
}

/// The periodicity of an obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Periodicity {
    /// How often it recurs.
    #[serde(rename = "type")]
    pub kind: PeriodicityType,
    /// How many months after the end of the period it falls due.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lag_months: Option<i64>,
}

/// The shape of a deadline rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadlineKind {
    /// A fixed day of the month.
    DayOfMonth,
    /// A fixed date of the year.
    FixedDate,
    /// A window whose end is the deadline.
    Range,
    /// The last day of the month.
    LastDayOfMonth,
    /// A month of the year, with no day fixed.
    MonthOfYear,
    /// A payment window, e.g. the 10th to the 20th.
    DayRange,
    /// A number of days after a triggering event.
    DaysAfterEvent,
    /// A retention period in years.
    RetentionYears,
}

/// How a deadline is described when the authority's published calendar does not
/// settle it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadlineRule {
    /// Which shape this deadline has.
    pub kind: DeadlineKind,
    /// Day within the month, when the rule fixes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<i64>,
    /// 1–12: the due month for annual rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<i64>,
    /// Window start month, for a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month_from: Option<i64>,
    /// Window end month, for a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month_to: Option<i64>,
    /// Window start day, for a payment window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_from: Option<i64>,
    /// Window end day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_to: Option<i64>,
    /// Days after the triggering event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub days: Option<i64>,
    /// Retention period in years for IVA documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iva: Option<i64>,
    /// Retention period in years for IRS documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irs: Option<i64>,
    /// A note carried with the deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A postponement published by the authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateExtension {
    /// Why the deadline moved.
    pub reason: String,
    /// The despacho that ordered it, when named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub despacho: Option<String>,
    /// Where it can be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A date published by the authority, which always beats a derived one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialDate {
    /// Month, 1–12.
    pub month: i64,
    /// Day within the month.
    pub day: i64,
    /// The period the date refers to, when the authority's table says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_label: Option<String>,
    /// A postponement attached to this date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<DateExtension>,
    /// A note carried with the date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One obligation the law declares.
///
/// `PartialEq` only: a condition carries arbitrary JSON, which has no `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObligationRule {
    /// Stable identifier.
    pub id: String,
    /// What kind of thing it asks for.
    pub kind: ObligationKind,
    /// Which body imposes it.
    pub authority: Authority,
    /// The tax it belongs to.
    pub tax: String,
    /// How the interface names it.
    pub title: String,
    /// Empty means always applicable.
    pub applies_when: Vec<Condition>,
    /// How often it recurs.
    pub periodicity: Periodicity,
    /// How its deadline is described. `None` means no fixed date.
    pub deadline_rule: Option<DeadlineRule>,
    /// Published dates, keyed by year.
    pub official_dates: BTreeMap<String, Vec<OfficialDate>>,
    /// The articles it rests on.
    pub legal_basis: Vec<String>,
    /// The citations it rests on.
    pub source_ids: Vec<String>,
    /// Where the person would go to comply.
    pub portal_url: Option<String>,
    /// Documents worth keeping for this obligation.
    pub documents_to_keep: Vec<String>,
    /// What happens when it is missed.
    pub penalty_note: Option<String>,
    /// How well established the rule is.
    pub verification: Verification,
    /// What exactly is not confirmed, when it is only partial.
    pub verify_note: Option<String>,
}

/// Where a single constant came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstantProvenance {
    /// Whether a human or the assistant proposed it.
    pub source: ProvenanceSource,
    /// When it was proposed.
    pub proposed_at: IsoDate,
    /// Where the proposal said it could be checked.
    pub source_url: Option<String>,
    /// Whether a human confirmed it.
    pub status: ProvenanceStatus,
    /// Anything else worth recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Who proposed a constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceSource {
    /// A person.
    Human,
    /// The rule-pack assistant.
    AiProposed,
}

/// Whether a proposed constant was confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvenanceStatus {
    /// Confirmed against the source.
    Verified,
    /// Not yet confirmed.
    Unverified,
}

/// The money and rate units the pack declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Units {
    /// Always `EUR_cents`.
    pub money: String,
    /// Always `basis_points`.
    pub rate: String,
}

/// The rule pack: the only place Portuguese tax law is written down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulePack {
    /// Always `PT`.
    pub jurisdiction: String,
    /// The year the pack describes.
    pub year: i64,
    /// The pack's own version.
    pub pack_version: String,
    /// When it was generated.
    pub generated_at: String,
    /// The units it declares.
    pub units: Units,
    /// The warning the interface shows.
    pub disclaimer: String,
    /// Its citations.
    pub sources: Vec<RuleSource>,
    /// The constants, kept exactly as the pack wrote them.
    ///
    /// Deliberately untyped: the reference loader **casts** this block rather than
    /// validating it, so a typed struct would either reject a pack that works today
    /// or change the checksum by dropping a key it did not know. Access is through
    /// the typed readers in [`crate::rules`], which return `None` for a value the
    /// pack leaves empty — and a calculation that needs a missing constant refuses.
    pub constants: Value,
    /// Where each constant came from, keyed by dotted path. Absent in the shipped
    /// pack, and absent from the checksum when the pack does not carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constant_provenance: Option<BTreeMap<String, ConstantProvenance>>,
    /// What the law obliges.
    pub obligations: Vec<ObligationRule>,
    /// Things the pack maintainers know are unfinished.
    pub todo: Vec<String>,
}

/// Whether a pack problem is fatal or advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProblemLevel {
    /// The pack cannot be used.
    Error,
    /// The pack works, but something is worth saying.
    Warning,
}

/// One thing wrong with a pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackProblem {
    /// Fatal or advisory.
    pub level: ProblemLevel,
    /// Where in the pack it is.
    pub path: String,
    /// What is wrong, in Portuguese.
    pub message: String,
}

/// A pack that has been read and validated.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPack {
    /// The validated pack.
    pub pack: RulePack,
    /// Everything the validator noticed.
    pub problems: Vec<PackProblem>,
    /// SHA-256 over the canonical form of the validated pack.
    pub checksum: String,
    /// Where it came from, for display.
    pub path: String,
}

// ---------------------------------------------------------------------------
// Materialised obligations
// ---------------------------------------------------------------------------

/// Where a due date came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DueDateSource {
    /// The authority published it.
    Official,
    /// It was derived from the general legal rule.
    Derived,
}

/// One item in a document checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
    /// What to keep.
    pub label: String,
    /// Whether it is already in the vault. Never persisted; the vault knows.
    pub done: bool,
}

/// One dated obligation, ready for the agenda.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObligationInstance {
    /// Stable identity: `<ruleId>@<dueDate>`, or `<ruleId>@na`.
    pub id: String,
    /// The rule it came from.
    pub rule_id: String,
    /// What kind of thing it asks for.
    pub kind: ObligationKind,
    /// Which body imposes it.
    pub authority: Authority,
    /// The tax it belongs to.
    pub tax: String,
    /// How the interface names it.
    pub title: String,
    /// Human label for the period it covers.
    pub period_label: String,
    /// First day of the period.
    pub period_start: Option<IsoDate>,
    /// Last day of the period.
    pub period_end: Option<IsoDate>,
    /// When it falls due.
    pub due_date: IsoDate,
    /// Whether that date came from the authority's table or was derived.
    pub due_date_source: DueDateSource,
    /// Whether the date moved off a weekend.
    pub adjusted_for_weekend: bool,
    /// True when the authority has not yet published dates for this year.
    pub provisional: bool,
    /// Where it stands.
    pub status: InstanceStatus,
    /// Why it does not apply, in Portuguese, when it does not.
    pub not_applicable_reason: Option<String>,
    /// How well established the rule is.
    pub verification: Verification,
    /// What exactly is not confirmed.
    pub verify_note: Option<String>,
    /// The articles it rests on.
    pub legal_basis: Vec<String>,
    /// The citations it rests on.
    pub source_ids: Vec<String>,
    /// Where the person would go to comply.
    pub portal_url: Option<String>,
    /// Documents worth keeping.
    pub documents_to_keep: Vec<String>,
    /// What happens when it is missed.
    pub penalty_note: Option<String>,
    /// Places where the derived logic and the published table do not agree.
    pub discrepancies: Vec<String>,
    /// The documents this obligation wants, for the Cofre checklist.
    pub checklists: Vec<ChecklistItem>,
}

/// How to build an agenda.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgendaOptions {
    /// The civil year to build.
    pub year: i64,
    /// The date the person believes it is today.
    pub today: IsoDate,
    /// Days ahead that count as "due soon". Defaults to 30.
    pub due_soon_days: Option<i64>,
    /// Ids of obligations already handled.
    pub completed_ids: Vec<String>,
    /// Obligations due before this date are history, not a miss.
    pub track_from: Option<IsoDate>,
    /// Public holidays for the year. Never guessed by the engine.
    pub holidays: Vec<IsoDate>,
}
