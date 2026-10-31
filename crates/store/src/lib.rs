//! The local vault.
//!
//! Everything lives in one directory the person owns: the profile, the invoice
//! ledger, the document index, the obligation completions and the audit log. No
//! server, no sync, no telemetry. Backup is copying a folder, deletion is deleting
//! a folder, and both statements are true only because nothing here writes
//! anywhere else.
//!
//! The ledger is append-only JSONL rather than a mutable table. For a tax record,
//! "what did I record, and when" is as important as "what is the current value",
//! and an append-only file answers both without a database engine.
//!
//! This crate is the only place in the workspace that touches the filesystem. Every
//! function that computes something still takes its inputs as parameters; the one
//! exception is [`clock::now_iso8601`], which exists because an audit line without a
//! timestamp is not a record — and it is pinned by a test that formats a fixed instant
//! rather than racing the wall clock.
//!
//! ## Four things this implementation does that the reference did not
//!
//! 1. **Atomic writes are actually atomic.** A unique temporary name, `sync_all`,
//!    the rename, and a directory sync. The reference shared one `<file>.tmp`, so
//!    two writers could rename each other's bytes into place.
//! 2. **Appends are synced.** `sync_data` after each line, so a power cut does not
//!    lose the invoice the person just recorded.
//! 3. **One writer at a time.** An advisory lock on the vault directory, held for
//!    the window's lifetime. The reference had none, and its document index is a
//!    whole-file read-modify-rewrite, so two processes silently lost entries.
//! 4. **The size walk is cached.** `describe` walked every file on every model
//!    build; it now runs once and is invalidated by a write.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![allow(unreachable_pub)]

pub mod atomic;
pub mod clock;
pub mod documents;
pub mod location;
pub mod lock;
pub mod model;
pub mod risk;
pub mod vault;

pub use documents::{DocumentEntry, sanitise_document_name};
pub use location::{
    DirectoryEntry, DirectoryListing, Env, ResolvedVault, VaultLocationSource, VaultPointer,
    browse_directories, default_data_dir, looks_like_vault, read_vault_pointer, resolve_vault_location,
    starting_points, vault_pointer_path, write_vault_pointer,
};
pub use lock::VaultLock;
pub use model::{DashboardModel, ModelConfig, build_model};
pub use risk::{DataDirRisk, RiskLevel, check_data_dir_risk, find_git_root};
pub use vault::{AuditEvent, CompletionRecord, Vault, VaultStats};

use core::fmt;

/// Why the vault could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A file operation failed.
    Io {
        /// The path involved.
        path: String,
        /// What the operating system said.
        message: String,
    },
    /// A JSON file is not JSON.
    NotJson {
        /// The path involved.
        path: String,
        /// What the parser said.
        message: String,
    },
    /// One line of a JSONL file is not JSON.
    Corrupt {
        /// The file, relative to the vault.
        path: String,
        /// The line number, counting from one.
        line: usize,
        /// What the parser said.
        message: String,
    },
    /// A document was named but is not on disk.
    MissingFile {
        /// The absolute path that was tried.
        path: String,
    },
    /// An upload arrived with no bytes.
    EmptyUpload,
    /// Another process holds the vault.
    Locked {
        /// What could not be acquired.
        path: String,
        /// Why.
        message: String,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "{path}: {message}"),
            Self::NotJson { path, message } => write!(f, "{path} não é JSON válido: {message}"),
            Self::Corrupt { path, line, message } => {
                write!(f, "{path}:{line} está corrompido: {message}")
            }
            Self::MissingFile { path } => write!(f, "ficheiro não encontrado: {path}"),
            Self::EmptyUpload => f.write_str("o ficheiro recebido está vazio."),
            Self::Locked { path, message } => {
                write!(f, "o cofre está em uso ({path}): {message}")
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// A `std::io::Error` with the path that caused it.
pub(crate) fn io_error(path: &std::path::Path, error: &std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}
