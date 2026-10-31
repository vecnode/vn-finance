//! One writer at a time.
//!
//! The reference had no lock at all, and its document index is a whole-file
//! read-modify-rewrite: two processes each read the index, each add their entry, each
//! write it back, and the slower writer silently drops the other's document. A tax
//! record that loses a filing without saying so is the worst failure this application
//! can have.
//!
//! The lock is advisory and lives on `<vault>/.lock`. It is held for the lifetime of
//! the window and released by the operating system when the process ends — a lock
//! file that a crash could leave behind would block the person from their own data,
//! which is a worse bug than the one it fixes.

use std::fs::File;
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use crate::{StoreError, io_error};

/// An exclusive hold on a vault directory.
///
/// Dropping it releases the lock. Keep it alive for as long as the vault is open.
#[derive(Debug)]
pub struct VaultLock {
    file: File,
    path: PathBuf,
}

impl VaultLock {
    /// Take the vault's lock, without waiting.
    ///
    /// # Errors
    ///
    /// [`StoreError::Locked`] when another process holds it, and [`StoreError::Io`]
    /// when the lock file cannot be created.
    pub fn acquire(vault_dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(vault_dir).map_err(|error| io_error(vault_dir, &error))?;
        let path = vault_dir.join(".lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| io_error(&path, &error))?;
        match file.try_lock_exclusive() {
            Ok(true) => Ok(Self { file, path }),
            Ok(false) => Err(StoreError::Locked {
                path: path.display().to_string(),
                message: "outra janela do vn-finance já tem este cofre aberto.".to_owned(),
            }),
            Err(error) => Err(io_error(&path, &error)),
        }
    }

    /// The lock file's path, for a diagnostic line.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for VaultLock {
    fn drop(&mut self) {
        // `unlock` takes `&self`; the file is closed when the struct drops, which
        // also releases the lock, but being explicit means the lock is gone even if
        // the file is kept alive by the operating system's handle table.
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Problem = Box<dyn std::error::Error>;

    #[test]
    fn a_second_writer_is_refused_while_the_first_holds_it() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let first = VaultLock::acquire(dir.path())?;
        assert!(first.path().ends_with(".lock"));
        // The same process asking twice is still a second writer.
        let second = VaultLock::acquire(dir.path());
        assert!(matches!(second, Err(StoreError::Locked { .. })), "got {second:?}");
        drop(first);
        // And once it is released, the vault is available again.
        assert!(VaultLock::acquire(dir.path()).is_ok());
        Ok(())
    }
}
