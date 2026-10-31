//! The document index and the names it stores.
//!
//! A document is archived under the first twelve characters of its SHA-256, which is
//! what lets the application prove later that a file has not changed and answer "was
//! this PDF already filed?" without reading the ledger.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One archived document, as the index stores it.
///
/// `original_path`, `obligation_id` and `invoice_id` are written as `null` when
/// absent, and `original_path` is `null` for bytes that arrived from a file dialog
/// — there is no original on this filesystem to point at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEntry {
    /// The stored name, `documents/<hash>-<name>`.
    pub file: String,
    /// Where it came from, when it came from a path.
    pub original_path: Option<String>,
    /// The content hash.
    pub sha256: String,
    /// When it was archived.
    pub added_at: String,
    /// What kind of document it is.
    pub kind: String,
    /// The obligation it documents, when it documents one.
    pub obligation_id: Option<String>,
    /// The ledger invoice it became, when it is a `fatura-recibo` that was read.
    pub invoice_id: Option<String>,
}

/// What to record about a document that is being archived.
#[derive(Debug, Clone, Copy, Default)]
pub struct DocumentMeta<'a> {
    /// What kind of document it is; `None` means "outro".
    pub kind: Option<&'a str>,
    /// The obligation it documents.
    pub obligation_id: Option<&'a str>,
    /// The ledger invoice it becomes.
    pub invoice_id: Option<&'a str>,
}

/// A file name that is safe to join onto the vault directory.
///
/// A name that arrived from a file dialog is untrusted input like any other: it is
/// reduced to its last segment (no `..`, no separators, no drive letters), stripped
/// of characters that are not meaningful in a name, and capped so the stored name
/// cannot outgrow the filesystem's limit once the hash prefix is added.
#[must_use]
pub fn sanitise_document_name(file_name: &str) -> String {
    let last = file_name.rsplit(['\\', '/']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\u{7f}')
        .map(|ch| if "<>:\"|?*".contains(ch) { '-' } else { ch })
        .collect();
    // Leading dots would make the stored file hidden; a trailing dot or space is not
    // a legal end to a name on Windows.
    let cleaned = cleaned.trim_start_matches('.');
    let cleaned = cleaned.trim_end_matches(|ch: char| ch == '.' || ch.is_whitespace());
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return "documento".to_owned();
    }
    // The reference keeps the LAST 120 characters, because the end of a name is the
    // part that identifies the document.
    let characters: Vec<char> = cleaned.chars().collect();
    if characters.len() > 120 {
        return characters[characters.len() - 120..].iter().collect();
    }
    cleaned.to_owned()
}

/// The stored name for a hash and a file name.
#[must_use]
pub fn stored_name(sha256: &str, file_name: &str) -> String {
    let prefix: String = sha256.chars().take(12).collect();
    format!("{prefix}-{file_name}")
}

/// The last segment of a path, the way both implementations take a base name.
#[must_use]
pub fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "documento".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_reduced_to_its_last_segment() {
        assert_eq!(sanitise_document_name("C:\\Users\\eu\\fatura.pdf"), "fatura.pdf");
        assert_eq!(sanitise_document_name("/home/eu/fatura.pdf"), "fatura.pdf");
        assert_eq!(sanitise_document_name("../../fatura.pdf"), "fatura.pdf");
    }

    #[test]
    fn characters_a_filesystem_will_not_take_are_replaced() {
        assert_eq!(
            sanitise_document_name("a<b>c:d\"e|f?g*h.pdf"),
            "a-b-c-d-e-f-g-h.pdf"
        );
        assert_eq!(sanitise_document_name("fatura\u{0}\u{1}.pdf"), "fatura.pdf");
    }

    #[test]
    fn leading_dots_and_trailing_dots_or_spaces_are_removed() {
        assert_eq!(sanitise_document_name("...fatura.pdf"), "fatura.pdf");
        assert_eq!(sanitise_document_name("fatura.pdf..."), "fatura.pdf");
        assert_eq!(sanitise_document_name("fatura.pdf   "), "fatura.pdf");
    }

    #[test]
    fn an_empty_name_gets_a_name() {
        assert_eq!(sanitise_document_name(""), "documento");
        assert_eq!(sanitise_document_name("..."), "documento");
        assert_eq!(sanitise_document_name("   "), "documento");
    }

    #[test]
    fn a_long_name_keeps_its_end_where_the_identity_lives() {
        let long = format!("{}-fatura-recibo-2026-014.pdf", "x".repeat(200));
        let cleaned = sanitise_document_name(&long);
        assert_eq!(cleaned.chars().count(), 120);
        assert!(cleaned.ends_with("fatura-recibo-2026-014.pdf"));
    }

    #[test]
    fn the_stored_name_is_the_hash_prefix_and_the_name() {
        let sha = "9f86d081884c7d65";
        assert_eq!(stored_name(sha, "fatura.pdf"), "9f86d081884c-fatura.pdf");
    }
}
