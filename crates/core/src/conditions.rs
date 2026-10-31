//! Evaluation of the rule pack's declarative conditions.
//!
//! The rule pack is untrusted data that happens to live in a file a person can
//! edit. It is therefore never evaluated as code and never used to build a property
//! path: every readable field is named explicitly in [`read_field`], so a malformed
//! pack can only ever be wrong, never dangerous.
//!
//! The comparison semantics are the reference implementation's, including the one
//! that looks like an accident and is load-bearing: **`ne` against an absent field
//! is true**. `intraCommunityOperationsAbove50k` is an optional declaration, and
//! the RITI rule asks `ne true` — so an unanswered question gets the base rule
//! (quarterly) while a declared "no" is an answer. Collapsing absent and `false`
//! here would silently change a filing frequency.

use serde_json::Value;

use crate::types::{Condition, ConditionField, ConditionOp, IrsRegime, IvaRegime, TaxProfile};

/// The complete vocabulary of fields a rule pack may ask about.
pub const CONDITION_FIELDS: [ConditionField; 12] = [
    ConditionField::IvaRegime,
    ConditionField::IrsRegime,
    ConditionField::ResidentPt,
    ConditionField::IsCompany,
    ConditionField::CategoryB,
    ConditionField::IntraCommunityOperations,
    ConditionField::IntraCommunityOperationsAbove50k,
    ConditionField::Exports,
    ConditionField::HasEmployees,
    ConditionField::UsesCertifiedInvoicingSoftware,
    ConditionField::UsesAtWebservice,
    ConditionField::StartupExemptionActive,
];

/// The operators the language offers, in the order the reference lists them.
pub const CONDITION_OPS: [ConditionOp; 8] = [
    ConditionOp::Eq,
    ConditionOp::Ne,
    ConditionOp::In,
    ConditionOp::Gt,
    ConditionOp::Gte,
    ConditionOp::Lt,
    ConditionOp::Lte,
    ConditionOp::Exists,
];

/// Whether a pack may use this field name.
#[must_use]
pub fn is_condition_field(value: &str) -> bool {
    serde_json::from_value::<ConditionField>(Value::String(value.to_owned())).is_ok()
}

/// Whether a pack may use this operator name.
#[must_use]
pub fn is_condition_op(value: &str) -> bool {
    serde_json::from_value::<ConditionOp>(Value::String(value.to_owned())).is_ok()
}

/// The string a value takes on the wire, which is what a condition compares against.
fn iva_regime_text(regime: IvaRegime) -> &'static str {
    match regime {
        IvaRegime::IsentoArt53 => "isento_art53",
        IvaRegime::Mensal => "mensal",
        IvaRegime::Trimestral => "trimestral",
    }
}

/// The string an IRS regime takes on the wire.
fn irs_regime_text(regime: IrsRegime) -> &'static str {
    match regime {
        IrsRegime::Simplificado => "simplificado",
        IrsRegime::Organizada => "organizada",
    }
}

/// Read one field of the profile.
///
/// `None` means the reference implementation's `undefined`: the field is absent
/// rather than false. Exactly one field can be absent — the undecalred
/// intra-community threshold — and the distinction is deliberate.
#[must_use]
pub fn read_field(profile: &TaxProfile, field: ConditionField) -> Option<Value> {
    let boolean = |value: bool| Some(Value::Bool(value));
    match field {
        ConditionField::IvaRegime => Some(Value::String(iva_regime_text(profile.iva.regime).to_owned())),
        ConditionField::IrsRegime => Some(Value::String(irs_regime_text(profile.irs.regime).to_owned())),
        ConditionField::ResidentPt => boolean(profile.resident_pt),
        ConditionField::IsCompany => boolean(profile.is_company),
        ConditionField::CategoryB => boolean(profile.activity.category_b),
        ConditionField::IntraCommunityOperations => boolean(profile.activity.intra_community_operations),
        ConditionField::IntraCommunityOperationsAbove50k => profile
            .activity
            .intra_community_operations_above_50k
            .map(Value::Bool),
        ConditionField::Exports => boolean(profile.activity.exports),
        ConditionField::HasEmployees => boolean(profile.activity.has_employees),
        ConditionField::UsesCertifiedInvoicingSoftware => {
            boolean(profile.activity.uses_certified_invoicing_software)
        }
        ConditionField::UsesAtWebservice => boolean(profile.activity.uses_at_webservice),
        ConditionField::StartupExemptionActive => boolean(profile.ss.startup_exemption_active),
    }
}

/// Equality the way the reference's `===` behaves for the values a pack can carry.
///
/// Numbers compare numerically (so `1` and `1.0` are equal, as in JavaScript);
/// everything else compares structurally.
fn same(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        _ => actual == expected,
    }
}

/// Evaluate one condition against a profile.
#[must_use]
pub fn test_condition(profile: &TaxProfile, condition: &Condition) -> bool {
    let actual = read_field(profile, condition.field);
    let expected = condition.value.as_ref();
    match condition.op {
        ConditionOp::Exists => match &actual {
            Some(value) => !value.is_null(),
            None => false,
        },
        ConditionOp::Eq => match (&actual, expected) {
            (Some(actual), Some(expected)) => same(actual, expected),
            (None, None) => true,
            _ => false,
        },
        // An absent field is "not equal", which is what makes an unanswered
        // optional declaration fall through to the base rule.
        ConditionOp::Ne => match (&actual, expected) {
            (Some(actual), Some(expected)) => !same(actual, expected),
            (None, None) => false,
            _ => true,
        },
        ConditionOp::In => match (&actual, expected) {
            (Some(actual), Some(Value::Array(values))) => {
                values.iter().any(|candidate| same(actual, candidate))
            }
            _ => false,
        },
        ConditionOp::Gt | ConditionOp::Gte | ConditionOp::Lt | ConditionOp::Lte => {
            numeric(condition.op, actual.as_ref(), expected)
        }
    }
}

/// The numeric half of the language.
///
/// No profile field is numeric today, so this branch is unreachable from real data
/// — which is exactly why it is tested directly rather than through a profile. A
/// future field (a turnover, a headcount) would make it live, and it should be
/// right when it does.
fn numeric(op: ConditionOp, actual: Option<&Value>, expected: Option<&Value>) -> bool {
    let (Some(Value::Number(left)), Some(Value::Number(right))) = (actual, expected) else {
        return false;
    };
    let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) else {
        return false;
    };
    match op {
        ConditionOp::Gt => left > right,
        ConditionOp::Gte => left >= right,
        ConditionOp::Lt => left < right,
        _ => left <= right,
    }
}

/// Whether every condition holds. An empty list is always applicable.
#[must_use]
pub fn applies_to(profile: &TaxProfile, conditions: &[Condition]) -> bool {
    conditions
        .iter()
        .all(|condition| test_condition(profile, condition))
}

/// How a value is quoted back to the person.
fn describe_value(value: Option<&Value>) -> String {
    match value {
        None => "(sem valor)".to_owned(),
        Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .collect::<Vec<String>>()
            .join(" | "),
        Some(other) => other.to_string(),
    }
}

/// Which words a failure reason uses for the field.
///
/// The reference takes an optional label map and falls back to the field's wire
/// name when it is not given; the calendar always passes the Portuguese labels, and
/// a caller that does not gets `iva.regime` rather than "regime de IVA". Keeping
/// both in one enum means the two callers cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldLabels {
    /// The field's name on the wire, `iva.regime`.
    Raw,
    /// The Portuguese phrase the interface shows, "regime de IVA".
    Portuguese,
}

/// The field's name on the wire.
#[must_use]
pub fn field_name(field: ConditionField) -> &'static str {
    match field {
        ConditionField::IvaRegime => "iva.regime",
        ConditionField::IrsRegime => "irs.regime",
        ConditionField::ResidentPt => "profile.residentPT",
        ConditionField::IsCompany => "profile.isCompany",
        ConditionField::CategoryB => "activity.categoryB",
        ConditionField::IntraCommunityOperations => "activity.intraCommunityOperations",
        ConditionField::IntraCommunityOperationsAbove50k => "activity.intraCommunityOperationsAbove50k",
        ConditionField::Exports => "activity.exports",
        ConditionField::HasEmployees => "activity.hasEmployees",
        ConditionField::UsesCertifiedInvoicingSoftware => "activity.usesCertifiedInvoicingSoftware",
        ConditionField::UsesAtWebservice => "activity.usesAtWebservice",
        ConditionField::StartupExemptionActive => "ss.startupExemptionActive",
    }
}

/// Portuguese labels for the condition vocabulary, used in "não aplicável" reasons.
#[must_use]
pub fn field_label(field: ConditionField) -> &'static str {
    match field {
        ConditionField::IvaRegime => "regime de IVA",
        ConditionField::IrsRegime => "regime de IRS",
        ConditionField::ResidentPt => "residência fiscal em Portugal",
        ConditionField::IsCompany => "ser uma sociedade",
        ConditionField::CategoryB => "ter atividade aberta na categoria B",
        ConditionField::IntraCommunityOperations => "efetuar operações intracomunitárias",
        ConditionField::IntraCommunityOperationsAbove50k => {
            "ter operações intracomunitárias acima de 50 000 EUR num trimestre"
        }
        ConditionField::Exports => "efetuar exportações",
        ConditionField::HasEmployees => "ter trabalhadores por conta de outrem",
        ConditionField::UsesCertifiedInvoicingSoftware => "usar software de faturação certificado",
        ConditionField::UsesAtWebservice => "comunicar faturas pelo webservice da AT",
        ConditionField::StartupExemptionActive => "estar no período de isenção de início de atividade",
    }
}

/// The words a reason uses for a field.
fn label_for(field: ConditionField, labels: FieldLabels) -> &'static str {
    match labels {
        FieldLabels::Raw => field_name(field),
        FieldLabels::Portuguese => field_label(field),
    }
}

/// Explain why an obligation does NOT apply, in Portuguese.
///
/// So the interface can show a reasoned "não aplicável" row instead of silently
/// hiding the obligation. Returns `None` when every condition holds.
#[must_use]
pub fn explain_failure(
    profile: &TaxProfile,
    conditions: &[Condition],
    labels: FieldLabels,
) -> Option<String> {
    let failing: Vec<&Condition> = conditions
        .iter()
        .filter(|condition| !test_condition(profile, condition))
        .collect();
    if failing.is_empty() {
        return None;
    }
    let parts: Vec<String> = failing
        .iter()
        .map(|condition| {
            let label = label_for(condition.field, labels);
            let actual = describe_value(read_field(profile, condition.field).as_ref());
            let expected = describe_value(condition.value.as_ref());
            match condition.op {
                ConditionOp::Exists => format!("{label} não está definido"),
                ConditionOp::Eq => {
                    format!("{label} é \"{actual}\" (a regra exige \"{expected}\")")
                }
                ConditionOp::Ne => {
                    format!("{label} é \"{actual}\" (a regra exige algo diferente de \"{expected}\")")
                }
                ConditionOp::In => {
                    format!("{label} é \"{actual}\" (a regra exige um de: {expected})")
                }
                other => format!(
                    "{label} é \"{actual}\" (a regra exige {} {expected})",
                    op_text(other)
                ),
            }
        })
        .collect();
    Some(parts.join("; "))
}

/// The operator's wire name, which is what the reason sentence quotes.
fn op_text(op: ConditionOp) -> &'static str {
    match op {
        ConditionOp::Eq => "eq",
        ConditionOp::Ne => "ne",
        ConditionOp::In => "in",
        ConditionOp::Gt => "gt",
        ConditionOp::Gte => "gte",
        ConditionOp::Lt => "lt",
        ConditionOp::Lte => "lte",
        ConditionOp::Exists => "exists",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ActivityProfile, CaeEntry, CaeRole, IrsChoice, IvaChoice, SsChoice};
    use serde_json::json;

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

    fn condition(field: ConditionField, op: ConditionOp, value: Option<Value>) -> Condition {
        Condition { field, op, value }
    }

    #[test]
    fn every_operator_behaves_as_the_reference_does() {
        let profile = profile();
        // eq / ne against a string field.
        assert!(test_condition(
            &profile,
            &condition(
                ConditionField::IvaRegime,
                ConditionOp::Eq,
                Some(json!("trimestral"))
            )
        ));
        assert!(!test_condition(
            &profile,
            &condition(ConditionField::IvaRegime, ConditionOp::Eq, Some(json!("mensal")))
        ));
        assert!(test_condition(
            &profile,
            &condition(ConditionField::IvaRegime, ConditionOp::Ne, Some(json!("mensal")))
        ));
        // in
        assert!(test_condition(
            &profile,
            &condition(
                ConditionField::IvaRegime,
                ConditionOp::In,
                Some(json!(["mensal", "trimestral"]))
            )
        ));
        assert!(!test_condition(
            &profile,
            &condition(
                ConditionField::IvaRegime,
                ConditionOp::In,
                Some(json!(["mensal"]))
            )
        ));
        // exists: a declared boolean exists, an undeclared one does not.
        assert!(test_condition(
            &profile,
            &condition(ConditionField::CategoryB, ConditionOp::Exists, None)
        ));
        assert!(!test_condition(
            &profile,
            &condition(
                ConditionField::IntraCommunityOperationsAbove50k,
                ConditionOp::Exists,
                None
            )
        ));
    }

    #[test]
    fn the_numeric_operators_compare_numbers_and_refuse_anything_else() {
        assert!(numeric(ConditionOp::Gt, Some(&json!(3)), Some(&json!(2))));
        assert!(!numeric(ConditionOp::Gt, Some(&json!(2)), Some(&json!(3))));
        assert!(numeric(ConditionOp::Gte, Some(&json!(3)), Some(&json!(3))));
        assert!(numeric(ConditionOp::Lt, Some(&json!(2)), Some(&json!(3))));
        assert!(numeric(ConditionOp::Lte, Some(&json!(3)), Some(&json!(3))));
        // A string is not a number, on either side.
        assert!(!numeric(ConditionOp::Gt, Some(&json!("3")), Some(&json!(2))));
        assert!(!numeric(ConditionOp::Gt, Some(&json!(3)), Some(&json!("2"))));
        assert!(!numeric(ConditionOp::Gt, None, Some(&json!(2))));
        // No profile field is numeric, so a numeric rule never matches a real one.
        for field in CONDITION_FIELDS {
            assert!(!test_condition(
                &profile(),
                &condition(field, ConditionOp::Gt, Some(json!(0)))
            ));
        }
    }

    #[test]
    fn the_absent_option_is_not_false_and_that_changes_the_answer() {
        let mut profile = profile();
        // Undeclared: `ne true` holds, so the base (quarterly) rule applies.
        let asks_ne_true = condition(
            ConditionField::IntraCommunityOperationsAbove50k,
            ConditionOp::Ne,
            Some(json!(true)),
        );
        assert!(
            test_condition(&profile, &asks_ne_true),
            "undecalred gets the base rule"
        );
        // Declared false: `ne true` still holds — the answer is "no".
        profile.activity.intra_community_operations_above_50k = Some(false);
        assert!(test_condition(&profile, &asks_ne_true));
        // Declared true: the exception applies and the rule is excluded.
        profile.activity.intra_community_operations_above_50k = Some(true);
        assert!(!test_condition(&profile, &asks_ne_true));
    }

    #[test]
    fn an_empty_condition_list_always_applies() {
        assert!(applies_to(&profile(), &[]));
    }

    #[test]
    fn the_reason_sentence_names_the_field_and_what_the_rule_wanted() {
        let profile = profile();
        let reason = explain_failure(
            &profile,
            &[condition(
                ConditionField::IvaRegime,
                ConditionOp::Eq,
                Some(json!("mensal")),
            )],
            FieldLabels::Portuguese,
        );
        assert_eq!(
            reason,
            Some("regime de IVA é \"trimestral\" (a regra exige \"mensal\")".to_owned())
        );
        // Without a label map the reference falls back to the wire name.
        let raw = explain_failure(
            &profile,
            &[condition(
                ConditionField::IvaRegime,
                ConditionOp::Eq,
                Some(json!("mensal")),
            )],
            FieldLabels::Raw,
        );
        assert_eq!(
            raw,
            Some("iva.regime é \"trimestral\" (a regra exige \"mensal\")".to_owned())
        );
        assert_eq!(explain_failure(&profile, &[], FieldLabels::Portuguese), None);
    }

    #[test]
    fn a_reason_for_an_absent_field_says_it_is_not_defined() {
        let profile = profile();
        let reason = explain_failure(
            &profile,
            &[condition(
                ConditionField::IntraCommunityOperationsAbove50k,
                ConditionOp::Exists,
                None,
            )],
            FieldLabels::Portuguese,
        );
        assert_eq!(
            reason,
            Some(
                "ter operações intracomunitárias acima de 50 000 EUR num trimestre não está definido"
                    .to_owned()
            )
        );
    }

    #[test]
    fn several_failing_conditions_are_joined_in_order() {
        let profile = profile();
        let reason = explain_failure(
            &profile,
            &[
                condition(ConditionField::HasEmployees, ConditionOp::Eq, Some(json!(true))),
                condition(ConditionField::Exports, ConditionOp::Eq, Some(json!(true))),
            ],
            FieldLabels::Portuguese,
        )
        .unwrap_or_default();
        assert_eq!(
            reason.matches("; ").count(),
            1,
            "two clauses, one separator: {reason}"
        );
        assert!(reason.starts_with("ter trabalhadores por conta de outrem"));
    }

    #[test]
    fn the_field_and_operator_names_are_the_packs_vocabulary() {
        for field in CONDITION_FIELDS {
            let name = serde_json::to_string(&field).unwrap_or_default();
            assert!(
                is_condition_field(name.trim_matches('"')),
                "{name} must be recognised"
            );
        }
        for op in CONDITION_OPS {
            let name = serde_json::to_string(&op).unwrap_or_default();
            assert!(
                is_condition_op(name.trim_matches('"')),
                "{name} must be recognised"
            );
        }
        assert!(!is_condition_field("iva"));
        assert!(!is_condition_op("equals"));
    }
}
