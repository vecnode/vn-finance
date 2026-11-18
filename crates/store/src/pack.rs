//! Finding the rule pack, and saying where it came from.
//!
//! A rule pack used to be a constant: `include_str!` at a path that spelled the year,
//! so shipping a new year was a Rust edit and a rebuild, and a machine running in a
//! year with no pack could not be given one at all. It is a **folder** now, searched
//! in a fixed order that ends at the pack compiled into the binary.
//!
//! The order is the same discipline as the vault's location
//! ([`crate::location::resolve_vault_location`]): a path given to this run wins over
//! the environment, which wins over what sits beside the binary, which wins over what
//! the binary carries. Every candidate is validated by
//! [`vnf_core::rules::load_for_year`], so a pack from a folder is held to exactly the
//! standard of the shipped one.
//!
//! The resolver deliberately **never fails** on a bad candidate. A file the person
//! supplied that cannot be used is recorded as refused, the window opens on the pack
//! it can trust, and the Regras page says which file was refused and why. Refusing to
//! open the window over a malformed file would take the agenda away from someone who
//! can fix it by deleting one file — and the refusal is worth more on that page than
//! in a console this build does not have.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vnf_core::rules::{PackChoice, PackError, load_for_year};
use vnf_core::types::LoadedPack;

use crate::location::Env;

/// The folder that holds `<jurisdiction>/<year>.json`.
pub const PACK_DIR: &str = "rules";

/// The optional variable that points at a pack file, or at a folder holding one.
///
/// It is the development and power-user door, and the only one there is: this build
/// has no command line, so a path has to arrive through the environment or sit in a
/// folder the application already looks at.
pub const PACK_ENV: &str = "VN_FINANCE_RULES";

/// The sub-folder the jurisdiction's packs live in.
const JURISDICTION_DIR: &str = "pt";

/// Where the pack the window is running on came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackOrigin {
    /// A path given to this run.
    Flag,
    /// `VN_FINANCE_RULES`.
    Env,
    /// `rules/` in the folder the binary runs from — the release archive's layout.
    Beside,
    /// `rules/` in the repository, when running out of `target/`.
    SourceTree,
    /// The pack compiled into the binary.
    Embedded,
}

impl PackOrigin {
    /// The phrase the interface uses for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::Beside => "beside",
            Self::SourceTree => "source",
            Self::Embedded => "embedded",
        }
    }
}

/// One candidate file that was found and refused.
///
/// Only refusals are kept: a missing file is the normal state of every year that has
/// not been written yet, and listing it would bury the one line that matters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCandidate {
    /// The path that was tried.
    pub path: String,
    /// Why it was not used.
    pub problem: String,
}

/// The pack the window runs on, and the trail that led to it.
#[derive(Debug, Clone, PartialEq)]
pub struct PackResolution {
    /// The pack.
    pub loaded: LoadedPack,
    /// Where its bytes came from.
    pub origin: PackOrigin,
    /// Whether it is the requested year's own pack.
    pub exact: bool,
    /// Files that exist and were refused, in the order they were tried.
    pub rejected: Vec<PackCandidate>,
}

/// What to search, and where.
///
/// A value rather than global reads, so a test can ask "what would happen on a machine
/// whose binary sits here and whose environment says this?" without owning either.
pub struct PackSearch<'a> {
    /// A path given directly.
    pub explicit: Option<&'a Path>,
    /// The environment, for [`PACK_ENV`].
    pub env: &'a Env,
    /// The folder the binary runs from; `None` when there is no such thing.
    pub exe_dir: Option<&'a Path>,
}

/// The pack file a base path means, for a year.
///
/// A base that names a `.json` file is taken as the pack itself — that is what
/// `VN_FINANCE_RULES=C:\packs\2027.json` has to mean. Anything else is a folder
/// holding `pt/`, so the same variable can point at a checkout's `rules/`.
fn candidate_path(base: &Path, year: i64) -> PathBuf {
    if base
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return base.to_path_buf();
    }
    base.join(JURISDICTION_DIR).join(format!("{year}.json"))
}

/// `rules/` in the repository, when the binary is running out of `target/`.
///
/// The check is the shape of the path, not a guess about its contents: a development
/// build lives in `target/<profile>/`, and an installed one does not. That keeps an
/// installed binary from reading a `rules/` folder two levels above wherever it was
/// put — a folder it does not own and did not ship.
fn source_tree_pack_dir(exe_dir: &Path) -> Option<PathBuf> {
    let profile_dir = exe_dir.file_name()?;
    let target = exe_dir.parent()?;
    if target.file_name()? != "target" || profile_dir.is_empty() {
        return None;
    }
    let root = target.parent()?;
    let rules = root.join(PACK_DIR);
    if rules.is_dir() { Some(rules) } else { None }
}

/// The pack files to try, in order, with the origin each one means.
#[must_use]
pub fn pack_candidates(search: &PackSearch<'_>, year: i64) -> Vec<(PackOrigin, PathBuf)> {
    let mut candidates: Vec<(PackOrigin, PathBuf)> = Vec::new();

    if let Some(explicit) = search.explicit.filter(|path| !path.as_os_str().is_empty()) {
        candidates.push((PackOrigin::Flag, candidate_path(explicit, year)));
    }
    if let Some(from_env) = search.env.non_blank(PACK_ENV) {
        candidates.push((PackOrigin::Env, candidate_path(Path::new(from_env), year)));
    }
    if let Some(exe_dir) = search.exe_dir {
        candidates.push((PackOrigin::Beside, candidate_path(&exe_dir.join(PACK_DIR), year)));
        if let Some(source) = source_tree_pack_dir(exe_dir) {
            candidates.push((PackOrigin::SourceTree, candidate_path(&source, year)));
        }
    }
    candidates
}

/// The pack for a year, from the first candidate that can be used.
///
/// # Errors
///
/// [`PackError`] only when the pack compiled into the binary is itself broken, which
/// is a build defect. Every other failure is recorded in
/// [`PackResolution::rejected`] and the next candidate is tried.
pub fn resolve_pack(year: i64, search: &PackSearch<'_>) -> Result<PackResolution, PackError> {
    let mut rejected: Vec<PackCandidate> = Vec::new();

    for (origin, path) in pack_candidates(search, year) {
        if !path.is_file() {
            continue;
        }
        match load_for_year(year, Some(&path)) {
            Ok(PackChoice { loaded, exact }) => {
                return Ok(PackResolution {
                    loaded,
                    origin,
                    exact,
                    rejected,
                });
            }
            Err(error) => rejected.push(PackCandidate {
                path: path.display().to_string(),
                problem: error.to_string(),
            }),
        }
    }

    let PackChoice { loaded, exact } = load_for_year(year, None)?;
    Ok(PackResolution {
        loaded,
        origin: PackOrigin::Embedded,
        exact,
        rejected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    type Problem = Box<dyn std::error::Error>;

    /// The shipped pack's text, with a version no release has, so a test can tell the
    /// file apart from the copy in the binary.
    fn pack_text() -> Result<String, Problem> {
        let text = vnf_core::rules::EMBEDDED_PACKS
            .first()
            .map(|entry| entry.text)
            .ok_or("the binary carries no embedded pack")?
            .replace("\"packVersion\": \"2026.1.2\"", "\"packVersion\": \"2099.9.9\"");
        Ok(text)
    }

    fn write_pack(dir: &Path, year: i64, text: &str) -> Result<PathBuf, Problem> {
        let folder = dir.join(JURISDICTION_DIR);
        fs::create_dir_all(&folder)?;
        let path = folder.join(format!("{year}.json"));
        fs::write(&path, text)?;
        Ok(path)
    }

    fn search<'a>(env: &'a Env, exe_dir: Option<&'a Path>) -> PackSearch<'a> {
        PackSearch {
            explicit: None,
            env,
            exe_dir,
        }
    }

    #[test]
    fn a_pack_beside_the_binary_wins_over_the_one_compiled_in() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        write_pack(&dir.path().join(PACK_DIR), 2026, &pack_text()?)?;
        let env = Env::for_test(&[], true);
        let exe = dir.path().to_path_buf();

        let resolution = resolve_pack(2026, &search(&env, Some(&exe)))?;

        assert_eq!(resolution.origin, PackOrigin::Beside);
        assert!(resolution.exact);
        assert_eq!(
            resolution.loaded.pack.pack_version, "2099.9.9",
            "the file beside the binary must be the one that loaded"
        );
        assert!(resolution.rejected.is_empty());
        Ok(())
    }

    #[test]
    fn a_refused_pack_falls_back_and_is_reported_rather_than_hidden() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        write_pack(&dir.path().join(PACK_DIR), 2027, "{ this is not a rule pack")?;
        let env = Env::for_test(&[], true);
        let exe = dir.path().to_path_buf();

        let resolution = resolve_pack(2027, &search(&env, Some(&exe)))?;

        assert_eq!(resolution.origin, PackOrigin::Embedded);
        assert_eq!(resolution.rejected.len(), 1, "the refusal must be kept");
        let first = resolution.rejected.first().ok_or("the refusal must be kept")?;
        assert!(first.path.contains("2027.json"), "path was {}", first.path);
        assert!(
            first.problem.contains("not valid JSON"),
            "problem was {}",
            first.problem
        );
        Ok(())
    }

    #[test]
    fn the_environment_can_name_a_file_or_a_folder() -> Result<(), Problem> {
        let dir = TempDir::new()?;

        let file = dir.path().join("pacote-2026.json");
        fs::write(&file, pack_text()?)?;
        let env = Env::for_test(&[(PACK_ENV, &file.display().to_string())], true);
        let resolution = resolve_pack(2026, &search(&env, None))?;
        assert_eq!(resolution.origin, PackOrigin::Env);
        assert_eq!(resolution.loaded.pack.pack_version, "2099.9.9");

        // The same variable pointing at a folder holding `pt/`.
        let checkout = dir.path().join("checkout");
        write_pack(&checkout, 2026, &pack_text()?)?;
        let env = Env::for_test(&[(PACK_ENV, &checkout.display().to_string())], true);
        let resolution = resolve_pack(2026, &search(&env, None))?;
        assert_eq!(resolution.origin, PackOrigin::Env);
        assert!(resolution.exact);
        Ok(())
    }

    #[test]
    fn a_development_build_reads_the_repository_rules() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        let target = dir.path().join("target").join("debug");
        fs::create_dir_all(&target)?;
        write_pack(&dir.path().join(PACK_DIR), 2026, &pack_text()?)?;
        let env = Env::for_test(&[], true);

        let resolution = resolve_pack(2026, &search(&env, Some(&target)))?;
        assert_eq!(resolution.origin, PackOrigin::SourceTree);
        assert_eq!(resolution.loaded.pack.pack_version, "2099.9.9");
        Ok(())
    }

    #[test]
    fn an_installed_binary_does_not_read_a_rules_folder_it_does_not_own() -> Result<(), Problem> {
        // `…/vn-finance/rules` beside the executable is a candidate; `…/rules` two
        // levels above an installed executable is not, even when it exists.
        let dir = TempDir::new()?;
        let app = dir.path().join("Programas").join("vn-finance");
        fs::create_dir_all(&app)?;
        write_pack(&dir.path().join(PACK_DIR), 2026, &pack_text()?)?;
        let env = Env::for_test(&[], true);

        let resolution = resolve_pack(2026, &search(&env, Some(&app)))?;
        assert_eq!(
            resolution.origin,
            PackOrigin::Embedded,
            "a parent folder the binary does not own must not be read"
        );
        Ok(())
    }

    #[test]
    fn a_file_that_declares_another_year_is_used_but_is_not_exact() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        // A 2027-named file that is really the 2026 pack: a copy-paste, not a calendar.
        write_pack(&dir.path().join(PACK_DIR), 2027, &pack_text()?)?;
        let env = Env::for_test(&[], true);
        let exe = dir.path().to_path_buf();

        let resolution = resolve_pack(2027, &search(&env, Some(&exe)))?;
        assert_eq!(resolution.loaded.pack.year, 2026);
        assert!(!resolution.exact, "the file's own year is what counts");
        Ok(())
    }

    #[test]
    fn a_year_with_no_file_anywhere_is_the_pack_compiled_in() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        let env = Env::for_test(&[], true);
        let exe = dir.path().to_path_buf();

        let resolution = resolve_pack(2099, &search(&env, Some(&exe)))?;
        assert_eq!(resolution.origin, PackOrigin::Embedded);
        assert!(!resolution.exact);
        assert!(resolution.rejected.is_empty(), "nothing was found to refuse");
        Ok(())
    }

    #[test]
    fn every_origin_has_a_label() {
        for origin in [
            PackOrigin::Flag,
            PackOrigin::Env,
            PackOrigin::Beside,
            PackOrigin::SourceTree,
            PackOrigin::Embedded,
        ] {
            assert!(!origin.label().is_empty());
        }
    }
}
