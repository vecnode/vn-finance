//! Rolling a pack forward, and telling a reviewer what differs.
//!
//! The authority publishes next year's calendar weeks before it starts, and the law
//! changes when it changes. Both arrive as a new pack, and both are authored by a
//! person: nothing here fetches, guesses or infers. What *can* be automated is the
//! bookkeeping — starting a year from the previous one without carrying its published
//! dates into it, and stating exactly what changed between two packs so a reviewer
//! reads a list instead of re-reading 1 500 lines of JSON.
//!
//! Two functions, both pure:
//!
//! - [`scaffold`] takes the previous year's pack as JSON and returns the next year's,
//!   with every published date stripped and every rule marked `stale`. A rule carried
//!   forward is not a rule confirmed.
//! - [`diff`] compares two loaded packs. It is a summary, not a substitute for the
//!   diff in git: it covers what a rollover changes (constants, verification, the
//!   count of published dates, the sources) and deliberately does not re-render prose.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::rules::PackError;
use crate::types::{RulePack, RuleSource, Verification};

/// A constant whose value differs between two packs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstantChange {
    /// The dotted path, `constants.iva.rates.normal`.
    pub path: String,
    /// What the earlier pack said, `None` when it did not have the path at all.
    pub from: Option<String>,
    /// What the later pack says.
    pub to: Option<String>,
}

/// A rule whose verification state differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationChange {
    /// The rule.
    pub id: String,
    /// Its state in the earlier pack.
    pub from: Verification,
    /// Its state in the later one.
    pub to: Verification,
}

/// How many dates a rule had published, before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatesChange {
    /// The rule.
    pub id: String,
    /// How many dates its own year had in the earlier pack.
    pub from: usize,
    /// How many its own year has in the later one.
    pub to: usize,
}

/// Something about a source that differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceChange {
    /// The source.
    pub id: String,
    /// Which part of it: `url`, `sha256`, `passages`, `file`, `retrievedAt`.
    pub field: String,
    /// The earlier value, shortened for display.
    pub from: String,
    /// The later value.
    pub to: String,
}

/// What changed between two packs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDiff {
    /// The earlier pack's year.
    pub from_year: i64,
    /// The later pack's year.
    pub to_year: i64,
    /// The earlier pack's version.
    pub from_version: String,
    /// The later pack's version.
    pub to_version: String,
    /// Constants that moved, in dotted-path order.
    pub constants: Vec<ConstantChange>,
    /// Obligations the later pack adds.
    pub obligations_added: Vec<String>,
    /// Obligations it no longer declares.
    pub obligations_removed: Vec<String>,
    /// Rules whose verification state moved.
    pub verification: Vec<VerificationChange>,
    /// Rules whose published-date count moved.
    pub dates: Vec<DatesChange>,
    /// Sources the later pack adds.
    pub sources_added: Vec<String>,
    /// Sources it no longer cites.
    pub sources_removed: Vec<String>,
    /// Sources whose details moved.
    pub sources_changed: Vec<SourceChange>,
}

impl PackDiff {
    /// How many separate changes it holds.
    #[must_use]
    pub fn total(&self) -> usize {
        self.constants.len()
            + self.obligations_added.len()
            + self.obligations_removed.len()
            + self.verification.len()
            + self.dates.len()
            + self.sources_added.len()
            + self.sources_removed.len()
            + self.sources_changed.len()
    }

    /// Whether the two packs say the same thing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

// ---------------------------------------------------------------------------
// The diff
// ---------------------------------------------------------------------------

/// A value, as a reviewer reads it, with long strings shortened.
fn display(value: &Value) -> String {
    let text = match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    shorten(&text)
}

/// Shorten a hash or a URL so a line of the report stays a line.
fn shorten(text: &str) -> String {
    if text.chars().count() <= 28 {
        return text.to_owned();
    }
    let head: String = text.chars().take(24).collect();
    format!("{head}…")
}

/// Every scalar in a tree, by dotted path.
///
/// `sourceIds` is skipped: provenance moved is a source change, and listing the ids
/// twice would bury the value that actually moved. `todo` is prose for maintainers.
fn flatten(node: &Value, path: &str, out: &mut BTreeMap<String, String>) {
    match node {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten(item, &format!("{path}[{index}]"), out);
            }
        }
        Value::Object(entries) => {
            for (key, value) in entries {
                if key == "sourceIds" {
                    continue;
                }
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                flatten(value, &child, out);
            }
        }
        other => {
            out.insert(path.to_owned(), display(other));
        }
    }
}

/// Compare two packs.
#[must_use]
pub fn diff(from: &RulePack, to: &RulePack) -> PackDiff {
    let mut left: BTreeMap<String, String> = BTreeMap::new();
    let mut right: BTreeMap<String, String> = BTreeMap::new();
    flatten(&from.constants, "constants", &mut left);
    flatten(&to.constants, "constants", &mut right);

    let paths: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
    let mut constants: Vec<ConstantChange> = Vec::new();
    for path in paths {
        let before = left.get(path);
        let after = right.get(path);
        if before == after {
            continue;
        }
        constants.push(ConstantChange {
            path: path.clone(),
            from: before.cloned(),
            to: after.cloned(),
        });
    }

    let before_rules: BTreeMap<&str, &crate::types::ObligationRule> = from
        .obligations
        .iter()
        .map(|rule| (rule.id.as_str(), rule))
        .collect();
    let after_rules: BTreeMap<&str, &crate::types::ObligationRule> = to
        .obligations
        .iter()
        .map(|rule| (rule.id.as_str(), rule))
        .collect();

    let obligations_added: Vec<String> = after_rules
        .keys()
        .filter(|id| !before_rules.contains_key(**id))
        .map(|id| (*id).to_owned())
        .collect();
    let obligations_removed: Vec<String> = before_rules
        .keys()
        .filter(|id| !after_rules.contains_key(**id))
        .map(|id| (*id).to_owned())
        .collect();

    let mut verification: Vec<VerificationChange> = Vec::new();
    let mut dates: Vec<DatesChange> = Vec::new();
    for (id, rule) in &after_rules {
        let Some(previous) = before_rules.get(id) else {
            continue;
        };
        if previous.verification != rule.verification {
            verification.push(VerificationChange {
                id: (*id).to_owned(),
                from: previous.verification,
                to: rule.verification,
            });
        }
        let before_dates = previous
            .official_dates
            .get(&from.year.to_string())
            .map_or(0, Vec::len);
        let after_dates = rule.official_dates.get(&to.year.to_string()).map_or(0, Vec::len);
        if before_dates != after_dates {
            dates.push(DatesChange {
                id: (*id).to_owned(),
                from: before_dates,
                to: after_dates,
            });
        }
    }

    let before_sources: BTreeMap<&str, &RuleSource> = from
        .sources
        .iter()
        .map(|source| (source.id.as_str(), source))
        .collect();
    let after_sources: BTreeMap<&str, &RuleSource> = to
        .sources
        .iter()
        .map(|source| (source.id.as_str(), source))
        .collect();

    let sources_added: Vec<String> = after_sources
        .keys()
        .filter(|id| !before_sources.contains_key(**id))
        .map(|id| (*id).to_owned())
        .collect();
    let sources_removed: Vec<String> = before_sources
        .keys()
        .filter(|id| !after_sources.contains_key(**id))
        .map(|id| (*id).to_owned())
        .collect();

    let mut sources_changed: Vec<SourceChange> = Vec::new();
    for (id, source) in &after_sources {
        let Some(previous) = before_sources.get(id) else {
            continue;
        };
        let fields: [(&str, String, String); 5] = [
            ("url", shorten(&previous.url), shorten(&source.url)),
            (
                "sha256",
                previous.sha256.clone().unwrap_or_else(|| "—".to_owned()),
                source.sha256.clone().unwrap_or_else(|| "—".to_owned()),
            ),
            (
                "file",
                previous.file.clone().unwrap_or_else(|| "—".to_owned()),
                source.file.clone().unwrap_or_else(|| "—".to_owned()),
            ),
            (
                "retrievedAt",
                previous.retrieved_at.clone(),
                source.retrieved_at.clone(),
            ),
            (
                "passages",
                previous.passages.len().to_string(),
                source.passages.len().to_string(),
            ),
        ];
        for (field, before, after) in fields {
            if before == after {
                continue;
            }
            sources_changed.push(SourceChange {
                id: (*id).to_owned(),
                field: field.to_owned(),
                from: shorten(&before),
                to: shorten(&after),
            });
        }
    }

    PackDiff {
        from_year: from.year,
        to_year: to.year,
        from_version: from.pack_version.clone(),
        to_version: to.pack_version.clone(),
        constants,
        obligations_added,
        obligations_removed,
        verification,
        dates,
        sources_added,
        sources_removed,
        sources_changed,
    }
}

// ---------------------------------------------------------------------------
// The scaffold
// ---------------------------------------------------------------------------

/// The word the interface uses for a verification state.
#[must_use]
pub fn verification_label(state: Verification) -> &'static str {
    match state {
        Verification::Verified => "verificada",
        Verification::Partial => "parcial",
        Verification::Unverified => "por verificar",
        Verification::Stale => "desatualizada",
    }
}

/// Remove every published-date block from one object.
///
/// Both shapes the loader tolerates: `officialDates2026` and an `officialDates` map.
/// Carrying last year's calendar into this year's pack is the one mistake that would
/// make a new pack look finished while every date in it is the old year's.
fn strip_dates(entry: &mut Map<String, Value>) {
    entry.retain(|key, _| !key.starts_with("officialDates"));
}

/// The next year's pack, from this one.
///
/// The rules and their citations carry forward, because a republication of the CIRS is
/// the same text. The published dates do not, the verification does not, and the pack
/// says in its own `todo` which sources are named after a year that has passed.
///
/// # Errors
///
/// [`PackError::NotAnObject`] when the source is not a JSON object.
pub fn scaffold(from: &Value, year: i64, on: &str) -> Result<Value, PackError> {
    let Value::Object(root) = from else {
        return Err(PackError::NotAnObject);
    };
    let from_year = root.get("year").and_then(Value::as_i64).unwrap_or_default();
    let mut next = root.clone();

    next.insert("year".to_owned(), Value::from(year));
    next.insert("packVersion".to_owned(), Value::from(format!("{year}.1.0")));
    next.insert("generatedAt".to_owned(), Value::from(on.to_owned()));
    strip_dates(&mut next);

    if let Some(Value::Array(obligations)) = next.get_mut("obligations") {
        for entry in obligations {
            let Value::Object(rule) = entry else {
                continue;
            };
            strip_dates(rule);
            rule.insert("verification".to_owned(), Value::from("stale"));
            rule.insert(
                "verifyNote".to_owned(),
                Value::from(format!(
                    "Regra transportada do pacote de {from_year} para {year} sem ser reconferida. \
                     Os prazos e os valores têm de ser confirmados nas fontes de {year} antes de \
                     esta regra voltar a ser marcada como verificada."
                )),
            );
        }
    }

    let stale_sources: Vec<String> = next
        .get("sources")
        .and_then(Value::as_array)
        .map(|sources| {
            sources
                .iter()
                .filter_map(|source| source.get("id").and_then(Value::as_str))
                .filter(|id| id.contains(&from_year.to_string()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let mut note = format!(
        "Pacote de {year} criado a partir do de {from_year}: nenhuma data publicada transitou, \
         todas as regras estão marcadas como desatualizadas e a verificação tem de ser refeita nas \
         fontes de {year}."
    );
    if !stale_sources.is_empty() {
        note.push_str(&format!(
            " Estas fontes têm {from_year} no próprio id e têm de ser substituídas pelas de {year}: \
             {}.",
            stale_sources.join(", ")
        ));
    }

    let mut todo: Vec<Value> = next
        .get("todo")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    todo.push(Value::from(note));
    next.insert("todo".to_owned(), Value::Array(todo));

    Ok(Value::Object(next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{load_embedded, load_rule_pack};

    type Problem = Box<dyn std::error::Error>;

    fn shipped() -> Result<Value, Problem> {
        let text = crate::rules::EMBEDDED_PACKS
            .first()
            .map(|entry| entry.text)
            .ok_or("the binary carries no embedded pack")?;
        Ok(serde_json::from_str(text)?)
    }

    #[test]
    fn a_scaffolded_pack_is_the_next_year_with_no_published_dates() -> Result<(), Problem> {
        let from = shipped()?;
        let raw = scaffold(&from, 2027, "2026-12-01")?;
        let loaded = load_rule_pack(&raw.to_string(), "2027.json")?;

        assert_eq!(loaded.pack.year, 2027);
        assert_eq!(loaded.pack.pack_version, "2027.1.0");
        assert_eq!(loaded.pack.generated_at, "2026-12-01");
        assert!(
            loaded
                .pack
                .obligations
                .iter()
                .all(|rule| rule.official_dates.is_empty()),
            "no published date may survive the scaffold"
        );
        // And the raw JSON holds no `officialDates*` key at all, so the next person to
        // read the file cannot mistake last year's calendar for a starting point.
        let text = raw.to_string();
        assert!(
            !text.contains("officialDates"),
            "a scaffolded pack must not carry an officialDates key"
        );
        Ok(())
    }

    #[test]
    fn every_rule_carried_forward_says_it_is_not_confirmed() -> Result<(), Problem> {
        let raw = scaffold(&shipped()?, 2027, "2026-12-01")?;
        let loaded = load_rule_pack(&raw.to_string(), "2027.json")?;

        assert!(!loaded.pack.obligations.is_empty());
        for rule in &loaded.pack.obligations {
            assert_eq!(
                rule.verification,
                Verification::Stale,
                "{} was carried forward as {}",
                rule.id,
                verification_label(rule.verification)
            );
            let note = rule.verify_note.as_deref().unwrap_or_default();
            assert!(
                note.contains("2027") && note.contains("reconferida"),
                "{} keeps a note that does not say it was not checked: {note}",
                rule.id
            );
        }
        Ok(())
    }

    #[test]
    fn the_citations_and_the_sources_carry_forward() -> Result<(), Problem> {
        let from = load_embedded()?;
        let raw = scaffold(&shipped()?, 2027, "2026-12-01")?;
        let loaded = load_rule_pack(&raw.to_string(), "2027.json")?;

        assert_eq!(loaded.pack.sources.len(), from.pack.sources.len());
        // A republication of the CIRS is the same article: the quoted passages and the
        // hashes they were checked against still describe what was read.
        assert_eq!(
            loaded
                .pack
                .sources
                .iter()
                .map(|source| source.passages.len())
                .sum::<usize>(),
            from.pack
                .sources
                .iter()
                .map(|source| source.passages.len())
                .sum::<usize>()
        );
        assert_eq!(
            loaded
                .pack
                .obligations
                .iter()
                .map(|rule| rule.citations.len())
                .sum::<usize>(),
            from.pack
                .obligations
                .iter()
                .map(|rule| rule.citations.len())
                .sum::<usize>()
        );
        Ok(())
    }

    #[test]
    fn the_scaffolded_pack_names_the_sources_still_carrying_the_old_year() -> Result<(), Problem> {
        let raw = scaffold(&shipped()?, 2027, "2026-12-01")?;
        let loaded = load_rule_pack(&raw.to_string(), "2027.json")?;
        let last = loaded.pack.todo.last().ok_or("the scaffold adds a todo")?;

        assert!(last.contains("2027"), "{last}");
        assert!(
            last.contains("at-agenda-decl-2026"),
            "the annual sources must be named, not left to memory: {last}"
        );
        assert!(
            !last.contains("at-cirs-art60"),
            "an article that did not change is not an annual source: {last}"
        );
        Ok(())
    }

    #[test]
    fn a_pack_compared_with_itself_has_no_changes() -> Result<(), Problem> {
        let pack = load_embedded()?;
        let report = diff(&pack.pack, &pack.pack);
        assert!(report.is_empty(), "{report:?}");
        assert_eq!(report.total(), 0);
        Ok(())
    }

    #[test]
    fn a_rolled_over_pack_differs_in_dates_and_verification() -> Result<(), Problem> {
        let from = load_embedded()?;
        let raw = scaffold(&shipped()?, 2027, "2026-12-01")?;
        let to = load_rule_pack(&raw.to_string(), "2027.json")?;
        let report = diff(&from.pack, &to.pack);

        assert!(!report.is_empty());
        assert_eq!(report.from_year, 2026);
        assert_eq!(report.to_year, 2027);
        // Every rule that had a published date in 2026 now has none: the calendar did
        // not follow the scaffold, which is the whole point of it.
        assert!(
            report.dates.iter().any(|change| change.id == "irs.modelo3"),
            "a dated rule must show its count falling to zero: {report:?}"
        );
        assert!(
            report
                .verification
                .iter()
                .all(|change| change.to == Verification::Stale),
            "every carried rule moves to stale"
        );
        assert!(
            report
                .verification
                .iter()
                .any(|change| change.id == "irs.modelo3"),
            "{report:?}"
        );
        // Constants are the same law, so none moved.
        assert!(report.constants.is_empty(), "{:?}", report.constants);
        Ok(())
    }

    #[test]
    fn a_changed_constant_is_reported_with_both_values() -> Result<(), Problem> {
        let from = load_embedded()?;
        let raw = scaffold(&shipped()?, 2027, "2026-12-01")?;
        let text = raw
            .to_string()
            .replace("\"iasMonthly\":53713", "\"iasMonthly\":55000");
        let to = load_rule_pack(&text, "2027.json")?;

        let report = diff(&from.pack, &to.pack);
        let change = report
            .constants
            .iter()
            .find(|change| change.path == "constants.socialSecurity.iasMonthly")
            .ok_or("the IAS move must be reported")?;
        assert_eq!(change.from.as_deref(), Some("53713"));
        assert_eq!(change.to.as_deref(), Some("55000"));
        Ok(())
    }

    #[test]
    fn a_source_whose_hash_moved_is_reported() -> Result<(), Problem> {
        let from = load_embedded()?;
        let raw = scaffold(&shipped()?, 2027, "2026-12-01")?;
        let text = raw.to_string().replace(
            "\"sha256\":\"20dcfcbdc76eacab88576843725753fc4c716f1a93b7f81529b616cd1dafee6f\"",
            "\"sha256\":\"1111111111111111111111111111111111111111111111111111111111111111\"",
        );
        let to = load_rule_pack(&text, "2027.json")?;

        let report = diff(&from.pack, &to.pack);
        let change = report
            .sources_changed
            .iter()
            .find(|change| change.id == "at-cirs-art60" && change.field == "sha256")
            .ok_or("a changed copy must be reported")?;
        assert!(change.from.starts_with("20dc"), "{}", change.from);
        assert!(change.to.starts_with("1111"), "{}", change.to);
        Ok(())
    }

    #[test]
    fn a_pack_that_is_not_an_object_is_refused() {
        assert_eq!(
            scaffold(&Value::Array(Vec::new()), 2027, "2026-12-01").err(),
            Some(PackError::NotAnObject)
        );
    }
}
