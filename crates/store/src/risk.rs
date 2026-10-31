//! Whether the vault sits somewhere it should not.
//!
//! Personal data must not be committable by accident. A vault inside **this**
//! application's repository is refused outright: it is the one place where a stray
//! `git add -A` would publish someone's entire financial life. A vault inside some
//! *other* repository — a dotfiles repo in the home directory, say — is legitimate
//! but worth a loud warning, because the ignore rules are then someone else's
//! responsibility.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How bad it is that the vault is where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Not inside a work tree.
    None,
    /// Inside somebody else's work tree.
    Warning,
    /// Inside this application's own repository.
    Fatal,
}

/// The verdict on a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataDirRisk {
    /// How bad it is.
    pub level: RiskLevel,
    /// The work-tree root the folder sits in, when it sits in one.
    pub git_root: Option<String>,
    /// A sentence for the person, in Portuguese.
    pub message: Option<String>,
}

/// Walk up looking for a `.git`, and return the work-tree root.
///
/// `.git` may be a directory or, in a worktree or a submodule, a file, so this asks
/// whether the entry exists rather than whether it is a directory.
#[must_use]
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    for _guard in 0..64 {
        if current.join(".git").exists() {
            return Some(current);
        }
        let parent = current.parent()?.to_path_buf();
        if parent == current {
            return None;
        }
        current = parent;
    }
    None
}

/// The repository this binary was built from, when it was built from one.
///
/// The reference derived this from its own module URL. A shipped binary has no
/// checkout, so the path is baked in at compile time: on a developer's machine it
/// is the real repository, and in a build from an archive it is a path no vault will
/// ever be inside. Either way the check is right, and it never guesses at runtime.
#[must_use]
pub fn build_root() -> Option<PathBuf> {
    option_env!("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|manifest| manifest.ancestors().nth(2).map(Path::to_path_buf))
}

/// Whether two paths name the same place, as far as this can tell.
fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
    } else {
        left == right
    }
}

/// Judge a folder.
#[must_use]
pub fn check_data_dir_risk(dir: &Path) -> DataDirRisk {
    let Some(git_root) = find_git_root(dir) else {
        return DataDirRisk {
            level: RiskLevel::None,
            git_root: None,
            message: None,
        };
    };

    if let Some(app_root) = build_root()
        && same_path(&git_root, &app_root)
    {
        let root = git_root.display();
        let given = crate::location::absolute(dir).display().to_string();
        return DataDirRisk {
            level: RiskLevel::Fatal,
            git_root: Some(git_root.display().to_string()),
            message: Some(format!(
                "o cofre não pode viver dentro do repositório da aplicação ({root}).\n  \
                 Caminho indicado: {given}\n  Os teus dados nunca devem estar a um `git add -A` \
                 de serem publicados.\n  Escolhe outra pasta (por omissão fica em \
                 ~/.vn-finance)."
            )),
        };
    }

    let root = git_root.display();
    DataDirRisk {
        level: RiskLevel::Warning,
        git_root: Some(git_root.display().to_string()),
        message: Some(format!(
            "o cofre está dentro de um repositório git ({root}). Confirma que está ignorado: um \
             repositório de dotfiles que inclua esta pasta publica os teus dados."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Problem = Box<dyn std::error::Error>;

    #[test]
    fn a_plain_folder_is_no_risk() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let risk = check_data_dir_risk(dir.path());
        assert_eq!(risk.level, RiskLevel::None);
        assert_eq!(risk.git_root, None);
        assert_eq!(risk.message, None);
        Ok(())
    }

    #[test]
    fn a_folder_inside_some_other_repository_is_a_warning() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path().canonicalize()?;
        std::fs::create_dir_all(repo.join(".git"))?;
        let vault = repo.join("vault");
        std::fs::create_dir_all(&vault)?;
        let risk = check_data_dir_risk(&vault);
        assert_eq!(risk.level, RiskLevel::Warning);
        assert_eq!(
            risk.git_root.as_deref(),
            Some(repo.display().to_string().as_str())
        );
        assert!(
            risk.message
                .unwrap_or_default()
                .contains("dentro de um repositório git"),
            "the warning must say what is wrong"
        );
        Ok(())
    }

    #[test]
    fn the_work_tree_root_is_found_from_a_nested_folder() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path().canonicalize()?;
        std::fs::create_dir_all(repo.join(".git"))?;
        let nested = repo.join("a").join("b").join("c");
        std::fs::create_dir_all(&nested)?;
        assert_eq!(find_git_root(&nested), Some(repo));
        Ok(())
    }

    #[test]
    fn findings_are_json_shaped_like_the_model_expects() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let risk = check_data_dir_risk(dir.path());
        let value = serde_json::to_value(&risk)?;
        assert_eq!(
            value.get("level").and_then(serde_json::Value::as_str),
            Some("none")
        );
        assert_eq!(value.get("gitRoot"), Some(&serde_json::Value::Null));
        Ok(())
    }
}
