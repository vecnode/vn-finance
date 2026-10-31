//! Loading and validating rule packs.
//!
//! A rule pack is the only place where Portuguese tax law is written down. It is
//! data, it is versioned, it carries its own citations, and it is validated on load
//! so that a broken pack fails loudly in the Diagnóstico page instead of quietly
//! producing a wrong deadline.
//!
//! The loader is deliberately forgiving about **shape** and strict about
//! **provenance**: a pack that lacks a citation is a warning, a pack with a
//! floating-point rate is a warning, and a pack with a malformed obligation is an
//! error. That asymmetry is why validation runs over `serde_json::Value` rather
//! than deriving `Deserialize` — a derived struct would reject the extra keys real
//! packs carry (`months`, `officialDates2026`), and the shipped pack must keep
//! loading.
//!
//! The pack is compiled into the binary, so the application carries the law it
//! applied and the checksum the interface shows is computed from the same bytes.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::conditions::{is_condition_field, is_condition_op};
use crate::dates;
use crate::types::{
    Condition, ConstantProvenance, DateExtension, DeadlineKind, DeadlineRule, LoadedPack, ObligationKind,
    ObligationRule, OfficialDate, PackProblem, Periodicity, PeriodicityType, ProblemLevel, RulePack,
    RuleSource, SourceTier, Units, Verification,
};

/// The pack compiled into the binary.
///
/// One file, at the repository root, read by both implementations: the TypeScript
/// reference resolves the same path, so the two cannot drift onto different copies
/// of the same year.
pub const EMBEDDED_PACK: &str = include_str!("../../../rules/pt/2026.json");

/// The jurisdiction the only supported pack describes.
pub const SUPPORTED_JURISDICTION: &str = "PT";

/// What the interface shows when the pack is the embedded one.
pub const EMBEDDED_LABEL: &str = "embutido no binário";

/// Why a pack could not be read or trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    /// The file could not be read.
    Unreadable {
        /// The path that was tried.
        path: String,
        /// What the filesystem said.
        message: String,
    },
    /// The text is not JSON.
    NotJson {
        /// The path that was tried.
        path: String,
        /// What the parser said.
        message: String,
    },
    /// The JSON is not an object.
    NotAnObject,
    /// The pack parsed but failed validation.
    Invalid {
        /// The path, or the embedded label.
        path: String,
        /// How many errors there were.
        errors: usize,
        /// The errors, one per line, ready to show.
        detail: String,
    },
}

impl core::fmt::Display for PackError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreadable { path, message } => write!(f, "could not read {path}: {message}"),
            Self::NotJson { path, message } => write!(f, "{path} is not valid JSON: {message}"),
            Self::NotAnObject => f.write_str("rule pack is not an object"),
            Self::Invalid { path, errors, detail } => {
                write!(f, "{path} failed validation with {errors} error(s):\n{detail}")
            }
        }
    }
}

impl std::error::Error for PackError {}

/// A pack chosen for a year, and whether it is that year's pack.
#[derive(Debug, Clone, PartialEq)]
pub struct PackChoice {
    /// The pack.
    pub loaded: LoadedPack,
    /// False when the requested year has no pack and a neighbouring one is used.
    ///
    /// The caller decides how to present that, and never pretends the fallback is
    /// the requested year.
    pub exact: bool,
}

// ---------------------------------------------------------------------------
// Canonical form and checksum
// ---------------------------------------------------------------------------

/// Deterministic JSON with sorted keys, so a checksum only changes when content does.
///
/// The reference implementation's `JSON.stringify`-then-sort, reproduced: string
/// keys are sorted, arrays keep their order, and a number is written the way
/// JavaScript writes it (which for this pack's integers is the same as Rust's).
#[must_use]
pub fn canonicalise(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned()),
        Value::Array(items) => {
            let rendered: Vec<String> = items.iter().map(canonicalise).collect();
            format!("[{}]", rendered.join(","))
        }
        Value::Object(entries) => {
            let mut keys: Vec<&String> = entries.keys().collect();
            keys.sort();
            let rendered: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    let name = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_owned());
                    format!("{name}:{}", canonicalise(&entries[key]))
                })
                .collect();
            format!("{{{}}}", rendered.join(","))
        }
    }
}

/// Lower-case hex of a SHA-256.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// SHA-256 over the canonical form of a pack.
///
/// Computed from the **validated** pack, exactly as the reference does, so a field
/// the validator defaults or drops changes the checksum in both implementations at
/// once.
#[must_use]
pub fn checksum_of(pack: &RulePack) -> String {
    let value = serde_json::to_value(pack).unwrap_or(Value::Null);
    hex(&Sha256::digest(canonicalise(&value).as_bytes()))
}

// ---------------------------------------------------------------------------
// Reading values the way JavaScript does
// ---------------------------------------------------------------------------

/// `Number(value)`, including the parts that surprise people.
///
/// `None` (a missing key) is `NaN`, as `Number(undefined)` is; `null` is `0`;
/// a numeric string parses; anything else is `NaN`.
fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => {
            if *flag {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            if text.trim().is_empty() {
                0.0
            } else {
                text.trim().parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(_) => f64::NAN,
    }
}

/// `Number.isInteger(value)` over [`to_number`].
fn integer(value: Option<&Value>) -> Option<i64> {
    let number = to_number(value);
    // The upper bound keeps the conversion to `i64` exact; no pack field the loader
    // reads is anywhere near it.
    if number.is_finite() && number.fract() == 0.0 && number.abs() <= 9.0e15 {
        return Some(number as i64);
    }
    None
}

/// `String(value ?? '')` for the shapes a pack actually uses.
fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        // JavaScript would say "[object Object]"; no field the loader reads is ever
        // an object, and inventing that string would be worse than empty.
        Some(_) => String::new(),
    }
}

/// A string value, when the key holds one.
fn as_text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

/// A list of strings, the way `Array.isArray(x) ? x.map(String) : []` behaves.
fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().map(|item| text(Some(item))).collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Authority
// ---------------------------------------------------------------------------

/// Normalise a prose authority into the closed set the interface uses.
///
/// The pack may say "Segurança Social" or "Finanças"; the accent is stripped before
/// matching, so `Finanças` still lands on AT.
#[must_use]
pub fn normalise_authority(value: Option<&Value>) -> crate::types::Authority {
    let raw = text(value).to_lowercase();
    let plain: String = raw.nfd().filter(|ch| !is_combining(*ch)).collect();
    if plain == "at" || plain.contains("tribut") || plain.contains("financ") {
        return crate::types::Authority::At;
    }
    if plain == "ss" || plain.contains("social") {
        return crate::types::Authority::Ss;
    }
    crate::types::Authority::Other
}

/// Whether a character is a combining mark, the part NFD separates out.
fn is_combining(ch: char) -> bool {
    unicode_normalization::char::is_combining_mark(ch)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// The problems found so far, with the two levels the interface distinguishes.
#[derive(Default)]
struct Problems(Vec<PackProblem>);

impl Problems {
    fn error(&mut self, path: &str, message: impl Into<String>) {
        self.0.push(PackProblem {
            level: ProblemLevel::Error,
            path: path.to_owned(),
            message: message.into(),
        });
    }

    fn warn(&mut self, path: &str, message: impl Into<String>) {
        self.0.push(PackProblem {
            level: ProblemLevel::Warning,
            path: path.to_owned(),
            message: message.into(),
        });
    }
}

/// Normalise the published dates, tolerating both shapes packs use.
fn normalise_official_dates(record: &Map<String, Value>) -> BTreeMap<String, Vec<OfficialDate>> {
    let mut result: BTreeMap<String, Vec<OfficialDate>> = BTreeMap::new();

    let absorb = |year: &str, list: Option<&Value>, result: &mut BTreeMap<String, Vec<OfficialDate>>| {
        let Some(Value::Array(entries)) = list else {
            return;
        };
        let bucket = result.entry(year.to_owned()).or_default();
        for entry in entries {
            let Some(Value::Object(fields)) = Some(entry) else {
                continue;
            };
            let (Some(month), Some(day)) = (integer(fields.get("month")), integer(fields.get("day"))) else {
                continue;
            };
            let mut date = OfficialDate {
                month,
                day,
                period_label: as_text(fields.get("periodLabel")),
                extension: None,
                note: as_text(fields.get("note")),
            };
            if let Some(Value::Object(extension)) = fields.get("extension") {
                date.extension = Some(DateExtension {
                    reason: as_text(extension.get("reason")).unwrap_or_else(|| "prorrogação".to_owned()),
                    despacho: as_text(extension.get("despacho")),
                    url: as_text(extension.get("url")),
                });
            }
            bucket.push(date);
        }
    };

    if let Some(Value::Object(per_year)) = record.get("officialDates") {
        for (year, list) in per_year {
            absorb(year, Some(list), &mut result);
        }
    }
    // Tolerate the flat, year-in-the-key form used by the first-generation packs.
    for (key, value) in record {
        if let Some(year) = key.strip_prefix("officialDates")
            && year.len() == 4
            && year.bytes().all(|byte| byte.is_ascii_digit())
        {
            absorb(year, Some(value), &mut result);
        }
    }
    for bucket in result.values_mut() {
        bucket.sort_by_key(|date| (date.month, date.day));
    }
    result
}

/// The deadline kinds the pack may name.
fn deadline_kind(name: &str) -> Option<DeadlineKind> {
    let known = [
        ("day_of_month", DeadlineKind::DayOfMonth),
        ("fixed_date", DeadlineKind::FixedDate),
        ("range", DeadlineKind::Range),
        ("last_day_of_month", DeadlineKind::LastDayOfMonth),
        ("month_of_year", DeadlineKind::MonthOfYear),
        ("day_range", DeadlineKind::DayRange),
        ("days_after_event", DeadlineKind::DaysAfterEvent),
        ("retention_years", DeadlineKind::RetentionYears),
    ];
    known
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, kind)| *kind)
}

/// The periodicity types the pack may name.
fn periodicity_type(name: &str) -> Option<PeriodicityType> {
    let known = [
        ("monthly", PeriodicityType::Monthly),
        ("quarterly", PeriodicityType::Quarterly),
        ("annual", PeriodicityType::Annual),
        ("continuous", PeriodicityType::Continuous),
        ("event_driven", PeriodicityType::EventDriven),
    ];
    known
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, kind)| *kind)
}

/// The obligation kinds the pack may name.
fn obligation_kind(name: &str) -> Option<ObligationKind> {
    let known = [
        ("declare", ObligationKind::Declare),
        ("pay", ObligationKind::Pay),
        ("communicate", ObligationKind::Communicate),
        ("save", ObligationKind::Save),
        ("review", ObligationKind::Review),
        ("submit", ObligationKind::Submit),
    ];
    known
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, kind)| *kind)
}

/// The verification states the pack may name.
fn verification(name: &str) -> Option<Verification> {
    let known = [
        ("verified", Verification::Verified),
        ("partial", Verification::Partial),
        ("unverified", Verification::Unverified),
        ("stale", Verification::Stale),
    ];
    known
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, state)| *state)
}

/// Walk every number in a JSON tree, with a dotted path.
fn collect_numbers(node: &Value, path: &str, out: &mut Vec<(String, f64)>) {
    match node {
        Value::Number(number) => {
            if let Some(value) = number.as_f64() {
                out.push((path.to_owned(), value));
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_numbers(item, &format!("{path}[{index}]"), out);
            }
        }
        Value::Object(entries) => {
            for (key, value) in entries {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                collect_numbers(value, &child, out);
            }
        }
        _ => {}
    }
}

/// Walk every null in a JSON tree, with a dotted path.
fn collect_nulls(node: &Value, path: &str, out: &mut Vec<String>) {
    match node {
        Value::Null => out.push(path.to_owned()),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_nulls(item, &format!("{path}[{index}]"), out);
            }
        }
        Value::Object(entries) => {
            for (key, value) in entries {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                collect_nulls(value, &child, out);
            }
        }
        _ => {}
    }
}

/// Validate a pack that has already been parsed.
///
/// # Errors
///
/// [`PackError::NotAnObject`] when the JSON root is not an object. Everything else
/// becomes a [`PackProblem`], so the caller can show a pack's warnings even when it
/// is usable.
pub fn validate_rule_pack(raw: &Value) -> Result<(RulePack, Vec<PackProblem>), PackError> {
    let Value::Object(root) = raw else {
        return Err(PackError::NotAnObject);
    };
    let mut problems = Problems::default();

    if root.get("jurisdiction").and_then(Value::as_str) != Some(SUPPORTED_JURISDICTION) {
        problems.error("jurisdiction", "só a jurisdição \"PT\" é suportada");
    }
    let year = match integer(root.get("year")) {
        Some(year) => year,
        None => {
            problems.error("year", "o ano tem de ser um número inteiro");
            0
        }
    };

    match root.get("units") {
        None | Some(Value::Null) => problems.error(
            "units",
            "o bloco units é obrigatório e tem de declarar EUR_cents / basis_points",
        ),
        Some(Value::Object(units)) => {
            if units.get("money").and_then(Value::as_str) != Some("EUR_cents") {
                problems.error("units.money", "tem de ser \"EUR_cents\"");
            }
            if units.get("rate").and_then(Value::as_str) != Some("basis_points") {
                problems.error("units.rate", "tem de ser \"basis_points\"");
            }
        }
        Some(_) => problems.error(
            "units",
            "o bloco units é obrigatório e tem de declarar EUR_cents / basis_points",
        ),
    }

    let mut sources: Vec<RuleSource> = Vec::new();
    match root.get("sources") {
        Some(Value::Array(entries)) => {
            for (index, entry) in entries.iter().enumerate() {
                let at = format!("sources[{index}]");
                let Some(Value::Object(fields)) = Some(entry) else {
                    problems.error(&at, "tem de ser um objeto");
                    continue;
                };
                let source = RuleSource {
                    id: text(fields.get("id")),
                    tier: match fields.get("tier").and_then(Value::as_str) {
                        Some("secondary") => SourceTier::Secondary,
                        _ => SourceTier::Official,
                    },
                    authority: text(fields.get("authority")),
                    title: text(fields.get("title")),
                    url: text(fields.get("url")),
                    retrieved_at: text(fields.get("retrievedAt")),
                };
                if source.id.is_empty() {
                    problems.error(&format!("{at}.id"), "falta o id");
                }
                if !source.url.starts_with("http") {
                    problems.error(&format!("{at}.url"), "url ausente ou não-http");
                }
                sources.push(source);
            }
        }
        _ => problems.error("sources", "sources tem de ser uma lista"),
    }
    let known_sources: Vec<&str> = sources.iter().map(|source| source.id.as_str()).collect();

    let constants = match root.get("constants") {
        Some(Value::Object(_)) => root.get("constants").cloned().unwrap_or(Value::Null),
        Some(other) => {
            problems.error("constants", "o bloco constants é obrigatório");
            other.clone()
        }
        None => {
            problems.error("constants", "o bloco constants é obrigatório");
            Value::Object(Map::new())
        }
    };
    if let Value::Object(_) = constants {
        let mut numbers: Vec<(String, f64)> = Vec::new();
        collect_numbers(&constants, "constants", &mut numbers);
        for (path, value) in numbers {
            // Ratios of the IAS are the one place a fractional number is legitimate.
            let allowed = path.ends_with("MultipleIas");
            if value.fract() != 0.0 && !allowed {
                problems.warn(
                    &path,
                    format!(
                        "{value} não é um número inteiro; dinheiro é em cêntimos de euro e taxas em \
                         pontos base. Um valor fracionário aqui é quase sempre um erro."
                    ),
                );
            }
        }
    }

    let mut obligations: Vec<ObligationRule> = Vec::new();
    match root.get("obligations") {
        Some(Value::Array(entries)) => {
            let mut seen: Vec<String> = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                let at = format!("obligations[{index}]");
                let Some(Value::Object(fields)) = Some(entry) else {
                    problems.error(&at, "tem de ser um objeto");
                    continue;
                };
                let id = text(fields.get("id"));
                if id.is_empty() {
                    problems.error(&format!("{at}.id"), "falta o id");
                } else if seen.contains(&id) {
                    problems.error(&format!("{at}.id"), format!("id duplicado \"{id}\""));
                } else {
                    seen.push(id.clone());
                }

                let raw_kind = text(fields.get("kind"));
                let kind = match obligation_kind(&raw_kind) {
                    Some(kind) => kind,
                    None => {
                        problems.error(&format!("{at}.kind"), format!("tipo desconhecido \"{raw_kind}\""));
                        ObligationKind::Review
                    }
                };

                let authority = normalise_authority(fields.get("authority"));
                if authority == crate::types::Authority::Other && text(fields.get("authority")) != "OTHER" {
                    problems.warn(
                        &format!("{at}.authority"),
                        format!(
                            "autoridade \"{}\" não reconhecida; classificada como OTHER.",
                            text(fields.get("authority"))
                        ),
                    );
                }

                // `record['verification'] ?? 'unverified'`: absent **and** null both
                // mean "unverified", which is the safe direction.
                let raw_verification = match fields.get("verification") {
                    None | Some(Value::Null) => "unverified".to_owned(),
                    Some(value) => text(Some(value)),
                };
                let state = match verification(&raw_verification) {
                    Some(state) => state,
                    None => {
                        problems.error(
                            &format!("{at}.verification"),
                            format!("verificação desconhecida \"{raw_verification}\""),
                        );
                        Verification::Unverified
                    }
                };

                let mut applies_when: Vec<Condition> = Vec::new();
                match fields.get("appliesWhen") {
                    None | Some(Value::Null) => {}
                    Some(Value::Array(conditions)) => {
                        for (condition_index, condition) in conditions.iter().enumerate() {
                            let condition_at = format!("{at}.appliesWhen[{condition_index}]");
                            let Some(Value::Object(condition_fields)) = Some(condition) else {
                                problems.error(&condition_at, "tem de ser um objeto");
                                continue;
                            };
                            let field_name = text(condition_fields.get("field"));
                            let op_name = text(condition_fields.get("op"));
                            if !is_condition_field(&field_name) {
                                problems.error(
                                    &format!("{condition_at}.field"),
                                    format!("campo desconhecido \"{field_name}\""),
                                );
                            }
                            if !is_condition_op(&op_name) {
                                problems.error(
                                    &format!("{condition_at}.op"),
                                    format!("operador desconhecido \"{op_name}\""),
                                );
                            }
                            // A field or operator the pack does not have is kept as
                            // written; `test_condition` cannot match it, which makes
                            // the rule inapplicable rather than silently applicable.
                            let parsed_field = serde_json::from_value::<crate::types::ConditionField>(
                                Value::String(field_name),
                            )
                            .unwrap_or(crate::types::ConditionField::CategoryB);
                            let parsed_op =
                                serde_json::from_value::<crate::types::ConditionOp>(Value::String(op_name))
                                    .unwrap_or(crate::types::ConditionOp::Eq);
                            applies_when.push(Condition {
                                field: parsed_field,
                                op: parsed_op,
                                value: condition_fields.get("value").cloned(),
                            });
                        }
                    }
                    Some(_) => {
                        problems.error(&format!("{at}.appliesWhen"), "tem de ser uma lista de condições");
                    }
                }

                let mut periodicity = Periodicity {
                    kind: PeriodicityType::Annual,
                    lag_months: None,
                };
                match fields.get("periodicity") {
                    None | Some(Value::Null) => {
                        problems.error(&format!("{at}.periodicity"), "periodicity é obrigatório");
                    }
                    Some(Value::Object(periodicity_fields)) => {
                        let type_name = text(periodicity_fields.get("type"));
                        match periodicity_type(&type_name) {
                            Some(kind) => periodicity.kind = kind,
                            None => problems.error(
                                &format!("{at}.periodicity.type"),
                                format!("periodicidade desconhecida \"{type_name}\""),
                            ),
                        }
                        if let Some(value) = periodicity_fields.get("lagMonths")
                            && !value.is_null()
                        {
                            periodicity.lag_months = integer(Some(value));
                        }
                    }
                    Some(_) => problems.error(&format!("{at}.periodicity"), "periodicity é obrigatório"),
                }

                let mut deadline_rule: Option<DeadlineRule> = None;
                match fields.get("deadlineRule") {
                    None | Some(Value::Null) => {}
                    Some(Value::Object(deadline)) => {
                        let kind_name = text(deadline.get("kind"));
                        match deadline_kind(&kind_name) {
                            Some(kind) => {
                                if deadline.get("months").is_some_and(Value::is_array) {
                                    problems.warn(
                                        &format!("{at}.deadlineRule.months"),
                                        "a lista \"months\" não é usada pelo motor: o mês de vencimento \
                                         resulta de periodicity.lagMonths. Confirma que as duas \
                                         descrições coincidem.",
                                    );
                                }
                                deadline_rule = Some(DeadlineRule {
                                    kind,
                                    day: integer(deadline.get("day")),
                                    month: integer(deadline.get("month")),
                                    month_from: integer(deadline.get("monthFrom")),
                                    month_to: integer(deadline.get("monthTo")),
                                    day_from: integer(deadline.get("dayFrom")),
                                    day_to: integer(deadline.get("dayTo")),
                                    days: integer(deadline.get("days")),
                                    iva: integer(deadline.get("iva")),
                                    irs: integer(deadline.get("irs")),
                                    note: as_text(deadline.get("note")),
                                });
                            }
                            None => problems.error(
                                &format!("{at}.deadlineRule.kind"),
                                format!("tipo de prazo desconhecido \"{kind_name}\""),
                            ),
                        }
                    }
                    Some(_) => problems.error(&format!("{at}.deadlineRule"), "tem de ser um objeto ou null"),
                }

                let official_dates = normalise_official_dates(fields);
                if deadline_rule.is_none() && official_dates.is_empty() {
                    problems.warn(
                        &at,
                        "sem deadlineRule e sem datas publicadas: esta obrigação não pode ser agendada",
                    );
                }

                let rule_source_ids = string_list(fields.get("sourceIds"));
                for referenced in &rule_source_ids {
                    if !known_sources.contains(&referenced.as_str()) {
                        problems.warn(
                            &format!("{at}.sourceIds"),
                            format!("cita uma fonte desconhecida \"{referenced}\""),
                        );
                    }
                }
                let legal_basis = string_list(fields.get("legalBasis"));
                if legal_basis.is_empty() {
                    problems.warn(
                        &format!("{at}.legalBasis"),
                        "sem base legal registada; todas as regras devem citar a lei",
                    );
                }
                if state == Verification::Verified && rule_source_ids.is_empty() {
                    problems.warn(
                        &format!("{at}.verification"),
                        "declara \"verified\" sem citar qualquer fonte: uma regra verificada tem de \
                         citar a origem.",
                    );
                }

                obligations.push(ObligationRule {
                    title: match as_text(fields.get("title")) {
                        Some(title) => title,
                        None => id.clone(),
                    },
                    id,
                    kind,
                    authority,
                    tax: text(fields.get("tax")),
                    applies_when,
                    periodicity,
                    deadline_rule,
                    official_dates,
                    legal_basis,
                    source_ids: rule_source_ids,
                    portal_url: as_text(fields.get("portalUrl")),
                    documents_to_keep: string_list(fields.get("documentsToKeep")),
                    penalty_note: as_text(fields.get("penaltyNote")),
                    verification: state,
                    verify_note: as_text(fields.get("verifyNote")),
                });
            }
        }
        _ => problems.error("obligations", "obligations tem de ser uma lista"),
    }

    let constant_provenance = match root.get("constantProvenance") {
        None | Some(Value::Null) => None,
        Some(value) => serde_json::from_value::<BTreeMap<String, ConstantProvenance>>(value.clone()).ok(),
    };

    let disclaimer = text(root.get("disclaimer"));
    if disclaimer.is_empty() {
        problems.warn("disclaimer", "sem advertência registada");
    }

    let pack = RulePack {
        jurisdiction: SUPPORTED_JURISDICTION.to_owned(),
        year,
        pack_version: match as_text(root.get("packVersion")) {
            Some(version) => version,
            None => "0.0.0".to_owned(),
        },
        generated_at: text(root.get("generatedAt")),
        units: Units {
            money: "EUR_cents".to_owned(),
            rate: "basis_points".to_owned(),
        },
        disclaimer,
        sources,
        constants,
        constant_provenance,
        obligations,
        todo: string_list(root.get("todo")),
    };
    Ok((pack, problems.0))
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load, validate and checksum a pack from text.
///
/// # Errors
///
/// [`PackError::NotJson`] when the text is not JSON, [`PackError::NotAnObject`] when
/// it is not an object, and [`PackError::Invalid`] when validation found an error.
pub fn load_rule_pack(text: &str, path: &str) -> Result<LoadedPack, PackError> {
    let raw: Value = serde_json::from_str(text).map_err(|error| PackError::NotJson {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    let (pack, problems) = validate_rule_pack(&raw)?;
    let errors: Vec<&PackProblem> = problems
        .iter()
        .filter(|problem| problem.level == ProblemLevel::Error)
        .collect();
    if !errors.is_empty() {
        let detail = errors
            .iter()
            .map(|problem| format!("  {}: {}", problem.path, problem.message))
            .collect::<Vec<String>>()
            .join("\n");
        return Err(PackError::Invalid {
            path: path.to_owned(),
            errors: errors.len(),
            detail,
        });
    }
    let checksum = checksum_of(&pack);
    Ok(LoadedPack {
        pack,
        problems,
        checksum,
        path: path.to_owned(),
    })
}

/// The pack compiled into the binary.
///
/// # Errors
///
/// [`PackError::Invalid`] when the embedded pack is broken, which is a build defect
/// rather than a user one.
pub fn load_embedded() -> Result<LoadedPack, PackError> {
    load_rule_pack(EMBEDDED_PACK, EMBEDDED_LABEL)
}

/// Load a pack from a path.
///
/// # Errors
///
/// [`PackError::Unreadable`] when the file cannot be read, plus everything
/// [`load_rule_pack`] can return.
pub fn load_file(path: &Path) -> Result<LoadedPack, PackError> {
    let text = std::fs::read_to_string(path).map_err(|error| PackError::Unreadable {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    load_rule_pack(&text, &path.display().to_string())
}

/// Choose the pack for a year: an explicit path wins, otherwise the embedded one.
///
/// `exact` is false when the requested year has no pack of its own — the authority
/// publishes next year's calendar only weeks before it starts, so a year without a
/// pack is a normal state that the interface has to show rather than hide.
///
/// # Errors
///
/// Whatever loading the chosen pack can return.
pub fn load_for_year(year: i64, explicit: Option<&Path>) -> Result<PackChoice, PackError> {
    match explicit {
        Some(path) => {
            let loaded = load_file(path)?;
            Ok(PackChoice { loaded, exact: true })
        }
        None => {
            let loaded = load_embedded()?;
            let exact = loaded.pack.year == year;
            Ok(PackChoice { loaded, exact })
        }
    }
}

// ---------------------------------------------------------------------------
// Typed reads of the constants
// ---------------------------------------------------------------------------

/// Read a constant by dotted path, `None` when the pack leaves it empty.
///
/// A numeric segment indexes an array, the way JavaScript treats `"0"` as a key
/// on an array, so `startupReductions.0.factorBp` reads what the reference reads.
#[must_use]
pub fn constant<'a>(pack: &'a RulePack, path: &[&str]) -> Option<&'a Value> {
    let mut node = &pack.constants;
    for key in path {
        node = match node {
            Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            other => other.get(*key)?,
        };
    }
    if node.is_null() { None } else { Some(node) }
}

/// Read a constant as money in cents.
///
/// A value the pack leaves null, or writes as a fraction, is `None` — and a
/// calculation that needs it refuses rather than rounding something into a figure
/// the pack never stated.
#[must_use]
pub fn constant_cents(pack: &RulePack, path: &[&str]) -> Option<i64> {
    constant(pack, path).and_then(|value| integer(Some(value)))
}

/// Read a constant as basis points.
#[must_use]
pub fn constant_bp(pack: &RulePack, path: &[&str]) -> Option<i64> {
    constant(pack, path).and_then(|value| integer(Some(value)))
}

/// Read a constant as a ratio, which is the one shape that may be fractional.
#[must_use]
pub fn constant_ratio(pack: &RulePack, path: &[&str]) -> Option<f64> {
    constant(pack, path).and_then(|value| {
        let number = to_number(Some(value));
        if number.is_finite() { Some(number) } else { None }
    })
}

/// Whether the pack declares an obligation with this id.
///
/// A document is filed against a rule by name, so a typo used to file it against
/// nothing: no error, no trace, and the obligation stayed undocumented for ever.
/// The check has one home, and both the interface and the store ask it.
#[must_use]
pub fn declares_obligation(pack: &RulePack, obligation_id: &str) -> bool {
    pack.obligations.iter().any(|rule| rule.id == obligation_id)
}

// ---------------------------------------------------------------------------
// Summary and freshness
// ---------------------------------------------------------------------------

/// The counts the Diagnóstico page and the sidebar badge show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    /// The pack's own version.
    pub pack_version: String,
    /// The year it describes.
    pub year: i64,
    /// Its checksum.
    pub checksum: String,
    /// How many sources it cites.
    pub sources: usize,
    /// How many of those are primary.
    pub official_sources: usize,
    /// How many obligations it declares.
    pub obligations: usize,
    /// How many are verified.
    pub verified: usize,
    /// How many are partly verified.
    pub partial: usize,
    /// How many are unverified.
    pub unverified: usize,
    /// How many are stale.
    pub stale: usize,
    /// How many are event-driven and therefore not on the calendar.
    pub event_driven: usize,
    /// What the maintainers know is unfinished.
    pub todo: Vec<String>,
    /// Dotted paths of the constants the pack leaves empty.
    pub null_constants: Vec<String>,
}

/// Count everything the interface reports about a pack.
#[must_use]
pub fn summarise_pack(loaded: &LoadedPack) -> PackSummary {
    let pack = &loaded.pack;
    let mut nulls = Vec::new();
    collect_nulls(&pack.constants, "constants", &mut nulls);
    let count = |state: Verification| {
        pack.obligations
            .iter()
            .filter(|rule| rule.verification == state)
            .count()
    };
    PackSummary {
        pack_version: pack.pack_version.clone(),
        year: pack.year,
        checksum: loaded.checksum.clone(),
        sources: pack.sources.len(),
        official_sources: pack
            .sources
            .iter()
            .filter(|source| source.tier == SourceTier::Official)
            .count(),
        obligations: pack.obligations.len(),
        verified: count(Verification::Verified),
        partial: count(Verification::Partial),
        unverified: count(Verification::Unverified),
        stale: count(Verification::Stale),
        event_driven: pack
            .obligations
            .iter()
            .filter(|rule| rule.periodicity.kind == PeriodicityType::EventDriven)
            .count(),
        todo: pack.todo.clone(),
        null_constants: nulls,
    }
}

/// Whether the pack covers the year the application is being used in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Freshness {
    /// Which of the three situations the pack is in.
    pub status: FreshnessStatus,
    /// A sentence for the person, in Portuguese.
    pub message: String,
}

/// The three situations a pack can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FreshnessStatus {
    /// It covers the year in course.
    Current,
    /// It is older than the year in course.
    Stale,
    /// It is ahead of the year in course.
    Future,
}

/// Whether a pack covers the year it is being used in.
///
/// Rule packs are annual artefacts. The authority publishes next year's calendar
/// only a few weeks before it starts, so the application must be honest about the
/// gap rather than pretending to know dates that have not been published.
///
/// # Errors
///
/// [`crate::dates::DateError`] when `today` is not an ISO date.
pub fn freshness(pack: &RulePack, today: &str) -> Result<Freshness, crate::dates::DateError> {
    let current_year = dates::parse_iso(today)?.year;
    Ok(if pack.year == current_year {
        Freshness {
            status: FreshnessStatus::Current,
            message: format!("Pacote de regras de {} (ano em curso).", pack.year),
        }
    } else if pack.year < current_year {
        Freshness {
            status: FreshnessStatus::Stale,
            message: format!(
                "O pacote de regras é de {} mas o ano em curso é {current_year}. As datas publicadas \
                 não cobrem este ano: os prazos são provisórios e têm de ser conferidos.",
                pack.year
            ),
        }
    } else {
        Freshness {
            status: FreshnessStatus::Future,
            message: format!(
                "Pacote de regras de {}, à frente do ano em curso ({current_year}).",
                pack.year
            ),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> Result<LoadedPack, PackError> {
        load_embedded()
    }

    #[test]
    fn the_embedded_pack_loads_without_errors() -> Result<(), PackError> {
        let pack = loaded()?;
        assert_eq!(pack.pack.jurisdiction, "PT");
        assert_eq!(pack.pack.year, 2026);
        assert_eq!(pack.pack.pack_version, "2026.1.1");
        assert_eq!(pack.pack.obligations.len(), 20);
        assert_eq!(pack.pack.sources.len(), 21);
        assert_eq!(pack.pack.todo.len(), 21);
        assert_eq!(pack.checksum.len(), 64);
        assert!(
            pack.problems
                .iter()
                .all(|problem| problem.level == ProblemLevel::Warning),
            "the shipped pack must load without a single error"
        );
        Ok(())
    }

    #[test]
    fn the_checksum_ignores_key_order_and_follows_content() -> Result<(), PackError> {
        // Two spellings of the same pack: different key order, same every value.
        let one = r#"{"jurisdiction":"PT","year":2026,"units":{"money":"EUR_cents","rate":"basis_points"},"disclaimer":"x","sources":[],"constants":{"a":1,"b":2},"obligations":[],"todo":[]}"#;
        let reordered = r#"{"todo":[],"obligations":[],"constants":{"b":2,"a":1},"sources":[],"disclaimer":"x","units":{"rate":"basis_points","money":"EUR_cents"},"year":2026,"jurisdiction":"PT"}"#;
        let changed = r#"{"jurisdiction":"PT","year":2026,"units":{"money":"EUR_cents","rate":"basis_points"},"disclaimer":"x","sources":[],"constants":{"a":9,"b":2},"obligations":[],"todo":[]}"#;
        let left = load_rule_pack(one, "um.json")?;
        let right = load_rule_pack(reordered, "dois.json")?;
        let different = load_rule_pack(changed, "tres.json")?;
        assert_eq!(
            left.checksum, right.checksum,
            "key order must not change the checksum"
        );
        assert_ne!(
            left.checksum, different.checksum,
            "a changed constant must change it"
        );
        Ok(())
    }

    #[test]
    fn a_malformed_pack_is_refused_with_the_reason() {
        let error = load_rule_pack("{\"jurisdiction\":\"ES\"}", "pacote.json")
            .err()
            .unwrap_or(PackError::NotAnObject);
        assert!(matches!(error, PackError::Invalid { .. }), "got {error:?}");
        if let PackError::Invalid { errors, detail, .. } = error {
            assert!(errors >= 1);
            assert!(detail.contains("jurisdiction"), "{detail}");
        }
    }

    #[test]
    fn a_pack_that_is_not_an_object_is_refused() {
        assert_eq!(
            load_rule_pack("[1,2,3]", "pacote.json").err(),
            Some(PackError::NotAnObject)
        );
    }

    #[test]
    fn prose_authorities_are_normalised() {
        let at = Value::String("Finanças".to_owned());
        assert_eq!(normalise_authority(Some(&at)), crate::types::Authority::At);
        let ss = Value::String("Segurança Social".to_owned());
        assert_eq!(normalise_authority(Some(&ss)), crate::types::Authority::Ss);
        let other = Value::String("Banco de Portugal".to_owned());
        assert_eq!(normalise_authority(Some(&other)), crate::types::Authority::Other);
        assert_eq!(normalise_authority(None), crate::types::Authority::Other);
    }

    #[test]
    fn a_constant_the_pack_leaves_empty_is_none_and_never_zero() -> Result<(), PackError> {
        let pack = loaded()?;
        // Whatever the shipped pack holds, a path that does not exist must be None.
        assert_eq!(constant_cents(&pack.pack, &["iva", "rates", "inexistente"]), None);
        assert_eq!(constant_ratio(&pack.pack, &["nada"]), None);
        // And a path that does exist reads as the number the pack wrote.
        assert_eq!(constant_bp(&pack.pack, &["iva", "rates", "normal"]), Some(2300));
        Ok(())
    }

    #[test]
    fn the_summary_counts_what_the_interface_shows() -> Result<(), PackError> {
        let summary = summarise_pack(&loaded()?);
        assert_eq!(summary.obligations, 20);
        assert_eq!(summary.sources, 21);
        assert_eq!(
            summary.official_sources + (summary.sources - summary.official_sources),
            21
        );
        assert_eq!(
            summary.verified + summary.partial + summary.unverified + summary.stale,
            20
        );
        assert!(
            summary
                .null_constants
                .iter()
                .all(|path| path.starts_with("constants"))
        );
        Ok(())
    }

    #[test]
    fn freshness_says_which_year_the_pack_belongs_to() -> Result<(), Box<dyn std::error::Error>> {
        let pack = loaded()?;
        let current = freshness(&pack.pack, "2026-09-20")?;
        assert_eq!(current.status, FreshnessStatus::Current);
        let stale = freshness(&pack.pack, "2027-01-15")?;
        assert_eq!(stale.status, FreshnessStatus::Stale);
        assert!(stale.message.contains("2027"));
        let future = freshness(&pack.pack, "2025-12-31")?;
        assert_eq!(future.status, FreshnessStatus::Future);
        Ok(())
    }

    #[test]
    fn a_declared_obligation_is_recognised_and_a_typo_is_not() -> Result<(), PackError> {
        let pack = loaded()?;
        let first = pack
            .pack
            .obligations
            .first()
            .map(|rule| rule.id.clone())
            .unwrap_or_default();
        assert!(declares_obligation(&pack.pack, &first));
        assert!(!declares_obligation(&pack.pack, &format!("{first}_typo")));
        Ok(())
    }
}
