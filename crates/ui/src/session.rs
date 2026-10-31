//! Opening the vault, and everything a page needs from it.
//!
//! A session owns three things for the lifetime of the window: the folder (locked, so
//! a second window cannot write to it), the rule pack (compiled in), and the
//! environment it resolved. Nothing here decides anything either; it loads, and it
//! rebuilds the model on demand.

use std::path::PathBuf;

use vnf_core::rules::{PackError, load_embedded};
use vnf_core::types::LoadedPack;
use vnf_store::location::{Env, VaultLocationSource, resolve_vault_location, vault_pointer_path};
use vnf_store::model::{DashboardModel, ModelConfig, ModelError, build_model};
use vnf_store::risk::{RiskLevel, check_data_dir_risk};
use vnf_store::{StoreError, Vault, VaultLock};

/// Why the window could not open a vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// A second window already has this folder.
    Locked(String),
    /// The folder is somewhere this application refuses to keep data.
    Refused(String),
    /// The vault could not be read.
    Store(String),
    /// The rule pack compiled into the binary is broken.
    Pack(String),
}

impl core::fmt::Display for OpenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Locked(message) | Self::Refused(message) | Self::Store(message) => f.write_str(message),
            Self::Pack(message) => write!(f, "o pacote de regras embutido está inválido: {message}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// Everything the window holds open.
pub struct Session {
    vault: Vault,
    pack: LoadedPack,
    env: Env,
    /// Held for the lifetime of the window; dropping it releases the vault.
    _lock: VaultLock,
    version: String,
    runtime: String,
}

impl Session {
    /// Open the vault this machine resolves, and lock it.
    ///
    /// # Errors
    ///
    /// [`OpenError`] when the folder is refused, already held by another window, or
    /// cannot be read.
    pub fn open(version: &str) -> Result<Self, OpenError> {
        let env = Env::from_process();
        let resolved = resolve_vault_location(None, &env);
        let risk = check_data_dir_risk(&resolved.dir);
        if risk.level == RiskLevel::Fatal
            && let Some(message) = risk.message
        {
            return Err(OpenError::Refused(message));
        }
        Self::open_in(&resolved.dir, &env, version)
    }

    /// Open a named folder, which is what a test does.
    ///
    /// # Errors
    ///
    /// [`OpenError`] when the folder cannot be created, is already held by another
    /// window, or cannot be read.
    pub fn open_in(dir: &std::path::Path, env: &Env, version: &str) -> Result<Self, OpenError> {
        std::fs::create_dir_all(dir)
            .map_err(|error| OpenError::Store(format!("{}: {error}", dir.display())))?;
        let lock = VaultLock::acquire(dir).map_err(|error| OpenError::Locked(error.to_string()))?;
        let vault = Vault::new(dir);
        vault
            .ensure()
            .map_err(|error| OpenError::Store(error.to_string()))?;
        // A failed audit line is not a reason to refuse to open: the vault is still
        // readable, and the person can see the window.
        let _ = vault.append_audit("vault.opened", Some("janela"), &vnf_store::clock::now_iso8601());
        let pack = load_embedded().map_err(|error: PackError| OpenError::Pack(error.to_string()))?;
        Ok(Self {
            vault,
            pack,
            env: env.clone(),
            _lock: lock,
            version: version.to_owned(),
            runtime: runtime_line(),
        })
    }

    /// The vault.
    #[must_use]
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// The rule pack in use.
    #[must_use]
    pub fn pack(&self) -> &LoadedPack {
        &self.pack
    }

    /// Where the folder came from.
    #[must_use]
    pub fn data_dir_source(&self) -> VaultLocationSource {
        resolve_vault_location(None, &self.env).source
    }

    /// The pointer file that remembers a chosen folder.
    #[must_use]
    pub fn pointer_path(&self) -> PathBuf {
        vault_pointer_path(&self.env)
    }

    /// The application's version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Rebuild the model for a date and year.
    ///
    /// # Errors
    ///
    /// [`ModelError`] when the vault cannot be read or a stored date is malformed.
    pub fn reload(&self, today: &str, year: i64) -> Result<DashboardModel, ModelError> {
        build_model(
            &self.vault,
            &ModelConfig {
                year,
                today: today.to_owned(),
                version: self.version.clone(),
                runtime: self.runtime.clone(),
                pack: &self.pack,
                data_dir_source: self.data_dir_source(),
                pointer_path: self.pointer_path(),
            },
        )
    }
}

/// The line the Diagnóstico page shows in place of the reference's Node version.
fn runtime_line() -> String {
    format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Turn a store error into the message a page shows.
#[must_use]
pub fn describe(error: &StoreError) -> String {
    error.to_string()
}
