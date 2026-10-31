//! The taxpayer profile, and the consistency checks that go with it.
//!
//! These checks encode real constraints of the Portuguese system rather than
//! cosmetic validation. Two examples: the art. 53.º exemption demands that the
//! taxpayer carries out no export operations and has national domicile, and a
//! start-up contribution exemption is a twelve-month window. Both are things a
//! taxpayer can get wrong silently for a year.
//!
//! One deliberate difference from the reference: **`validate_profile` takes
//! `today`**. In the TypeScript implementation the twelve-month check called
//! `monthsSince(startDate)` with a default clock, so the same profile produced a
//! different problem list depending on the day the test ran. A function that is
//! supposed to be a pure check cannot read a clock.

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::nif::{is_valid_pt_tax_number, normalise_nif};
use crate::types::{
    ActivityProfile, CaeEntry, CaeRole, IrsChoice, IrsRegime, IvaChoice, IvaRegime, SsChoice, TaxProfile,
};

/// The regimes the AT lets a taxpayer declare. Anything else is not a regime.
pub const IVA_REGIMES: [IvaRegime; 3] = [IvaRegime::IsentoArt53, IvaRegime::Trimestral, IvaRegime::Mensal];
/// The IRS regimes the AT lets a taxpayer declare.
pub const IRS_REGIMES: [IrsRegime; 2] = [IrsRegime::Simplificado, IrsRegime::Organizada];

/// The CAE the interface offers when the person does not name one.
#[must_use]
pub fn default_cae() -> CaeEntry {
    CaeEntry {
        code: "62010".to_owned(),
        description: "Atividades de programação informática".to_owned(),
        role: CaeRole::Principal,
    }
}

/// The sentinel `startDate` that means "not declared".
pub const UNDECLARED_START_DATE: &str = "1970-01-01";

/// Whether a problem is fatal or advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProblemLevel {
    /// The profile cannot be trusted.
    Error,
    /// Something to look at.
    Warning,
}

/// One thing wrong with a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileProblem {
    /// Fatal or advisory.
    pub level: ProblemLevel,
    /// Which field it is about.
    pub field: String,
    /// What is wrong, in Portuguese.
    pub message: String,
}

/// Why a profile could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// A new profile needs a tax number.
    MissingNif,
    /// A new profile needs a name.
    MissingName,
    /// The imported file is not a JSON object.
    NotAnObject,
    /// The imported object has no usable `nif` and `name`.
    NifAndNameNotText,
    /// A regime value that is not one of the declared ones.
    UnknownRegime {
        /// What the value belongs to, e.g. "o regime de IVA".
        label: String,
        /// The value as it arrived.
        value: String,
        /// The values that would have been accepted.
        allowed: String,
    },
    /// A date that is not in `YYYY-MM-DD` shape.
    BadDate {
        /// What the date is, e.g. "a data de início de atividade".
        label: String,
    },
    /// A cents value that is not a non-negative integer.
    BadCents {
        /// The field name.
        field: String,
    },
    /// The CAE list is absent or empty.
    BadCaeList,
    /// A CAE entry is not an object.
    BadCaeEntry,
    /// A CAE entry has no text code or description.
    CaeMissingText,
    /// A CAE entry has an unknown role.
    CaeBadRole {
        /// The code the entry carried, for the message.
        code: String,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingNif => f.write_str("o NIF é obrigatório para criar o perfil."),
            Self::MissingName => f.write_str("o nome é obrigatório para criar o perfil."),
            Self::NotAnObject => f.write_str("o ficheiro não contém um perfil: esperava um objeto JSON."),
            Self::NifAndNameNotText => {
                f.write_str("o perfil importado tem de ter \"nif\" e \"name\" como texto.")
            }
            Self::UnknownRegime {
                label,
                value,
                allowed,
            } => write!(f, "{label} \"{value}\" não é reconhecido (esperado: {allowed})."),
            Self::BadDate { label } => write!(f, "{label} não está em formato AAAA-MM-DD."),
            Self::BadCents { field } => {
                write!(
                    f,
                    "\"{field}\" tem de ser um valor em cêntimos (inteiro, não negativo)."
                )
            }
            Self::BadCaeList => f.write_str("\"activity.cae\" tem de ser uma lista com pelo menos um CAE."),
            Self::BadCaeEntry => {
                f.write_str("cada CAE tem de ser um objeto com \"code\", \"description\" e \"role\".")
            }
            Self::CaeMissingText => f.write_str("cada CAE tem de ter \"code\" e \"description\" em texto."),
            Self::CaeBadRole { code } => {
                write!(f, "o CAE {code} tem de ter \"role\" principal ou secondary.")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

/// Everything needed to create a brand-new profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileInput {
    /// The tax number.
    pub nif: String,
    /// The name.
    pub name: String,
    /// Tax residence in Portugal.
    pub resident_pt: Option<bool>,
    /// Whether this is a company.
    pub is_company: Option<bool>,
    /// Declared CAE codes.
    pub cae: Option<Vec<CaeEntry>>,
    /// When the activity began.
    pub start_date: Option<String>,
    /// When tracking began.
    pub tracking_start: Option<String>,
    /// Previous civil year's turnover in national territory, in cents.
    pub turnover_previous_year_cents: Option<i64>,
    /// The person's estimate for the current civil year, in cents.
    pub turnover_current_year_expected_cents: Option<i64>,
    /// CIRS category B.
    pub category_b: Option<bool>,
    /// The declared IVA regime.
    pub iva_regime: Option<IvaRegime>,
    /// The declared IRS regime.
    pub irs_regime: Option<IrsRegime>,
    /// The simplified-regime coefficient, in basis points.
    pub coefficient_bp: Option<i64>,
    /// Intra-community operations.
    pub intra_community_operations: Option<bool>,
    /// The RITI art. 30.º n.º 2 declaration.
    pub intra_community_operations_above_50k: Option<bool>,
    /// Export operations.
    pub exports: Option<bool>,
    /// Whether the taxpayer employs anybody.
    pub has_employees: Option<bool>,
    /// Whether invoicing uses certified software.
    pub uses_certified_invoicing_software: Option<bool>,
    /// Whether invoices are sent through the AT webservice.
    pub uses_at_webservice: Option<bool>,
    /// The start-up contribution exemption.
    pub startup_exemption_active: Option<bool>,
}

/// A change to a profile, where every field is optional.
///
/// A draft that omits a field leaves the stored value alone. A field set to
/// `Some(None)` **clears** an optional value — which is how a wrong turnover figure
/// is removed, and is a decision the caller made rather than something this module
/// guessed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileDraft {
    /// The tax number.
    pub nif: Option<String>,
    /// The name.
    pub name: Option<String>,
    /// The declared IVA regime.
    pub iva_regime: Option<IvaRegime>,
    /// The declared IRS regime.
    pub irs_regime: Option<IrsRegime>,
    /// When the activity began.
    pub start_date: Option<String>,
    /// When tracking began.
    pub tracking_start: Option<String>,
    /// Previous civil year's turnover. Absent leaves it, `null` deletes it.
    #[serde(deserialize_with = "double_option")]
    pub turnover_previous_year_cents: Option<Option<i64>>,
    /// The current year's expected turnover. Absent leaves it, `null` deletes it.
    #[serde(deserialize_with = "double_option")]
    pub turnover_current_year_expected_cents: Option<Option<i64>>,
    /// Declared CAE codes.
    pub cae: Option<Vec<CaeEntry>>,
    /// CIRS category B.
    pub category_b: Option<bool>,
    /// Tax residence in Portugal.
    pub resident_pt: Option<bool>,
    /// Whether this is a company.
    pub is_company: Option<bool>,
    /// Intra-community operations.
    pub intra_community_operations: Option<bool>,
    /// The RITI art. 30.º n.º 2 declaration.
    pub intra_community_operations_above_50k: Option<bool>,
    /// Export operations.
    pub exports: Option<bool>,
    /// The start-up contribution exemption.
    pub startup_exemption_active: Option<bool>,
}

/// Distinguishes an absent field from one set to `null`, which serde's
/// `Option<Option<T>>` does not do on its own.
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// Build one complete profile from a draft and, optionally, the profile it replaces.
///
/// Every field the draft omits is taken from `current` when there is one, and only
/// then from the documented default. A field explicitly set to `null` clears an
/// optional value and is nevertheless a decision the caller made.
///
/// The IVA regime has a default here because a profile built by a test, a fixture or
/// an old import should not be blocked on a declaration it is not making. What the
/// **interfaces** require is a separate matter, and they require it: the profile form
/// refuses to create a profile without a declared regime, because a guess there would
/// silently change every downstream obligation.
///
/// # Errors
///
/// [`ProfileError::MissingNif`] or [`ProfileError::MissingName`] when neither the
/// draft nor the current profile supplies them.
pub fn build_profile(
    draft: &ProfileDraft,
    current: Option<&TaxProfile>,
    coefficient_bp: Option<i64>,
) -> Result<TaxProfile, ProfileError> {
    let nif = draft
        .nif
        .clone()
        .or_else(|| current.map(|profile| profile.nif.clone()))
        .ok_or(ProfileError::MissingNif)?;
    let name = draft
        .name
        .clone()
        .or_else(|| current.map(|profile| profile.name.clone()))
        .ok_or(ProfileError::MissingName)?;
    let iva_regime = draft
        .iva_regime
        .or_else(|| current.map(|profile| profile.iva.regime))
        .unwrap_or(IvaRegime::Trimestral);

    // Null deletes an optional figure; absent means the draft did not mention it.
    let carry_over = |drafted: &Option<Option<i64>>, stored: Option<i64>| match drafted {
        Some(None) => None,
        Some(Some(value)) => Some(*value),
        None => stored,
    };

    let activity = current.map(|profile| &profile.activity);
    let start_date = draft
        .start_date
        .clone()
        .or_else(|| activity.map(|activity| activity.start_date.clone()))
        .unwrap_or_else(|| UNDECLARED_START_DATE.to_owned());
    let turnover_previous = carry_over(
        &draft.turnover_previous_year_cents,
        activity.and_then(|activity| activity.turnover_previous_year_cents),
    );
    let turnover_expected = carry_over(
        &draft.turnover_current_year_expected_cents,
        activity.and_then(|activity| activity.turnover_current_year_expected_cents),
    );

    let first_activity_date = current
        .and_then(|profile| profile.ss.first_activity_date.clone())
        .unwrap_or_else(|| start_date.clone());

    let tracking_start = match (
        &draft.tracking_start,
        current.and_then(|p| p.tracking_start.clone()),
    ) {
        (None, None) => None,
        (drafted, stored) => drafted.clone().or(stored),
    };

    let cae = match &draft.cae {
        Some(list) if !list.is_empty() => list.clone(),
        _ => match activity {
            Some(activity) if !activity.cae.is_empty() => activity.cae.clone(),
            _ => vec![default_cae()],
        },
    };

    // Deliberately NOT defaulted to false. Absent means "nobody answered", which is a
    // different fact from "no", and the recapitulativa frequency cannot be settled
    // without the answer.
    let above_50k = match (
        draft.intra_community_operations_above_50k,
        activity.and_then(|activity| activity.intra_community_operations_above_50k),
    ) {
        (None, None) => None,
        (drafted, stored) => drafted.or(stored),
    };

    Ok(TaxProfile {
        nif: normalise_nif(&nif),
        name,
        resident_pt: draft
            .resident_pt
            .or(current.map(|p| p.resident_pt))
            .unwrap_or(true),
        is_company: draft
            .is_company
            .or(current.map(|p| p.is_company))
            .unwrap_or(false),
        activity: ActivityProfile {
            category_b: draft
                .category_b
                .or(activity.map(|activity| activity.category_b))
                .unwrap_or(true),
            start_date,
            cae,
            turnover_previous_year_cents: turnover_previous,
            turnover_current_year_expected_cents: turnover_expected,
            intra_community_operations: draft
                .intra_community_operations
                .or(activity.map(|activity| activity.intra_community_operations))
                .unwrap_or(false),
            intra_community_operations_above_50k: above_50k,
            exports: draft
                .exports
                .or(activity.map(|activity| activity.exports))
                .unwrap_or(false),
            // The three technical flags are not collected by any form; they come
            // from the stored profile or from the defaults.
            has_employees: activity.is_some_and(|activity| activity.has_employees),
            uses_certified_invoicing_software: activity
                .is_none_or(|activity| activity.uses_certified_invoicing_software),
            uses_at_webservice: activity.is_some_and(|activity| activity.uses_at_webservice),
        },
        tracking_start,
        iva: IvaChoice { regime: iva_regime },
        irs: IrsChoice {
            regime: draft
                .irs_regime
                .or(current.map(|p| p.irs.regime))
                .unwrap_or(IrsRegime::Simplificado),
            coefficient_bp: coefficient_bp
                .or(current.map(|p| p.irs.coefficient_bp))
                .unwrap_or(7500),
        },
        ss: SsChoice {
            startup_exemption_active: draft
                .startup_exemption_active
                .or(current.map(|p| p.ss.startup_exemption_active))
                .unwrap_or(false),
            first_activity_date: Some(first_activity_date),
        },
    })
}

/// Create a profile from the fields a first run collects.
///
/// # Errors
///
/// Whatever [`build_profile`] refuses.
pub fn create_default_profile(input: &ProfileInput) -> Result<TaxProfile, ProfileError> {
    let draft = ProfileDraft {
        nif: Some(input.nif.clone()),
        name: Some(input.name.clone()),
        iva_regime: input.iva_regime,
        irs_regime: input.irs_regime,
        start_date: input.start_date.clone(),
        tracking_start: input.tracking_start.clone(),
        turnover_previous_year_cents: input.turnover_previous_year_cents.map(Some),
        turnover_current_year_expected_cents: input.turnover_current_year_expected_cents.map(Some),
        cae: input.cae.clone(),
        category_b: input.category_b,
        resident_pt: input.resident_pt,
        is_company: input.is_company,
        intra_community_operations: input.intra_community_operations,
        intra_community_operations_above_50k: input.intra_community_operations_above_50k,
        exports: input.exports,
        startup_exemption_active: input.startup_exemption_active,
    };
    build_profile(&draft, None, input.coefficient_bp)
}

/// `No.mês "value" não é reconhecido` needs the value rendered as JavaScript would.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .collect::<Vec<String>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// Whether a string has the shape `YYYY-MM-DD`, without judging the values.
fn is_iso_shape(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0_usize, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|index| bytes[*index].is_ascii_digit())
}

/// Read a regime value, refusing one the AT does not have.
fn read_regime<T: Copy>(
    value: Option<&Value>,
    allowed: &[(T, &str)],
    label: &str,
) -> Result<Option<T>, ProfileError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let text = match value {
        Value::String(text) => text.clone(),
        other => js_string(other),
    };
    allowed
        .iter()
        .find(|(_, name)| *name == text)
        .map(|(regime, _)| Some(*regime))
        .ok_or_else(|| ProfileError::UnknownRegime {
            label: label.to_owned(),
            value: text,
            allowed: allowed
                .iter()
                .map(|(_, name)| *name)
                .collect::<Vec<&str>>()
                .join(", "),
        })
}

/// Read an optional date, refusing one that is not in the ISO shape.
fn read_optional_date(value: Option<&Value>, label: &str) -> Result<Option<String>, ProfileError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.is_empty() => Ok(None),
        Some(Value::String(text)) if is_iso_shape(text) => Ok(Some(text.clone())),
        Some(_) => Err(ProfileError::BadDate {
            label: label.to_owned(),
        }),
    }
}

/// Read an optional amount in cents.
fn read_optional_cents(value: Option<&Value>, field: &str) -> Result<Option<i64>, ProfileError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => match number.as_i64() {
            Some(cents) if cents >= 0 => Ok(Some(cents)),
            _ => Err(ProfileError::BadCents {
                field: field.to_owned(),
            }),
        },
        Some(_) => Err(ProfileError::BadCents {
            field: field.to_owned(),
        }),
    }
}

/// Read the CAE list, refusing anything that is not a list of entries.
fn read_cae(value: Option<&Value>) -> Result<Option<Vec<CaeEntry>>, ProfileError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Value::Array(entries) = value else {
        return Err(ProfileError::BadCaeList);
    };
    if entries.is_empty() {
        return Err(ProfileError::BadCaeList);
    }
    let mut codes = Vec::with_capacity(entries.len());
    for entry in entries {
        let Value::Object(fields) = entry else {
            return Err(ProfileError::BadCaeEntry);
        };
        let code = fields.get("code").and_then(Value::as_str).map(str::to_owned);
        let description = fields
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let (Some(code), Some(description)) = (code, description) else {
            return Err(ProfileError::CaeMissingText);
        };
        let role = match fields.get("role").and_then(Value::as_str) {
            Some("principal") => CaeRole::Principal,
            Some("secondary") => CaeRole::Secondary,
            _ => return Err(ProfileError::CaeBadRole { code }),
        };
        codes.push(CaeEntry {
            code,
            description,
            role,
        });
    }
    Ok(Some(codes))
}

/// A profile that arrived as a file, turned back into a profile.
///
/// An import is not a trusted operation: the file may have been hand-edited, may
/// come from an older version, or may not be a profile at all. Every unknown is a
/// problem for [`validate_profile`] to report, but a value that is the wrong *type*
/// is refused here, because storing it would make every later calculation nonsense
/// rather than merely wrong.
///
/// # Errors
///
/// [`ProfileError`] naming the field that could not be read.
pub fn normalise_imported_profile(value: &Value) -> Result<TaxProfile, ProfileError> {
    let Value::Object(root) = value else {
        return Err(ProfileError::NotAnObject);
    };
    let (Some(nif), Some(name)) = (
        root.get("nif").and_then(Value::as_str),
        root.get("name").and_then(Value::as_str),
    ) else {
        return Err(ProfileError::NifAndNameNotText);
    };

    let block = |key: &str| -> Option<&serde_json::Map<String, Value>> {
        match root.get(key) {
            Some(Value::Object(map)) => Some(map),
            _ => None,
        }
    };
    let empty = serde_json::Map::new();
    let activity = block("activity").unwrap_or(&empty);
    let iva = block("iva").unwrap_or(&empty);
    let irs = block("irs").unwrap_or(&empty);
    let ss = block("ss").unwrap_or(&empty);

    let iva_regime = read_regime(
        iva.get("regime"),
        &[
            (IvaRegime::IsentoArt53, "isento_art53"),
            (IvaRegime::Trimestral, "trimestral"),
            (IvaRegime::Mensal, "mensal"),
        ],
        "o regime de IVA",
    )?;
    let irs_regime = read_regime(
        irs.get("regime"),
        &[
            (IrsRegime::Simplificado, "simplificado"),
            (IrsRegime::Organizada, "organizada"),
        ],
        "o regime de IRS",
    )?;

    let draft = ProfileDraft {
        nif: Some(nif.to_owned()),
        name: Some(name.to_owned()),
        iva_regime,
        irs_regime,
        start_date: read_optional_date(activity.get("startDate"), "a data de início de atividade")?,
        tracking_start: read_optional_date(root.get("trackingStart"), "a data de início do acompanhamento")?,
        turnover_previous_year_cents: read_optional_cents(
            activity.get("turnoverPreviousYearCents"),
            "turnoverPreviousYearCents",
        )?
        .map(Some),
        turnover_current_year_expected_cents: read_optional_cents(
            activity.get("turnoverCurrentYearExpectedCents"),
            "turnoverCurrentYearExpectedCents",
        )?
        .map(Some),
        cae: read_cae(activity.get("cae"))?,
        category_b: activity.get("categoryB").and_then(Value::as_bool),
        resident_pt: root.get("residentPT").and_then(Value::as_bool),
        is_company: root.get("isCompany").and_then(Value::as_bool),
        intra_community_operations: activity.get("intraCommunityOperations").and_then(Value::as_bool),
        intra_community_operations_above_50k: activity
            .get("intraCommunityOperationsAbove50k")
            .and_then(Value::as_bool),
        exports: activity.get("exports").and_then(Value::as_bool),
        startup_exemption_active: ss.get("startupExemptionActive").and_then(Value::as_bool),
    };
    let coefficient = irs.get("coefficientBp").and_then(Value::as_i64);
    build_profile(&draft, None, coefficient)
}

/// The inputs only the taxpayer can supply.
///
/// Kept separate from [`validate_profile`] because the two mean different things: a
/// validation problem is "what you typed is wrong", while a missing input is "the
/// application cannot run this check at all without your number".
#[must_use]
pub fn missing_profile_inputs(profile: &TaxProfile) -> Vec<String> {
    let mut missing = Vec::new();
    if profile.activity.turnover_previous_year_cents.is_none() {
        missing.push(
            "volume de negócios do ano anterior em território nacional — obrigatório para avaliar o \
             art. 53.º do CIVA e para avisar quando o limite se aproxima"
                .to_owned(),
        );
    }
    if profile.activity.turnover_current_year_expected_cents.is_none() {
        missing.push(
            "estimativa de volume de negócios para o ano corrente — é o que permite avisar a meio do \
             ano se o limite do art. 53.º está a ser ultrapassado"
                .to_owned(),
        );
    }
    if profile.activity.start_date == UNDECLARED_START_DATE {
        missing.push("data de início de atividade".to_owned());
    }
    if !profile.activity.intra_community_operations && !profile.activity.exports {
        missing.push(
            "se faturas a clientes fora de Portugal (UE ou países terceiros) — determina a declaração \
             recapitulativa, o VIES e o tratamento de IVA nas faturas"
                .to_owned(),
        );
    }
    missing
}

/// Whole months between an ISO start date and an ISO reference date.
///
/// Month starts are compared, which is what the reference does: a start on the 20th
/// and a reference on the 2nd still count as one month.
fn months_since(start_date: &str, today: &str) -> Option<i64> {
    if !is_iso_shape(start_date) || !is_iso_shape(today) {
        return None;
    }
    let start_year: i64 = start_date.get(..4)?.parse().ok()?;
    let start_month: i64 = start_date.get(5..7)?.parse().ok()?;
    let now_year: i64 = today.get(..4)?.parse().ok()?;
    let now_month: i64 = today.get(5..7)?.parse().ok()?;
    Some((now_year - start_year) * 12 + (now_month - start_month))
}

/// What is wrong with a profile, if anything.
///
/// `today` is a parameter rather than a clock read: the twelve-month exemption
/// window is a fact about the calendar, and a check that depends on when it is run
/// cannot be tested and cannot be trusted.
#[must_use]
pub fn validate_profile(profile: &TaxProfile, today: &str) -> Vec<ProfileProblem> {
    let mut problems: Vec<ProfileProblem> = Vec::new();
    fn error(problems: &mut Vec<ProfileProblem>, field: &str, message: String) {
        problems.push(ProfileProblem {
            level: ProblemLevel::Error,
            field: field.to_owned(),
            message,
        });
    }

    if !is_valid_pt_tax_number(&profile.nif) {
        error(
            &mut problems,
            "nif",
            format!(
                "\"{}\" não é um NIF válido (o dígito de controlo não confere).",
                profile.nif
            ),
        );
    }
    if profile.name.trim().is_empty() {
        error(
            &mut problems,
            "name",
            "o nome do contribuinte está vazio.".to_owned(),
        );
    }
    if profile.activity.cae.is_empty() {
        error(
            &mut problems,
            "activity.cae",
            "não há qualquer CAE definido.".to_owned(),
        );
    }
    if !profile
        .activity
        .cae
        .iter()
        .any(|entry| entry.role == CaeRole::Principal)
    {
        problems.push(ProfileProblem {
            level: ProblemLevel::Warning,
            field: "activity.cae".to_owned(),
            message: "não há CAE principal definido; o CAE principal determina a atividade declarada."
                .to_owned(),
        });
    }
    if !is_iso_shape(&profile.activity.start_date) {
        error(
            &mut problems,
            "activity.startDate",
            "a data de início de atividade não está em formato AAAA-MM-DD.".to_owned(),
        );
    }
    if !profile.activity.category_b {
        problems.push(ProfileProblem {
            level: ProblemLevel::Warning,
            field: "activity.categoryB".to_owned(),
            message: "sem atividade aberta na categoria B não é possível emitir recibos verdes: \
                      confirma o início de atividade."
                .to_owned(),
        });
    }

    // The art. 53.º evaluation is impossible without this figure, and a tool that
    // guesses it is a tool that cannot be trusted to say whether the exemption
    // applies. Better to name the missing input than to assume a value.
    if profile.activity.turnover_previous_year_cents.is_none() {
        problems.push(ProfileProblem {
            level: ProblemLevel::Warning,
            field: "activity.turnoverPreviousYearCents".to_owned(),
            message: "falta o volume de negócios do ano anterior em território nacional. Sem esse \
                      valor a aplicação não pode avaliar a isenção do art. 53.º do CIVA nem avisar \
                      quando o limite se aproxima."
                .to_owned(),
        });
    }

    // Art. 53.º CIVA requires national domicile and no export operations.
    if profile.iva.regime == IvaRegime::IsentoArt53 {
        if !profile.resident_pt {
            error(
                &mut problems,
                "iva.regime",
                "o regime especial de isenção do art. 53.º do CIVA exige domicílio ou sede em \
                 território nacional."
                    .to_owned(),
            );
        }
        if profile.activity.exports {
            error(
                &mut problems,
                "iva.regime",
                "o regime especial de isenção do art. 53.º do CIVA não é aplicável a quem pratique \
                 operações de exportação ou atividades conexas. O perfil indica exportações."
                    .to_owned(),
            );
        }
    }

    if profile.irs.regime == IrsRegime::Simplificado && profile.irs.coefficient_bp <= 0 {
        error(
            &mut problems,
            "irs.coefficientBp",
            "o coeficiente do regime simplificado tem de ser positivo.".to_owned(),
        );
    }

    // The twelve-month window only matters when the exemption is declared active,
    // and only once it has closed.
    let startup_months = if profile.ss.startup_exemption_active {
        months_since(&profile.activity.start_date, today).filter(|months| *months > 12)
    } else {
        None
    };
    if let Some(months) = startup_months {
        problems.push(ProfileProblem {
            level: ProblemLevel::Warning,
            field: "ss.startupExemptionActive".to_owned(),
            // The reference says "Segurança Social Social Direta" — a typo in the
            // source. It is reproduced verbatim so the port is a port; correcting it
            // is a one-word change after the cutover.
            message: format!(
                "a isenção de contribuições do primeiro ano é uma janela de 12 meses e a atividade \
                 começou há {months} meses. Confirma o enquadramento na Segurança Social Social \
                 Direta."
            ),
        });
    }

    if profile.activity.intra_community_operations
        && profile.iva.regime != IvaRegime::Mensal
        && profile.iva.regime != IvaRegime::Trimestral
    {
        problems.push(ProfileProblem {
            level: ProblemLevel::Warning,
            field: "activity.intraCommunityOperations".to_owned(),
            message: "há operações intracomunitárias: confirma o registo no VIES e a obrigação de \
                      declaração recapitulativa."
                .to_owned(),
        });
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_profile() -> Result<TaxProfile, ProfileError> {
        create_default_profile(&ProfileInput {
            nif: "123456789".to_owned(),
            name: "Pessoa Exemplo".to_owned(),
            start_date: Some("2019-04-01".to_owned()),
            iva_regime: Some(IvaRegime::Trimestral),
            ..Default::default()
        })
    }

    #[test]
    fn a_minimal_draft_becomes_a_complete_profile() -> Result<(), Box<dyn std::error::Error>> {
        let profile = valid_profile()?;
        assert_eq!(profile.nif, "123456789");
        assert_eq!(profile.activity.start_date, "2019-04-01");
        assert_eq!(profile.activity.cae.len(), 1);
        assert_eq!(profile.irs.coefficient_bp, 7500);
        assert_eq!(profile.activity.turnover_previous_year_cents, None);
        assert!(profile.activity.intra_community_operations_above_50k.is_none());
        assert_eq!(profile.ss.first_activity_date.as_deref(), Some("2019-04-01"));
        Ok(())
    }

    #[test]
    fn a_draft_without_a_start_date_uses_the_sentinel_rather_than_today()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = create_default_profile(&ProfileInput {
            nif: "123456789".to_owned(),
            name: "Pessoa Exemplo".to_owned(),
            ..Default::default()
        })?;
        assert_eq!(profile.activity.start_date, UNDECLARED_START_DATE);
        assert!(missing_profile_inputs(&profile).contains(&"data de início de atividade".to_owned()));
        Ok(())
    }

    #[test]
    fn a_missing_nif_or_name_is_refused() {
        assert_eq!(
            build_profile(&ProfileDraft::default(), None, None),
            Err(ProfileError::MissingNif)
        );
        let draft = ProfileDraft {
            nif: Some("123456789".to_owned()),
            ..Default::default()
        };
        assert_eq!(build_profile(&draft, None, None), Err(ProfileError::MissingName));
    }

    #[test]
    fn an_edit_keeps_what_it_does_not_mention_and_null_deletes() -> Result<(), Box<dyn std::error::Error>> {
        let mut profile = valid_profile()?;
        profile.activity.turnover_previous_year_cents = Some(1_500_000);
        profile.activity.has_employees = true;

        // Mentioning only the name changes only the name.
        let renamed = build_profile(
            &ProfileDraft {
                name: Some("Outro Nome".to_owned()),
                ..Default::default()
            },
            Some(&profile),
            None,
        )?;
        assert_eq!(renamed.name, "Outro Nome");
        assert_eq!(renamed.activity.turnover_previous_year_cents, Some(1_500_000));
        assert!(renamed.activity.has_employees);

        // An explicit null deletes the figure.
        let cleared = build_profile(
            &ProfileDraft {
                turnover_previous_year_cents: Some(None),
                ..Default::default()
            },
            Some(&profile),
            None,
        )?;
        assert_eq!(cleared.activity.turnover_previous_year_cents, None);
        Ok(())
    }

    #[test]
    fn the_undecalred_fifty_thousand_answer_is_never_defaulted() -> Result<(), Box<dyn std::error::Error>> {
        let mut profile = valid_profile()?;
        assert!(profile.activity.intra_community_operations_above_50k.is_none());

        // A declared "no" is an answer and is carried over.
        profile.activity.intra_community_operations_above_50k = Some(false);
        let carried = build_profile(&ProfileDraft::default(), Some(&profile), None)?;
        assert_eq!(carried.activity.intra_community_operations_above_50k, Some(false));
        Ok(())
    }

    #[test]
    fn validation_names_each_problem_in_portuguese() -> Result<(), Box<dyn std::error::Error>> {
        let mut profile = valid_profile()?;
        profile.nif = "123456788".to_owned();
        profile.name = "  ".to_owned();
        profile.activity.cae.clear();
        profile.activity.start_date = "01/04/2019".to_owned();
        profile.activity.category_b = false;
        let problems = validate_profile(&profile, "2026-09-20");
        let fields: Vec<&str> = problems.iter().map(|problem| problem.field.as_str()).collect();
        assert!(fields.contains(&"nif"));
        assert!(fields.contains(&"name"));
        assert!(fields.contains(&"activity.cae"));
        assert!(fields.contains(&"activity.startDate"));
        assert!(fields.contains(&"activity.categoryB"));
        assert!(
            problems.iter().all(|problem| problem.level == ProblemLevel::Error
                || problem.level == ProblemLevel::Warning)
        );
        Ok(())
    }

    #[test]
    fn the_art53_exemption_contradictions_are_errors() -> Result<(), Box<dyn std::error::Error>> {
        let mut profile = valid_profile()?;
        profile.iva.regime = IvaRegime::IsentoArt53;
        profile.resident_pt = false;
        profile.activity.exports = true;
        let problems = validate_profile(&profile, "2026-09-20");
        let art53: Vec<&ProfileProblem> = problems
            .iter()
            .filter(|problem| problem.field == "iva.regime")
            .collect();
        assert_eq!(
            art53.len(),
            2,
            "domicile and exports are two separate contradictions"
        );
        assert!(art53.iter().all(|problem| problem.level == ProblemLevel::Error));
        Ok(())
    }

    #[test]
    fn the_twelve_month_window_depends_on_the_date_it_is_given() -> Result<(), Box<dyn std::error::Error>> {
        let mut profile = valid_profile()?;
        profile.activity.start_date = "2025-01-10".to_owned();
        profile.ss.startup_exemption_active = true;
        // Thirteen months later: the window has closed.
        let late = validate_profile(&profile, "2026-02-01");
        assert!(
            late.iter()
                .any(|problem| problem.field == "ss.startupExemptionActive"
                    && problem.message.contains("13 meses"))
        );
        // Eleven months later: it has not.
        let early = validate_profile(&profile, "2025-12-01");
        assert!(
            !early
                .iter()
                .any(|problem| problem.field == "ss.startupExemptionActive")
        );
        Ok(())
    }

    #[test]
    fn an_import_round_trips_and_refuses_what_it_cannot_read() -> Result<(), Box<dyn std::error::Error>> {
        let profile = valid_profile()?;
        let json = serde_json::to_value(&profile)?;
        let imported = normalise_imported_profile(&json)?;
        assert_eq!(imported, profile);

        assert_eq!(
            normalise_imported_profile(&serde_json::json!([1, 2])),
            Err(ProfileError::NotAnObject)
        );
        assert_eq!(
            normalise_imported_profile(&serde_json::json!({"nif": 1, "name": "x"})),
            Err(ProfileError::NifAndNameNotText)
        );
        let bad_regime = normalise_imported_profile(&serde_json::json!({
            "nif": "123456789", "name": "x", "iva": {"regime": "trimestral "},
        }));
        assert!(matches!(bad_regime, Err(ProfileError::UnknownRegime { .. })));
        let bad_date = normalise_imported_profile(&serde_json::json!({
            "nif": "123456789", "name": "x", "activity": {"startDate": "2019-4-1"},
        }));
        assert!(matches!(bad_date, Err(ProfileError::BadDate { .. })));
        let bad_cents = normalise_imported_profile(&serde_json::json!({
            "nif": "123456789", "name": "x", "activity": {"turnoverPreviousYearCents": -5},
        }));
        assert!(matches!(bad_cents, Err(ProfileError::BadCents { .. })));
        Ok(())
    }

    #[test]
    fn every_undeclared_input_is_named_and_none_is_guessed() -> Result<(), Box<dyn std::error::Error>> {
        let profile = valid_profile()?;
        let missing = missing_profile_inputs(&profile);
        assert_eq!(
            missing.len(),
            3,
            "turnover, expected turnover and foreign clients"
        );
        assert!(missing.iter().any(|line| line.contains("art. 53.º")));
        Ok(())
    }
}
