//! Where the vault is, and why it is there.
//!
//! The order is explicit-before-implicit at every step: a flag on this run wins over
//! the environment, which wins over the remembered choice, which wins over the
//! default. The interface shows which of the four applied, because "your vault is
//! `~/.vn-finance`" and "you chose this folder" are different claims and only one of
//! them is worth reminding someone about.
//!
//! The environment arrives as a value rather than being read from the process, so a
//! test can ask "what would happen on a machine whose HOME is this?" without
//! changing the machine.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::StoreError;
use crate::atomic::{read_json, write_json_atomic};
use crate::risk::{DataDirRisk, check_data_dir_risk};

/// The folder name the application owns in the home directory.
pub const VAULT_FOLDER: &str = ".vn-finance";

/// The file name the ledger uses.
pub const LEDGER_INVOICES: &str = "ledger/invoices.jsonl";
/// The file name the completions use.
pub const OBLIGATION_COMPLETIONS: &str = "obligations/completions.jsonl";
/// The file name the audit log uses.
pub const AUDIT_LOG: &str = "audit/audit.jsonl";
/// The profile's file name.
pub const PROFILE_FILE: &str = "profile.json";
/// The archived-documents folder.
pub const DOCUMENTS_DIR: &str = "documents";
/// The document index's file name.
pub const DOCUMENT_INDEX: &str = "documents/index.json";

/// The environment the location logic reads.
///
/// A value, not a global: the process environment is read once at startup and
/// passed in, so a test states the machine it is pretending to be.
#[derive(Debug, Clone, Default)]
pub struct Env {
    vars: BTreeMap<String, String>,
    windows: bool,
}

impl Env {
    /// The real process environment.
    #[must_use]
    pub fn from_process() -> Self {
        let mut vars = BTreeMap::new();
        for key in [
            "USERPROFILE",
            "HOME",
            "XDG_DATA_HOME",
            "VN_FINANCE_DATA_DIR",
            "VN_FINANCE_ALLOW_IN_REPO",
        ] {
            if let Ok(value) = std::env::var(key) {
                vars.insert(key.to_owned(), value);
            }
        }
        Self {
            vars,
            windows: cfg!(windows),
        }
    }

    /// An environment for a test, with the variables it names.
    #[must_use]
    pub fn for_test(vars: &[(&str, &str)], windows: bool) -> Self {
        Self {
            vars: vars
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            windows,
        }
    }

    /// One variable, when it is set.
    #[must_use]
    pub fn var(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    /// One variable, treating blank as unset — which is how the reference reads them.
    #[must_use]
    pub fn non_blank(&self, key: &str) -> Option<&str> {
        self.var(key).filter(|value| !value.trim().is_empty())
    }

    /// Whether this is pretending to be Windows.
    #[must_use]
    pub fn is_windows(&self) -> bool {
        self.windows
    }

    /// The home directory the platform would report.
    #[must_use]
    pub fn home(&self) -> Option<PathBuf> {
        let key = if self.windows { "USERPROFILE" } else { "HOME" };
        self.non_blank(key).map(PathBuf::from)
    }
}

/// Where the vault folder came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultLocationSource {
    /// A flag on this run.
    Flag,
    /// `VN_FINANCE_DATA_DIR`.
    Env,
    /// The folder remembered in the pointer file.
    Pointer,
    /// The default.
    Default,
}

impl VaultLocationSource {
    /// The phrase the interface uses for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::Pointer => "pointer",
            Self::Default => "default",
        }
    }
}

/// The small file that remembers the folder someone picked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultPointer {
    /// The folder.
    pub path: String,
    /// When it was chosen.
    pub chosen_at: String,
    /// Which interface chose it, for the audit line.
    pub by: String,
}

/// Which folder is the vault, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVault {
    /// The folder.
    pub dir: PathBuf,
    /// Where that choice came from.
    pub source: VaultLocationSource,
    /// The pointer file, whether or not it exists.
    pub pointer_path: PathBuf,
    /// What the pointer said, when it was the thing that decided.
    pub pointer: Option<VaultPointer>,
}

/// Where the vault lives when nobody said otherwise.
///
/// `~/.vn-finance` is a fine default for a developer and a bad one for everybody
/// else: it is a hidden folder in the home directory, which is to say a folder the
/// person who owns the data cannot find. That is why the location is **remembered**
/// once it has been chosen, and why the interface opens on a chooser rather than on
/// a path it invented.
#[must_use]
pub fn default_data_dir(env: &Env) -> PathBuf {
    if env.is_windows() {
        return env.home().unwrap_or_default().join(VAULT_FOLDER);
    }
    let base = match env.non_blank("XDG_DATA_HOME") {
        Some(xdg) => PathBuf::from(xdg),
        None => env.home().unwrap_or_default().join(".local").join("share"),
    };
    base.join("vn-finance")
}

/// The pointer file's path. It lives beside the default vault, never inside a chosen
/// one, because a file that says where the vault is cannot live where you would have
/// to find it first.
#[must_use]
pub fn vault_pointer_path(env: &Env) -> PathBuf {
    default_data_dir(env).join("vault-location.json")
}

/// Read the remembered folder.
///
/// A pointer that cannot be read is **not** a reason to lose access to the data: it
/// is ignored, and resolution falls back to the default, which the interface can then
/// offer to change again.
#[must_use]
pub fn read_vault_pointer(env: &Env) -> Option<VaultPointer> {
    let file = vault_pointer_path(env);
    let value = read_json(&file).ok().flatten()?;
    let path = value
        .get("path")
        .and_then(serde_json::Value::as_str)?
        .trim()
        .to_owned();
    if path.is_empty() {
        return None;
    }
    Some(VaultPointer {
        path,
        chosen_at: value
            .get("chosenAt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        by: value
            .get("by")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("desconhecido")
            .to_owned(),
    })
}

/// Remember the chosen folder.
///
/// # Errors
///
/// [`StoreError::Io`] when the pointer cannot be written.
pub fn write_vault_pointer(dir: &Path, by: &str, at: &str, env: &Env) -> Result<VaultPointer, StoreError> {
    let pointer = VaultPointer {
        path: absolute(dir).display().to_string(),
        chosen_at: at.to_owned(),
        by: by.to_owned(),
    };
    let value = serde_json::to_value(&pointer).unwrap_or(serde_json::Value::Null);
    write_json_atomic(&vault_pointer_path(env), &value)?;
    Ok(pointer)
}

/// Which folder is the vault, and why.
#[must_use]
pub fn resolve_vault_location(explicit: Option<&Path>, env: &Env) -> ResolvedVault {
    let pointer_path = vault_pointer_path(env);
    if let Some(dir) = explicit.filter(|dir| !dir.as_os_str().is_empty()) {
        return ResolvedVault {
            dir: absolute(dir),
            source: VaultLocationSource::Flag,
            pointer_path,
            pointer: None,
        };
    }
    if let Some(from_env) = env.non_blank("VN_FINANCE_DATA_DIR") {
        return ResolvedVault {
            dir: absolute(Path::new(from_env)),
            source: VaultLocationSource::Env,
            pointer_path,
            pointer: None,
        };
    }
    if let Some(pointer) = read_vault_pointer(env) {
        return ResolvedVault {
            dir: absolute(Path::new(&pointer.path)),
            source: VaultLocationSource::Pointer,
            pointer_path,
            pointer: Some(pointer),
        };
    }
    ResolvedVault {
        dir: default_data_dir(env),
        source: VaultLocationSource::Default,
        pointer_path,
        pointer: None,
    }
}

/// The vault path, refusing one inside this application's own repository.
///
/// # Errors
///
/// The multi-line refusal when the folder sits inside the repository the binary was
/// built from, unless `VN_FINANCE_ALLOW_IN_REPO=1`.
pub fn resolve_data_dir(explicit: Option<&Path>, env: &Env) -> Result<PathBuf, StoreError> {
    let dir = resolve_vault_location(explicit, env).dir;
    if env.var("VN_FINANCE_ALLOW_IN_REPO") == Some("1") {
        return Ok(dir);
    }
    let risk = check_data_dir_risk(&dir);
    match (risk.level, risk.message) {
        (crate::risk::RiskLevel::Fatal, Some(message)) => Err(StoreError::Io {
            path: dir.display().to_string(),
            message,
        }),
        _ => Ok(dir),
    }
}

/// Make a path absolute without touching the filesystem's symlinks.
#[must_use]
pub fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `true` when the folder already holds vault data.
#[must_use]
pub fn looks_like_vault(dir: &Path) -> bool {
    dir.join(PROFILE_FILE).exists() || dir.join(DOCUMENTS_DIR).is_dir() || dir.join(LEDGER_INVOICES).exists()
}

// ---------------------------------------------------------------------------
// Choosing a folder, from inside the interface
// ---------------------------------------------------------------------------

/// One folder in the chooser's listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    /// The folder's own name.
    pub name: String,
    /// Its absolute path.
    pub path: String,
    /// Already looks like a vault: it holds a profile, a ledger or archived files.
    pub is_vault: bool,
}

/// A shortcut to somewhere a person would look first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryRoot {
    /// What to call it.
    pub label: String,
    /// Where it is.
    pub path: String,
}

/// What the folder chooser shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    /// The folder being listed.
    pub path: String,
    /// Its parent, or `null` at a filesystem root.
    pub parent: Option<String>,
    /// Whether it is there at all.
    pub exists: bool,
    /// Shortcuts to start from, so the picker does not open on an empty string.
    pub roots: Vec<DirectoryRoot>,
    /// The sub-folders.
    pub entries: Vec<DirectoryEntry>,
    /// Refused here for the same reason the vault refuses it: a repository can be
    /// published.
    pub risk: DataDirRisk,
    /// Why the folder could not be read, in Portuguese.
    pub error: Option<String>,
}

/// The obvious places to put a folder, in the order a person would look for them.
#[must_use]
pub fn starting_points(env: &Env) -> Vec<DirectoryRoot> {
    let home = env.home().unwrap_or_default();
    let candidates = [
        ("Ambiente de trabalho", home.join("Desktop")),
        ("Documentos", home.join("Documents")),
        ("Transferências", home.join("Downloads")),
        ("Pasta pessoal", home.clone()),
    ];
    let mut points: Vec<DirectoryRoot> = candidates
        .into_iter()
        .filter(|(_, path)| path.exists())
        .map(|(label, path)| DirectoryRoot {
            label: label.to_owned(),
            path: path.display().to_string(),
        })
        .collect();

    if env.is_windows() {
        for letter in 'C'..='Z' {
            let root = PathBuf::from(format!("{letter}:\\"));
            if root.exists() {
                points.push(DirectoryRoot {
                    label: format!("Unidade {letter}:"),
                    path: root.display().to_string(),
                });
            }
        }
    } else if Path::new("/").exists() {
        points.push(DirectoryRoot {
            label: "Raiz do sistema".to_owned(),
            path: "/".to_owned(),
        });
    }
    points
}

/// List the **sub-directories** of a path, for the folder chooser.
///
/// Directories only, and never their contents: the chooser exists so somebody can
/// point at a folder, and a listing that also returned file names would hand any
/// window that ever leaked its state a way to enumerate the disk.
#[must_use]
pub fn browse_directories(target: Option<&Path>, env: &Env) -> DirectoryListing {
    let roots = starting_points(env);
    let fallback = roots
        .first()
        .map(|root| PathBuf::from(&root.path))
        .unwrap_or_else(|| default_data_dir(env));
    let path = match target {
        Some(target) if !target.as_os_str().is_empty() => absolute(target),
        _ => fallback,
    };
    let parent = path.parent().map(|parent| parent.display().to_string());
    let mut listing = DirectoryListing {
        path: path.display().to_string(),
        parent,
        exists: path.is_dir(),
        roots,
        entries: Vec::new(),
        risk: check_data_dir_risk(&path),
        error: None,
    };
    if !listing.exists {
        listing.error = Some("Esta pasta não existe. Podes criá-la aqui ou escolher outra.".to_owned());
        return listing;
    }
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let child = path.join(&name);
                listing.entries.push(DirectoryEntry {
                    name,
                    path: child.display().to_string(),
                    is_vault: child.join(PROFILE_FILE).exists() || child.join(DOCUMENTS_DIR).is_dir(),
                });
            }
        }
        Err(error) => {
            listing.error = Some(format!("Não foi possível ler esta pasta: {error}"));
        }
    }
    // The reference uses a Portuguese locale compare; without an ICU in the tree,
    // case-insensitive alphabetical order is the closest honest thing, and it keeps
    // the list stable.
    listing.entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    listing
}

#[cfg(test)]
mod tests {
    use super::*;

    type Problem = Box<dyn std::error::Error>;

    #[test]
    fn the_default_folder_is_the_one_the_reference_documents() {
        let windows = Env::for_test(&[("USERPROFILE", "C:\\Users\\eu")], true);
        assert_eq!(
            default_data_dir(&windows)
                .display()
                .to_string()
                .replace('/', "\\"),
            "C:\\Users\\eu\\.vn-finance"
        );
        let unix = Env::for_test(&[("HOME", "/home/eu")], false);
        assert_eq!(
            default_data_dir(&unix),
            PathBuf::from("/home/eu/.local/share/vn-finance")
        );
        let xdg = Env::for_test(&[("HOME", "/home/eu"), ("XDG_DATA_HOME", "/data")], false);
        assert_eq!(default_data_dir(&xdg), PathBuf::from("/data/vn-finance"));
        // A blank XDG_DATA_HOME is not a directory called "".
        let blank = Env::for_test(&[("HOME", "/home/eu"), ("XDG_DATA_HOME", "  ")], false);
        assert_eq!(
            default_data_dir(&blank),
            PathBuf::from("/home/eu/.local/share/vn-finance")
        );
    }

    #[test]
    fn the_pointer_lives_beside_the_default_vault_not_inside_a_chosen_one() {
        let env = Env::for_test(&[("HOME", "/home/eu")], false);
        assert_eq!(
            vault_pointer_path(&env),
            PathBuf::from("/home/eu/.local/share/vn-finance/vault-location.json")
        );
    }

    #[test]
    fn resolution_is_explicit_before_implicit() {
        let env = Env::for_test(&[("HOME", "/home/eu")], false);
        let explicit = resolve_vault_location(Some(Path::new("/vaults/mine")), &env);
        assert_eq!(explicit.source, VaultLocationSource::Flag);
        assert_eq!(explicit.dir, absolute(Path::new("/vaults/mine")));

        // A blank flag is not a choice.
        let blank = resolve_vault_location(Some(Path::new("")), &env);
        assert_eq!(blank.source, VaultLocationSource::Default);
    }

    #[test]
    fn the_environment_beats_the_remembered_folder() {
        let env = Env::for_test(
            &[("HOME", "/home/eu"), ("VN_FINANCE_DATA_DIR", "/from/env")],
            false,
        );
        let resolved = resolve_vault_location(None, &env);
        assert_eq!(resolved.source, VaultLocationSource::Env);
        assert_eq!(resolved.dir, absolute(Path::new("/from/env")));
    }

    #[test]
    fn a_corrupt_pointer_does_not_lock_the_person_out() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home)?;
        let env = Env::for_test(&[("HOME", &home.display().to_string())], false);
        let pointer = vault_pointer_path(&env);
        if let Some(parent) = pointer.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&pointer, "{ this is not json")?;
        assert_eq!(read_vault_pointer(&env), None);
        assert_eq!(
            resolve_vault_location(None, &env).source,
            VaultLocationSource::Default
        );
        Ok(())
    }

    #[test]
    fn a_written_pointer_is_read_back_and_used() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home)?;
        let env = Env::for_test(&[("HOME", &home.display().to_string())], false);
        let vault = dir.path().join("vault");
        let pointer = write_vault_pointer(&vault, "painel", "2026-09-20T10:00:00.000Z", &env)?;
        assert_eq!(pointer.by, "painel");
        let resolved = resolve_vault_location(None, &env);
        assert_eq!(resolved.source, VaultLocationSource::Pointer);
        assert_eq!(resolved.dir, absolute(&vault));
        assert_eq!(
            resolved.pointer.map(|pointer| pointer.by),
            Some("painel".to_owned())
        );
        Ok(())
    }

    #[test]
    fn a_folder_that_looks_like_a_vault_is_recognised() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        assert!(!looks_like_vault(dir.path()));
        std::fs::write(dir.path().join(PROFILE_FILE), "{}")?;
        assert!(looks_like_vault(dir.path()));
        Ok(())
    }

    #[test]
    fn the_chooser_lists_directories_and_never_files() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        std::fs::create_dir_all(root.join("Documentos"))?;
        std::fs::create_dir_all(root.join("Cofre").join(DOCUMENTS_DIR))?;
        std::fs::create_dir_all(root.join(".escondida"))?;
        std::fs::write(root.join("notas.txt"), "x")?;

        let env = Env::for_test(&[("HOME", &root.display().to_string())], false);
        let listing = browse_directories(Some(&root), &env);
        let names: Vec<&str> = listing.entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Cofre", "Documentos"],
            "no files, no dot-folders, sorted"
        );
        assert!(listing.entries[0].is_vault, "Cofre holds documents/");
        assert!(!listing.entries[1].is_vault);
        assert_eq!(listing.error, None);

        let missing = browse_directories(Some(&root.join("nao-existe")), &env);
        assert!(!missing.exists);
        assert_eq!(
            missing.error.as_deref(),
            Some("Esta pasta não existe. Podes criá-la aqui ou escolher outra.")
        );
        Ok(())
    }
}
