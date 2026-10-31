//! The vault itself: one folder, a few files, and the rules for writing them.
//!
//! Every write takes the timestamp it should record. The reference read the clock
//! inside the store, which meant a test could not assert what it wrote without
//! waiting for time to pass; here the caller supplies `at`, so the bytes are
//! reproducible.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use vnf_core::estimate::Invoice;
use vnf_core::types::TaxProfile;

use crate::atomic::{
    append_line, append_typed, read_json, read_jsonl, write_bytes_atomic, write_json_atomic, write_typed,
};
use crate::documents::{DocumentEntry, DocumentMeta, base_name, sanitise_document_name, stored_name};
use crate::location::{
    AUDIT_LOG, DOCUMENT_INDEX, DOCUMENTS_DIR, LEDGER_INVOICES, OBLIGATION_COMPLETIONS, PROFILE_FILE, absolute,
};
use crate::{StoreError, io_error};

/// One line of the audit log.
///
/// `detail` is omitted rather than written as `null`, because that is what the
/// reference wrote and the files are read by both implementations during the port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// What happened, e.g. `invoice.recorded`.
    pub action: String,
    /// Anything worth recording beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// When it happened.
    pub at: String,
}

/// One line of the completions ledger.
///
/// Field order matches the reference's, so the two implementations write the same
/// bytes for the same completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRecord {
    /// The instance id, `<ruleId>@<dueDate>`.
    pub id: String,
    /// The rule it came from.
    pub rule_id: String,
    /// The date it fell due.
    pub due_date: String,
    /// Anything the person wrote about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When it was marked handled.
    pub at: String,
}

/// What a document archive operation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRef {
    /// The stored name, `documents/<hash>-<name>`.
    pub file: String,
    /// The content hash.
    pub sha256: String,
}

/// An indexed document, found by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundDocument {
    /// The name the index uses.
    pub file: String,
    /// Its content hash.
    pub sha256: String,
    /// Where it actually is on disk.
    pub absolute: PathBuf,
}

/// An indexed document, found by content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundByHash {
    /// The name the index uses.
    pub file: String,
    /// The content hash.
    pub sha256: String,
    /// The invoice it was recorded as, when it was.
    pub invoice_id: Option<String>,
}

/// How much is in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStats {
    /// The folder.
    pub dir: String,
    /// How many files, including hidden ones and a leftover temporary file.
    pub files: usize,
    /// Their total size.
    pub bytes: u64,
}

/// The local vault: one directory, and the ledger files inside it.
#[derive(Debug)]
pub struct Vault {
    dir: PathBuf,
    stats: Mutex<Option<VaultStats>>,
}

impl Vault {
    /// Open the vault at a directory. Nothing is created until [`Vault::ensure`].
    #[must_use]
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: absolute(dir.as_ref()),
            stats: Mutex::new(None),
        }
    }

    /// The folder.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A path inside the vault.
    #[must_use]
    pub fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// Create the folders the vault needs.
    ///
    /// Deliberately fewer than the reference made: `ai/` belonged to the assistant,
    /// which this build does not have, and `exports/` was created by nothing and
    /// written by nothing. A folder that exists to be empty is a promise the
    /// application does not keep.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when a folder cannot be created.
    pub fn ensure(&self) -> Result<(), StoreError> {
        for relative in ["ledger", "obligations", "audit", DOCUMENTS_DIR] {
            let path = self.path(relative);
            std::fs::create_dir_all(&path).map_err(|error| io_error(&path, &error))?;
        }
        Ok(())
    }

    /// Whether a file or folder exists inside the vault.
    #[must_use]
    pub fn exists(&self, relative: &str) -> bool {
        self.path(relative).exists()
    }

    /// Read a JSON file, or `None` when it is not there.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotJson`] when it exists and is not JSON.
    pub fn read_json(&self, relative: &str) -> Result<Option<Value>, StoreError> {
        read_json(&self.path(relative))
    }

    /// Replace a JSON file atomically.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when it cannot be written.
    pub fn write_json(&self, relative: &str, value: &Value) -> Result<(), StoreError> {
        self.invalidate();
        write_json_atomic(&self.path(relative), value)
    }

    /// Append one record to a JSONL file and flush it.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when it cannot be written.
    pub fn append_jsonl(&self, relative: &str, value: &Value) -> Result<(), StoreError> {
        self.invalidate();
        append_line(&self.path(relative), value)
    }

    /// Read a JSONL file.
    ///
    /// # Errors
    ///
    /// [`StoreError::Corrupt`] naming the line that is not JSON.
    pub fn read_jsonl(&self, relative: &str) -> Result<Vec<Value>, StoreError> {
        read_jsonl(&self.path(relative))
    }

    /// The stored profile, when there is one.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotJson`] when `profile.json` is not a profile.
    pub fn load_profile(&self) -> Result<Option<TaxProfile>, StoreError> {
        let Some(value) = self.read_json(PROFILE_FILE)? else {
            return Ok(None);
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(|error| StoreError::NotJson {
                path: self.path(PROFILE_FILE).display().to_string(),
                message: error.to_string(),
            })
    }

    /// Store the profile.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when it cannot be written.
    pub fn save_profile(&self, profile: &TaxProfile) -> Result<(), StoreError> {
        self.invalidate();
        write_typed(&self.path(PROFILE_FILE), profile)
    }

    /// The invoice ledger, in the order it was written.
    ///
    /// # Errors
    ///
    /// [`StoreError::Corrupt`] naming a line that is not an invoice.
    pub fn load_invoices(&self) -> Result<Vec<Invoice>, StoreError> {
        let mut invoices = Vec::new();
        for value in self.read_jsonl(LEDGER_INVOICES)? {
            match serde_json::from_value::<Invoice>(value) {
                Ok(invoice) => invoices.push(invoice),
                Err(error) => {
                    return Err(StoreError::Corrupt {
                        path: LEDGER_INVOICES.to_owned(),
                        line: invoices.len() + 1,
                        message: error.to_string(),
                    });
                }
            }
        }
        Ok(invoices)
    }

    /// Append an invoice and record it in the audit log.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when either write fails.
    pub fn append_invoice(&self, invoice: &Invoice, at: &str) -> Result<(), StoreError> {
        self.invalidate();
        append_typed(&self.path(LEDGER_INVOICES), invoice)?;
        self.append_audit("invoice.recorded", Some(&invoice.number), at)
    }

    /// The obligations the person has marked as handled.
    ///
    /// # Errors
    ///
    /// [`StoreError::Corrupt`] naming a line that is not a completion.
    pub fn completions(&self) -> Result<Vec<CompletionRecord>, StoreError> {
        let mut records = Vec::new();
        for value in self.read_jsonl(OBLIGATION_COMPLETIONS)? {
            match serde_json::from_value::<CompletionRecord>(value) {
                Ok(record) => records.push(record),
                Err(error) => {
                    return Err(StoreError::Corrupt {
                        path: OBLIGATION_COMPLETIONS.to_owned(),
                        line: records.len() + 1,
                        message: error.to_string(),
                    });
                }
            }
        }
        Ok(records)
    }

    /// The ids of the obligations already handled.
    ///
    /// # Errors
    ///
    /// Whatever [`Vault::completions`] refuses.
    pub fn completed_ids(&self) -> Result<Vec<String>, StoreError> {
        Ok(self.completions()?.into_iter().map(|record| record.id).collect())
    }

    /// Mark an obligation handled.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when either write fails.
    pub fn mark_completed(
        &self,
        id: &str,
        rule_id: &str,
        due_date: &str,
        note: Option<&str>,
        at: &str,
    ) -> Result<(), StoreError> {
        let record = CompletionRecord {
            id: id.to_owned(),
            rule_id: rule_id.to_owned(),
            due_date: due_date.to_owned(),
            note: note.map(str::to_owned),
            at: at.to_owned(),
        };
        self.invalidate();
        append_typed(&self.path(OBLIGATION_COMPLETIONS), &record)?;
        self.append_audit("obligation.completed", Some(id), at)
    }

    /// Obligation ids that already have at least one document archived.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotJson`] when the index is not a list.
    pub fn documented_obligation_ids(&self) -> Result<Vec<String>, StoreError> {
        let mut ids: Vec<String> = Vec::new();
        for entry in self.document_index()? {
            if let Some(id) = entry.obligation_id
                && !id.is_empty()
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// Write one audit line.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when it cannot be written.
    pub fn append_audit(&self, action: &str, detail: Option<&str>, at: &str) -> Result<(), StoreError> {
        let event = AuditEvent {
            action: action.to_owned(),
            detail: detail.map(str::to_owned),
            at: at.to_owned(),
        };
        self.invalidate();
        append_typed(&self.path(AUDIT_LOG), &event)
    }

    /// The audit log.
    ///
    /// # Errors
    ///
    /// [`StoreError::Corrupt`] naming a line that is not an audit event.
    pub fn audit(&self) -> Result<Vec<AuditEvent>, StoreError> {
        let mut events = Vec::new();
        for value in self.read_jsonl(AUDIT_LOG)? {
            match serde_json::from_value::<AuditEvent>(value) {
                Ok(event) => events.push(event),
                Err(error) => {
                    return Err(StoreError::Corrupt {
                        path: AUDIT_LOG.to_owned(),
                        line: events.len() + 1,
                        message: error.to_string(),
                    });
                }
            }
        }
        Ok(events)
    }

    // -----------------------------------------------------------------------
    // Documents
    // -----------------------------------------------------------------------

    /// Archive a file: copy it into the vault under its hash, then index it.
    ///
    /// The copy happens **before** the index entry, so the index can never point at
    /// a file that does not exist, and the original is never moved or modified.
    ///
    /// # Errors
    ///
    /// [`StoreError::MissingFile`] when the path is not there, and
    /// [`StoreError::Io`] when the copy or the index write fails.
    pub fn add_document(
        &self,
        absolute_path: &Path,
        meta: DocumentMeta<'_>,
        at: &str,
    ) -> Result<DocumentRef, StoreError> {
        if !absolute_path.is_file() {
            return Err(StoreError::MissingFile {
                path: absolute_path.display().to_string(),
            });
        }
        self.ensure()?;
        let sha256 = self.hash_file(absolute_path)?;
        // The reference takes the base name EXACTLY as the path spells it; only the
        // byte path is sanitised, because only that name arrived from elsewhere.
        let name = base_name(absolute_path);
        let stored = stored_name(&sha256, &name);
        let destination = self.path(DOCUMENTS_DIR).join(&stored);
        std::fs::copy(absolute_path, &destination).map_err(|error| io_error(&destination, &error))?;
        self.invalidate();
        self.index_document(&stored, &sha256, meta, Some(absolute_path), at)
    }

    /// Archive bytes that arrived from a file dialog.
    ///
    /// There is no `originalPath`, because there is no original on this filesystem
    /// to point at.
    ///
    /// # Errors
    ///
    /// [`StoreError::EmptyUpload`] for no bytes, and [`StoreError::Io`] when the file
    /// or the index cannot be written.
    pub fn add_document_bytes(
        &self,
        bytes: &[u8],
        file_name: &str,
        meta: DocumentMeta<'_>,
        at: &str,
    ) -> Result<DocumentRef, StoreError> {
        if bytes.is_empty() {
            return Err(StoreError::EmptyUpload);
        }
        self.ensure()?;
        let sha256 = self.hash_bytes(bytes);
        let stored = stored_name(&sha256, &sanitise_document_name(file_name));
        let destination = self.path(DOCUMENTS_DIR).join(&stored);
        write_bytes_atomic(&destination, bytes)?;
        self.invalidate();
        self.index_document(&stored, &sha256, meta, None, at)
    }

    /// Add one entry to the index, after the bytes are already in place.
    fn index_document(
        &self,
        stored: &str,
        sha256: &str,
        meta: DocumentMeta<'_>,
        original: Option<&Path>,
        at: &str,
    ) -> Result<DocumentRef, StoreError> {
        let mut index = self.index_values()?;
        let entry = DocumentEntry {
            file: format!("{DOCUMENTS_DIR}/{stored}"),
            original_path: original.map(|path| path.display().to_string()),
            sha256: sha256.to_owned(),
            added_at: at.to_owned(),
            kind: meta.kind.unwrap_or("outro").to_owned(),
            obligation_id: meta.obligation_id.map(str::to_owned),
            invoice_id: meta.invoice_id.map(str::to_owned),
        };
        let value = serde_json::to_value(&entry).map_err(|error| StoreError::Io {
            path: DOCUMENT_INDEX.to_owned(),
            message: error.to_string(),
        })?;
        index.push(value);
        self.write_json(DOCUMENT_INDEX, &Value::Array(index))?;
        self.append_audit("document.added", Some(stored), at)?;
        Ok(DocumentRef {
            file: entry.file,
            sha256: sha256.to_owned(),
        })
    }

    /// Point an archived document at the ledger entry it became.
    ///
    /// Returns `false` when the file is not in the index, so a caller can tell "I
    /// linked it" from "there was nothing to link".
    ///
    /// # Errors
    ///
    /// [`StoreError::NotJson`] when the index is not a list.
    pub fn link_document_to_invoice(
        &self,
        file: &str,
        invoice_id: &str,
        at: &str,
    ) -> Result<bool, StoreError> {
        let mut index = self.index_values()?;
        let Some(entry) = index
            .iter_mut()
            .find(|entry| entry.get("file").and_then(Value::as_str) == Some(file))
        else {
            return Ok(false);
        };
        let Some(object) = entry.as_object_mut() else {
            return Ok(false);
        };
        object.insert("invoiceId".to_owned(), Value::String(invoice_id.to_owned()));
        self.write_json(DOCUMENT_INDEX, &Value::Array(index))?;
        self.append_audit("document.linked", Some(&format!("{file} → {invoice_id}")), at)?;
        Ok(true)
    }

    /// An indexed document, by the name the index uses.
    ///
    /// The stored name is taken **from the index**, never from the caller, so a name
    /// that arrived over the wire cannot escape the documents folder.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotJson`] when the index is not a list.
    pub fn find_document(&self, file: &str) -> Result<Option<FoundDocument>, StoreError> {
        for entry in self.index_values()? {
            if entry.get("file").and_then(Value::as_str) != Some(file) {
                continue;
            }
            let base = file.rsplit(['\\', '/']).next().unwrap_or_default();
            if base.is_empty() {
                return Ok(None);
            }
            return Ok(Some(FoundDocument {
                file: file.to_owned(),
                sha256: entry
                    .get("sha256")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                absolute: self.path(DOCUMENTS_DIR).join(base),
            }));
        }
        Ok(None)
    }

    /// The indexed document with this content hash, if it is already archived.
    ///
    /// Its purpose is the duplicate: the same PDF handed over twice is the same
    /// document, and filing it twice would append a second invoice for one piece of
    /// income. Content addressing is what makes the question answerable at all.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotJson`] when the index is not a list.
    pub fn find_document_by_hash(&self, sha256: &str) -> Result<Option<FoundByHash>, StoreError> {
        for entry in self.index_values()? {
            if entry.get("sha256").and_then(Value::as_str) != Some(sha256) {
                continue;
            }
            return Ok(Some(FoundByHash {
                file: entry
                    .get("file")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                sha256: sha256.to_owned(),
                invoice_id: entry.get("invoiceId").and_then(Value::as_str).map(str::to_owned),
            }));
        }
        Ok(None)
    }

    /// The whole index, read leniently.
    ///
    /// An entry that cannot be understood is skipped rather than hidden: one
    /// hand-edited line must not make the other documents disappear.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotJson`] when the index file is not a list.
    pub fn document_index(&self) -> Result<Vec<DocumentEntry>, StoreError> {
        Ok(self.index_values()?.iter().filter_map(entry_from_value).collect())
    }

    /// The index raw, so adding an entry cannot drop a field this build does not know.
    fn index_values(&self) -> Result<Vec<Value>, StoreError> {
        match self.read_json(DOCUMENT_INDEX)? {
            None => Ok(Vec::new()),
            Some(Value::Array(entries)) => Ok(entries),
            Some(other) => Err(StoreError::NotJson {
                path: self.path(DOCUMENT_INDEX).display().to_string(),
                message: format!("expected a list, found {other}"),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Hashing and size
    // -----------------------------------------------------------------------

    /// Content hash, so a document in the vault can be proven unchanged.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when the file cannot be read.
    pub fn hash_file(&self, path: &Path) -> Result<String, StoreError> {
        let bytes = std::fs::read(path).map_err(|error| io_error(path, &error))?;
        Ok(self.hash_bytes(&bytes))
    }

    /// The same hash for bytes that were just read.
    #[must_use]
    pub fn hash_bytes(&self, bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    /// How many files the vault holds and how big they are.
    ///
    /// Cached: the reference walked every file on every model build.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when a directory cannot be read.
    pub fn describe(&self) -> Result<VaultStats, StoreError> {
        if let Ok(guard) = self.stats.lock()
            && let Some(stats) = guard.as_ref()
        {
            return Ok(stats.clone());
        }
        let mut files = 0_usize;
        let mut bytes = 0_u64;
        walk(&self.dir, &mut files, &mut bytes)?;
        let stats = VaultStats {
            dir: self.dir.display().to_string(),
            files,
            bytes,
        };
        if let Ok(mut guard) = self.stats.lock() {
            *guard = Some(stats.clone());
        }
        Ok(stats)
    }

    /// Forget the cached size, because something was written.
    fn invalidate(&self) {
        if let Ok(mut guard) = self.stats.lock() {
            *guard = None;
        }
    }
}

/// Lower-case hex of a digest.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Count files and bytes under a directory.
fn walk(directory: &Path, files: &mut usize, bytes: &mut u64) -> Result<(), StoreError> {
    if !directory.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(directory).map_err(|error| io_error(directory, &error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error(directory, &error))?;
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, files, bytes)?;
        } else {
            *files += 1;
            match entry.metadata() {
                Ok(metadata) => *bytes += metadata.len(),
                Err(error) => return Err(io_error(&path, &error)),
            }
        }
    }
    Ok(())
}

/// Read one index entry leniently, taking the fields a document needs.
fn entry_from_value(value: &Value) -> Option<DocumentEntry> {
    let object = value.as_object()?;
    let text = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(DocumentEntry {
        file: text("file")?,
        original_path: text("originalPath"),
        sha256: text("sha256").unwrap_or_default(),
        added_at: text("addedAt").unwrap_or_default(),
        kind: text("kind").unwrap_or_else(|| "outro".to_owned()),
        obligation_id: text("obligationId"),
        invoice_id: text("invoiceId"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnf_core::estimate::{InvoiceStatus, VatTreatment};

    type Problem = Box<dyn std::error::Error>;

    const AT: &str = "2026-09-20T10:00:00.000Z";

    fn profile() -> Result<TaxProfile, Problem> {
        Ok(vnf_core::profile::create_default_profile(
            &vnf_core::profile::ProfileInput {
                nif: "123456789".to_owned(),
                name: "Pessoa Exemplo".to_owned(),
                ..Default::default()
            },
        )?)
    }

    fn invoice(number: &str) -> Invoice {
        Invoice {
            id: format!("inv_{number}"),
            number: number.to_owned(),
            date: "2026-04-12".to_owned(),
            client_name: "Cliente Exemplo".to_owned(),
            client_nif: Some("501234560".to_owned()),
            client_country: "PT".to_owned(),
            description: "Consultoria".to_owned(),
            base_cents: 100_000,
            iva_rate_bp: 2300,
            vat_treatment: VatTreatment::IvaPt,
            retention_bp: 2300,
            atcud: None,
            status: InvoiceStatus::Issued,
            payment_proof_in_vault: false,
        }
    }

    #[test]
    fn a_profile_survives_a_round_trip() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        assert_eq!(vault.load_profile()?, None);
        let profile = profile()?;
        vault.save_profile(&profile)?;
        assert_eq!(vault.load_profile()?, Some(profile));
        Ok(())
    }

    #[test]
    fn an_invoice_is_appended_with_an_audit_line() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        vault.append_invoice(&invoice("FR 1"), AT)?;
        vault.append_invoice(&invoice("FR 2"), AT)?;
        let invoices = vault.load_invoices()?;
        assert_eq!(invoices.len(), 2);
        assert_eq!(invoices[1].number, "FR 2");
        let audit = vault.audit()?;
        assert_eq!(audit.len(), 2);
        assert_eq!(audit[0].action, "invoice.recorded");
        assert_eq!(audit[0].detail.as_deref(), Some("FR 1"));
        assert_eq!(audit[0].at, AT);
        // The ledger file is one JSON object per line, which is what the reference
        // reads and what a person can grep.
        let text = std::fs::read_to_string(vault.path(LEDGER_INVOICES))?;
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().next().unwrap_or_default().starts_with('{'));
        Ok(())
    }

    #[test]
    fn a_completion_is_recorded_and_counted() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        assert!(vault.completed_ids()?.is_empty());
        vault.mark_completed(
            "iva.dp.trimestral@2026-05-20",
            "iva.dp.trimestral",
            "2026-05-20",
            Some("submetida"),
            AT,
        )?;
        assert_eq!(
            vault.completed_ids()?,
            vec!["iva.dp.trimestral@2026-05-20".to_owned()]
        );
        let completions = vault.completions()?;
        assert_eq!(completions[0].note.as_deref(), Some("submetida"));
        Ok(())
    }

    #[test]
    fn a_document_is_copied_hashed_and_indexed() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("fatura-recibo.pdf");
        std::fs::write(&source, b"%PDF-1.7 exemplar")?;
        let vault = Vault::new(dir.path().join("vault"));
        let archived = vault.add_document(&source, DocumentMeta::default(), AT)?;
        assert!(archived.file.starts_with("documents/"));
        assert_eq!(archived.sha256.len(), 64);
        assert!(archived.file.contains(&archived.sha256[..12]));
        // The original is untouched and the copy is byte-identical.
        assert_eq!(std::fs::read(&source)?, b"%PDF-1.7 exemplar");
        let stored = vault.path(&archived.file);
        assert_eq!(std::fs::read(&stored)?, b"%PDF-1.7 exemplar");
        // And it is in the index, with an audit line behind it.
        let index = vault.document_index()?;
        assert_eq!(index.len(), 1);
        assert_eq!(
            index[0].original_path.as_deref(),
            Some(source.display().to_string().as_str())
        );
        assert_eq!(index[0].kind, "outro");
        assert!(
            vault
                .audit()?
                .iter()
                .any(|event| event.action == "document.added")
        );
        Ok(())
    }

    #[test]
    fn bytes_are_archived_without_an_original_path() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let archived =
            vault.add_document_bytes(b"conteudo", "C:\\Windows\\evil..pdf", DocumentMeta::default(), AT)?;
        assert!(archived.file.starts_with("documents/"));
        assert!(
            archived.file.ends_with("-evil..pdf"),
            "the name is sanitised, not trusted: {}",
            archived.file
        );
        assert_eq!(vault.document_index()?[0].original_path, None);
        assert_eq!(
            vault.add_document_bytes(&[], "empty.pdf", DocumentMeta::default(), AT),
            Err(StoreError::EmptyUpload)
        );
        Ok(())
    }

    #[test]
    fn the_same_bytes_twice_are_the_same_document() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let first = vault.add_document_bytes(b"mesma coisa", "a.pdf", DocumentMeta::default(), AT)?;
        let second = vault.add_document_bytes(b"mesma coisa", "b.pdf", DocumentMeta::default(), AT)?;
        assert_eq!(first.sha256, second.sha256);
        // Content addressing is what answers "was this already filed?".
        let found = vault
            .find_document_by_hash(&first.sha256)?
            .unwrap_or_else(|| FoundByHash {
                file: String::new(),
                sha256: String::new(),
                invoice_id: None,
            });
        assert_eq!(found.file, first.file);
        assert_eq!(found.invoice_id, None);
        Ok(())
    }

    #[test]
    fn a_document_can_be_linked_to_the_invoice_it_became() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let archived = vault.add_document_bytes(b"pdf", "fatura.pdf", DocumentMeta::default(), AT)?;
        assert!(vault.link_document_to_invoice(&archived.file, "inv_1", AT)?);
        assert!(!vault.link_document_to_invoice("documents/nada", "inv_1", AT)?);
        let found = vault.find_document_by_hash(&archived.sha256)?;
        assert_eq!(found.and_then(|found| found.invoice_id), Some("inv_1".to_owned()));
        assert!(
            vault
                .audit()?
                .iter()
                .any(|event| event.action == "document.linked")
        );
        Ok(())
    }

    #[test]
    fn a_document_is_found_by_the_name_the_index_holds() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let archived = vault.add_document_bytes(b"pdf", "fatura.pdf", DocumentMeta::default(), AT)?;
        let found = vault
            .find_document(&archived.file)?
            .unwrap_or_else(|| FoundDocument {
                file: String::new(),
                sha256: String::new(),
                absolute: PathBuf::new(),
            });
        assert_eq!(found.sha256, archived.sha256);
        assert!(found.absolute.starts_with(vault.path(DOCUMENTS_DIR)));
        assert_eq!(vault.find_document("documents/nao-existe.pdf")?, None);
        Ok(())
    }

    #[test]
    fn documents_are_counted_by_obligation_not_by_occurrence() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        vault.add_document_bytes(
            b"um",
            "a.pdf",
            DocumentMeta {
                obligation_id: Some("iva.dp.trimestral"),
                ..Default::default()
            },
            AT,
        )?;
        vault.add_document_bytes(
            b"dois",
            "b.pdf",
            DocumentMeta {
                obligation_id: Some("iva.dp.trimestral"),
                ..Default::default()
            },
            AT,
        )?;
        vault.add_document_bytes(b"tres", "c.pdf", DocumentMeta::default(), AT)?;
        assert_eq!(
            vault.documented_obligation_ids()?,
            vec!["iva.dp.trimestral".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn the_size_walk_counts_what_is_in_the_folder() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        vault.append_invoice(&invoice("FR 1"), AT)?;
        let first = vault.describe()?;
        assert!(first.files >= 2, "the ledger and the audit log");
        assert!(first.bytes > 0);
        // A write invalidates the cache, so the count moves.
        vault.append_invoice(&invoice("FR 2"), AT)?;
        let second = vault.describe()?;
        assert!(second.files >= first.files);
        assert!(second.bytes > first.bytes);
        Ok(())
    }

    #[test]
    fn an_unknown_index_field_survives_a_new_entry() -> Result<(), Problem> {
        // The index is rewritten whole, so a field this build does not know must not
        // be dropped by the rewrite.
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        std::fs::create_dir_all(vault.path(DOCUMENTS_DIR))?;
        vault.write_json(
            DOCUMENT_INDEX,
            &serde_json::json!([{ "file": "documents/old", "sha256": "x", "addedAt": "a", "kind": "outro", "originalPath": null, "obligationId": null, "invoiceId": null, "extra": "kept" }]),
        )?;
        vault.add_document_bytes(b"novo", "novo.pdf", DocumentMeta::default(), AT)?;
        let raw = vault
            .read_json(DOCUMENT_INDEX)?
            .unwrap_or(serde_json::Value::Null);
        let entries = raw.as_array().cloned().unwrap_or_default();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("extra").and_then(Value::as_str), Some("kept"));
        Ok(())
    }

    #[test]
    fn ensure_creates_only_folders_that_are_used() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        vault.ensure()?;
        for present in ["ledger", "obligations", "audit", DOCUMENTS_DIR] {
            assert!(vault.path(present).is_dir(), "{present} must exist");
        }
        assert!(!vault.path("ai").exists(), "the assistant is not in this build");
        assert!(!vault.path("exports").exists(), "nothing writes there");
        Ok(())
    }
}
