//! Checking the pack's citations against the copies that were read.
//!
//! The evidence layer of the three in `ARCHITECTURE.md`, in code. The pack says where
//! a figure comes from; this module answers the only question a reader has about that
//! claim — is the sentence it cites actually in the copy the repository kept, and is
//! the copy the one that was retrieved?
//!
//! It answers it with two independent facts, and they fail separately:
//!
//! - **The hash.** The bytes are hashed and compared with the `sha256` the pack
//!   records. A source whose page changed under a rule that still says `verified` is
//!   caught here and nowhere else, because the URL keeps answering.
//! - **The quote.** Each declared passage must be present in the text extracted from
//!   those bytes, after whitespace is collapsed. Accents are compared, because the law
//!   is Portuguese and `e` is not `é`.
//!
//! Pure by construction: the corpus arrives as bytes, already read by the caller.
//! Nothing here opens a file, and a machine without the corpus gets no errors at all —
//! only sources that are not checked, which the interface says out loud.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::pdf::{extract_pdf_text, looks_like_pdf};
use crate::rules::hex;
use crate::types::{ProblemLevel, RulePack, RuleSource, Verification};

/// One document of the corpus, as the caller read it.
///
/// The path is the key in [`Corpus`], not a field here: one document is one entry, and
/// carrying its own name on it invited the two to disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusEntry {
    /// The bytes, exactly as they were retrieved.
    pub bytes: Vec<u8>,
}

/// The documents that back the pack, keyed by the path the pack names.
///
/// A path rather than a source id, because two articles of a code can live on one
/// page and the file is the thing with a hash.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Corpus {
    entries: BTreeMap<String, CorpusEntry>,
}

impl Corpus {
    /// An empty corpus, which is what a machine with no `sources/` folder has.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one document.
    pub fn insert(&mut self, path: impl Into<String>, bytes: Vec<u8>) {
        self.entries.insert(path.into(), CorpusEntry { bytes });
    }

    /// One document, by the path the pack names.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&CorpusEntry> {
        self.entries.get(path)
    }

    /// How many documents it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The total size of the stored bytes.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.entries.values().map(|entry| entry.bytes.len() as u64).sum()
    }

    /// The paths it holds, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

/// What happened to one source's local copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceState {
    /// The pack names no local copy, and does not promise one.
    NotNamed,
    /// Named, but there is no such file in the corpus.
    Absent,
    /// Present, but with no hash to prove it is the copy that was read.
    Unproven,
    /// Present, the hash matches, and every declared passage was found.
    Verified,
    /// Present, and something did not match.
    Failed,
}

impl SourceState {
    /// Whether the copy can be trusted as the one that was read.
    #[must_use]
    pub fn is_trusted(self) -> bool {
        matches!(self, Self::Verified | Self::NotNamed)
    }
}

/// The result of checking one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCheck {
    /// What happened to the copy.
    pub state: SourceState,
    /// How many of the declared passages were located.
    pub passages_found: usize,
    /// How many the pack declares.
    pub passages_expected: usize,
}

impl SourceCheck {
    fn new(state: SourceState, found: usize, expected: usize) -> Self {
        Self {
            state,
            passages_found: found,
            passages_expected: expected,
        }
    }
}

/// One thing wrong with the evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusProblem {
    /// Fatal or advisory.
    pub level: ProblemLevel,
    /// The source it is about, when it is about one.
    pub source_id: Option<String>,
    /// The rule it is about, when it is about one.
    pub rule_id: Option<String>,
    /// What is wrong, in Portuguese, ready to show.
    pub message: String,
}

/// Everything the corpus check found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusReport {
    /// The state of every source the pack declares, by id.
    pub sources: BTreeMap<String, SourceCheck>,
    /// Everything worth saying.
    pub problems: Vec<CorpusProblem>,
    /// How many documents the corpus holds.
    pub files: usize,
    /// How large they are together.
    pub bytes: u64,
    /// How many sources are verified.
    pub verified: usize,
    /// How many are named and missing.
    pub absent: usize,
    /// How many are present but did not match.
    pub failed: usize,
    /// How many are present without a hash.
    pub unproven: usize,
    /// How many declare no local copy at all.
    pub not_named: usize,
}

impl CorpusReport {
    /// The state of one source.
    #[must_use]
    pub fn state_of(&self, source_id: &str) -> SourceState {
        self.check_of(source_id).state
    }

    /// The check of one source; a source the pack does not declare reads as not named.
    #[must_use]
    pub fn check_of(&self, source_id: &str) -> SourceCheck {
        self.sources.get(source_id).cloned().unwrap_or(SourceCheck {
            state: SourceState::NotNamed,
            passages_found: 0,
            passages_expected: 0,
        })
    }

    /// Whether anything is wrong with the evidence.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.problems
            .iter()
            .any(|problem| problem.level == ProblemLevel::Error)
    }
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// Collapse whitespace the way a reader sees it.
///
/// The stored pages break a paragraph across lines and indent it with spaces; a quote
/// copied from the page will not have the same line breaks. Everything else is left
/// alone: case, punctuation and accents are the document's, not ours to smooth over.
#[must_use]
pub fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// Whether a quote occurs in already-normalised document text.
///
/// `…` splits the quote into fragments that must appear in order, for a page that puts
/// a table or a heading between two halves of a sentence. Nothing else is tolerated: a
/// fragment that is not there is a citation that does not check out, which is the one
/// answer this module must never give by accident.
#[must_use]
pub fn quote_is_present(normalised_document: &str, quote: &str) -> bool {
    let mut cursor = 0;
    for fragment in quote.split('…') {
        let fragment = normalise(fragment);
        if fragment.is_empty() {
            continue;
        }
        match normalised_document[cursor..].find(&fragment) {
            Some(at) => cursor += at + fragment.len(),
            None => return false,
        }
    }
    true
}

/// One entity, decoded. `None` when the text after `&` is not one this knows.
fn decode_entity(body: &str) -> Option<char> {
    if let Some(number) = body.strip_prefix('#') {
        let (digits, radix) = match number.strip_prefix(['x', 'X']) {
            Some(hexadecimal) => (hexadecimal, 16),
            None => (number, 10),
        };
        let value = u32::from_str_radix(digits, radix).ok()?;
        return char::from_u32(value);
    }
    match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => None,
    }
}

/// Decode the entities a page uses for the punctuation of a law.
///
/// Named entities beyond the XML five and `&nbsp;` are deliberately not guessed: an
/// unknown entity stays as written, so a quote that depends on it fails loudly instead
/// of matching something else.
#[must_use]
pub fn decode_entities(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character != '&' {
            out.push(character);
            index += 1;
            continue;
        }
        // Look for the closing `;` within a sensible window: an entity is short.
        let window_end = (index + 12).min(characters.len());
        let mut relative = None;
        for (offset, candidate) in characters[index + 1..window_end].iter().enumerate() {
            if *candidate == ';' {
                relative = Some(offset);
                break;
            }
        }
        let Some(relative) = relative else {
            out.push('&');
            index += 1;
            continue;
        };
        let body: String = characters[index + 1..index + 1 + relative].iter().collect();
        match decode_entity(&body) {
            Some(decoded) => {
                out.push(decoded);
                index += relative + 2;
            }
            None => {
                out.push('&');
                index += 1;
            }
        }
    }
    out
}

/// Where `needle` first occurs at or after `from`.
fn find_from(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

/// The text of a page, as a reader sees it: no scripts, no tags, no entities.
#[must_use]
pub fn html_to_text(bytes: &[u8]) -> String {
    let source = String::from_utf8_lossy(bytes);
    // Lower-cased by ASCII only, so the two copies keep the same byte offsets: the
    // scan below walks `lower` and slices `source` with the positions it finds.
    let lower: Vec<u8> = source.bytes().map(|byte| byte.to_ascii_lowercase()).collect();
    let mut visible = String::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        // A script or a style is not text, and a page is mostly made of them.
        let mut skipped = false;
        for (open, close) in [
            (b"<script".as_slice(), b"</script".as_slice()),
            (b"<style".as_slice(), b"</style".as_slice()),
        ] {
            if lower[index..].starts_with(open) {
                index = find_from(&lower, index, close).unwrap_or(source.len());
                skipped = true;
                break;
            }
        }
        if skipped {
            continue;
        }
        let Some(character) = source[index..].chars().next() else {
            break;
        };
        if character == '<' {
            match source[index..].find('>') {
                Some(end) => {
                    visible.push(' ');
                    index += end + 1;
                }
                None => break,
            }
            continue;
        }
        visible.push(character);
        index += character.len_utf8();
    }
    normalise(&decode_entities(&visible))
}

/// The text of a stored document, whatever it is.
///
/// A PDF goes through the reader the application already carries; anything else is
/// treated as a page. An unreadable PDF yields empty text, which makes its quotes fail
/// rather than pass — the direction a check must fail in.
#[must_use]
pub fn extract_text(entry: &CorpusEntry) -> String {
    if looks_like_pdf(&entry.bytes) {
        return normalise(&extract_pdf_text(&entry.bytes).text);
    }
    html_to_text(&entry.bytes)
}

// ---------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------

/// Check one source against the corpus.
#[must_use]
pub fn check_source(source: &RuleSource, corpus: &Corpus) -> (SourceCheck, Vec<CorpusProblem>) {
    let mut problems: Vec<CorpusProblem> = Vec::new();
    let expected = source.passages.len();
    let Some(file) = &source.file else {
        return (SourceCheck::new(SourceState::NotNamed, 0, expected), problems);
    };

    let Some(entry) = corpus.get(file) else {
        problems.push(CorpusProblem {
            level: ProblemLevel::Error,
            source_id: Some(source.id.clone()),
            rule_id: None,
            message: format!(
                "a cópia local de \"{}\" não está no corpus ({file}): a citação não pode ser \
                 conferida neste computador.",
                source.id
            ),
        });
        return (SourceCheck::new(SourceState::Absent, 0, expected), problems);
    };

    let actual = hex(&Sha256::digest(&entry.bytes));
    let hash_matches = match &source.sha256 {
        Some(expected_hash) => expected_hash.eq_ignore_ascii_case(&actual),
        None => false,
    };
    if let Some(expected_hash) = &source.sha256
        && !hash_matches
    {
        problems.push(CorpusProblem {
            level: ProblemLevel::Error,
            source_id: Some(source.id.clone()),
            rule_id: None,
            message: format!(
                "a cópia local de \"{}\" ({file}) já não é a que foi lida: o sha256 é {actual}, e o \
                 pacote registou {expected_hash}. O documento mudou, ou a cópia foi substituída.",
                source.id
            ),
        });
    }

    let document = extract_text(entry);
    let mut found = 0;
    for passage in &source.passages {
        if quote_is_present(&document, &passage.quote) {
            found += 1;
            continue;
        }
        let locator = match &passage.locator {
            Some(locator) => format!(" ({locator})"),
            None => String::new(),
        };
        problems.push(CorpusProblem {
            level: ProblemLevel::Error,
            source_id: Some(source.id.clone()),
            rule_id: None,
            message: format!(
                "a passagem \"{}\"{locator} não se encontra em {file}: a citação não confere com a \
                 cópia que está guardada.",
                passage.id
            ),
        });
    }

    let state = if source.sha256.is_none() {
        SourceState::Unproven
    } else if !hash_matches || found < expected {
        SourceState::Failed
    } else {
        SourceState::Verified
    };
    (SourceCheck::new(state, found, expected), problems)
}

/// Check every source, and every rule that says it is verified.
#[must_use]
pub fn check(pack: &RulePack, corpus: &Corpus) -> CorpusReport {
    let mut report = CorpusReport {
        files: corpus.len(),
        bytes: corpus.bytes(),
        ..CorpusReport::default()
    };

    for source in &pack.sources {
        let (check, problems) = check_source(source, corpus);
        match check.state {
            SourceState::Verified => report.verified += 1,
            SourceState::Absent => report.absent += 1,
            SourceState::Failed => report.failed += 1,
            SourceState::Unproven => report.unproven += 1,
            SourceState::NotNamed => report.not_named += 1,
        }
        report.problems.extend(problems);
        report.sources.insert(source.id.clone(), check);
    }

    // A rule that says "verified" while the copy of the article behind it is missing
    // is the one combination this whole module exists to make impossible to miss.
    for rule in &pack.obligations {
        if rule.verification != Verification::Verified {
            continue;
        }
        for source_id in &rule.source_ids {
            let state = report.state_of(source_id);
            if state.is_trusted() {
                continue;
            }
            report.problems.push(CorpusProblem {
                level: ProblemLevel::Warning,
                source_id: Some(source_id.clone()),
                rule_id: Some(rule.id.clone()),
                message: format!(
                    "a regra \"{}\" está marcada como verificada, mas a fonte \"{source_id}\" não \
                     tem uma cópia local conferida — a verificação assenta numa página que ninguém \
                     guardou.",
                    rule.id
                ),
            });
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::load_rule_pack;

    /// A one-rule pack whose single source is named `file` with `sha256` and passages.
    fn pack(source_extra: &str, citations: &str) -> Result<RulePack, Box<dyn std::error::Error>> {
        let text = format!(
            r#"{{
              "jurisdiction": "PT",
              "year": 2026,
              "units": {{ "money": "EUR_cents", "rate": "basis_points" }},
              "disclaimer": "teste",
              "sources": [{{
                "id": "fonte",
                "tier": "official",
                "authority": "AT",
                "title": "Um artigo",
                "url": "https://example.invalid/artigo",
                "retrievedAt": "2026-11-18"
                {source_extra}
              }}],
              "constants": {{}},
              "obligations": [{{
                "id": "uma.regra",
                "kind": "declare",
                "authority": "AT",
                "tax": "IVA",
                "title": "Uma regra",
                "appliesWhen": [],
                "periodicity": {{ "type": "annual" }},
                "deadlineRule": {{ "kind": "day_of_month", "day": 20 }},
                "legalBasis": ["CIVA art. 41.º"],
                "sourceIds": ["fonte"],
                "verification": "verified"
                {citations}
              }}],
              "todo": []
            }}"#
        );
        Ok(load_rule_pack(&text, "teste.json")?.pack)
    }

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    const PAGE: &str = "<html><head><style>p{color:red}</style><script>var x = '<p>';</script></head>\
        <body><p>Artigo 101.&#186;-B</p><p>Dispensa de reten&#231;&#227;o na fonte</p>\
        <p>d) Os rendimentos das categorias B, E e F, sempre que o montante de cada\n\
        reten&#231;&#227;o seja inferior a &#8364; 25;</p></body></html>";

    #[test]
    fn a_page_becomes_the_text_a_reader_sees() {
        let text = html_to_text(PAGE.as_bytes());
        assert!(text.contains("Artigo 101.º-B"), "was: {text}");
        assert!(text.contains("Dispensa de retenção na fonte"), "was: {text}");
        assert!(
            text.contains("sempre que o montante de cada retenção seja inferior a € 25"),
            "the line break inside the paragraph must not survive: {text}"
        );
        assert!(!text.contains("color:red"), "a style is not text: {text}");
        assert!(!text.contains("var x"), "a script is not text: {text}");
    }

    #[test]
    fn a_quote_broken_by_a_line_break_still_matches() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = PAGE.as_bytes().to_vec();
        let mut corpus = Corpus::new();
        corpus.insert("artigo.html", bytes.clone());
        let loaded = pack(
            &format!(
                r#",
                 "file": "artigo.html",
                 "sha256": "{}",
                 "passages": [
                   {{ "id": "limiar", "locator": "art. 101.º-B n.º 1 d)",
                      "quote": "sempre que o montante de cada retenção seja inferior a € 25" }}
                 ]"#,
                sha(&bytes)
            ),
            r#",
             "citations": [{ "sourceId": "fonte", "passageId": "limiar" }]"#,
        )?;

        let report = check(&loaded, &corpus);
        assert_eq!(
            report.state_of("fonte"),
            SourceState::Verified,
            "{:?}",
            report.problems
        );
        assert_eq!(report.verified, 1);
        assert!(!report.has_errors(), "{:?}", report.problems);
        Ok(())
    }

    #[test]
    fn a_quote_that_is_not_in_the_copy_is_an_error() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = PAGE.as_bytes().to_vec();
        let mut corpus = Corpus::new();
        corpus.insert("artigo.html", bytes.clone());
        let loaded = pack(
            &format!(
                r#",
                 "file": "artigo.html",
                 "sha256": "{}",
                 "passages": [{{ "id": "inventada", "quote": "o imposto é de 30 %" }}]"#,
                sha(&bytes)
            ),
            "",
        )?;

        let report = check(&loaded, &corpus);
        assert_eq!(report.state_of("fonte"), SourceState::Failed);
        assert!(report.has_errors());
        assert!(
            report
                .problems
                .iter()
                .any(|problem| problem.message.contains("inventada")),
            "{:?}",
            report.problems
        );
        Ok(())
    }

    #[test]
    fn a_copy_whose_hash_moved_is_an_error_even_when_every_quote_still_matches()
    -> Result<(), Box<dyn std::error::Error>> {
        let bytes = PAGE.as_bytes().to_vec();
        let mut corpus = Corpus::new();
        corpus.insert("artigo.html", bytes);
        let loaded = pack(
            r#",
             "file": "artigo.html",
             "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
             "passages": [{ "id": "titulo", "quote": "Dispensa de retenção na fonte" }]"#,
            "",
        )?;

        let report = check(&loaded, &corpus);
        assert_eq!(report.state_of("fonte"), SourceState::Failed);
        assert!(
            report
                .problems
                .iter()
                .any(|problem| problem.message.contains("já não é a que foi lida")),
            "{:?}",
            report.problems
        );
        Ok(())
    }

    #[test]
    fn a_source_named_but_not_stored_is_absent_and_says_so() -> Result<(), Box<dyn std::error::Error>> {
        let loaded = pack(r#", "file": "nao-existe.html", "sha256": "abc""#, "")?;
        let report = check(&loaded, &Corpus::new());
        assert_eq!(report.state_of("fonte"), SourceState::Absent);
        assert!(report.has_errors());
        assert_eq!(report.files, 0);
        Ok(())
    }

    #[test]
    fn a_copy_without_a_hash_cannot_be_called_verified() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = PAGE.as_bytes().to_vec();
        let mut corpus = Corpus::new();
        corpus.insert("artigo.html", bytes);
        let loaded = pack(r#", "file": "artigo.html""#, "")?;

        let report = check(&loaded, &corpus);
        assert_eq!(report.state_of("fonte"), SourceState::Unproven);
        assert!(!report.has_errors(), "unproven is not a failure, it is a gap");
        assert!(!SourceState::Unproven.is_trusted());
        Ok(())
    }

    #[test]
    fn a_source_with_no_copy_is_not_an_error() -> Result<(), Box<dyn std::error::Error>> {
        let loaded = pack("", "")?;
        let report = check(&loaded, &Corpus::new());
        assert_eq!(report.state_of("fonte"), SourceState::NotNamed);
        assert_eq!(report.not_named, 1);
        assert!(!report.has_errors());
        Ok(())
    }

    #[test]
    fn a_verified_rule_whose_promised_copy_is_missing_is_a_warning_naming_both_sides()
    -> Result<(), Box<dyn std::error::Error>> {
        let loaded = pack(r#", "file": "nao-existe.html", "sha256": "abc""#, "")?;
        let report = check(&loaded, &Corpus::new());
        let warning = report
            .problems
            .iter()
            .find(|problem| problem.level == ProblemLevel::Warning)
            .ok_or("the cross-check must fire")?;
        assert_eq!(warning.rule_id.as_deref(), Some("uma.regra"));
        assert_eq!(warning.source_id.as_deref(), Some("fonte"));
        assert!(warning.message.contains("verificada"), "{}", warning.message);
        Ok(())
    }

    #[test]
    fn a_source_the_pack_never_promised_to_keep_does_not_raise_the_alarm()
    -> Result<(), Box<dyn std::error::Error>> {
        // Most of the pack's secondary sources are like this: the value rests on a
        // page the repository does not redistribute. That is a gap, and the summary
        // counts it, but it must not paint a verified rule as broken — the alarm is
        // for "you said you keep a copy and it is not here".
        let loaded = pack("", "")?;
        let report = check(&loaded, &Corpus::new());
        assert!(
            !report
                .problems
                .iter()
                .any(|problem| problem.level == ProblemLevel::Warning),
            "{:?}",
            report.problems
        );
        assert_eq!(report.not_named, 1);
        Ok(())
    }

    #[test]
    fn an_ellipsis_joins_fragments_and_the_order_is_enforced() {
        let document = normalise("Artigo 41.º O prazo é o dia 20 do segundo mês seguinte");
        assert!(quote_is_present(&document, "Artigo 41.º … dia 20"));
        assert!(!quote_is_present(&document, "dia 20 … Artigo 41.º"));
    }

    #[test]
    fn accents_are_compared_rather_than_smoothed_over() {
        let document = normalise("A declaração de alterações é entregue");
        assert!(quote_is_present(&document, "declaração de alterações"));
        assert!(
            !quote_is_present(&document, "declaracao de alteracoes"),
            "an accent stripped is a different sentence"
        );
    }

    #[test]
    fn an_unknown_entity_stays_as_written() {
        assert_eq!(decode_entities("a&b; c"), "a&b; c");
        assert_eq!(decode_entities("&#233; e &#xE9;"), "é e é");
        assert_eq!(decode_entities("&amp;lt;"), "&lt;");
    }
}
