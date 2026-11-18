//! Reading the corpus folder into memory.
//!
//! The domain (`vnf_core::sources`) checks citations; this reads the bytes it checks
//! them against. The two are kept apart on purpose: the check is a function of values
//! and can be tested without a directory, and this is the only place that decides a
//! file on disk is one of the documents the pack cites.
//!
//! A missing folder is **not** an error. A machine that installed the binary without
//! the `sources/` directory has no evidence and the application still opens; every
//! source is then reported as not collected, which is true and is what the Regras page
//! says. Losing the window because a folder of documents is absent would be the tool
//! confusing its own evidence for its ability to compute.

use std::path::{Path, PathBuf};

use vnf_core::sources::Corpus;

use crate::{StoreError, io_error};

/// The folder, beside a pack, that holds the documents it cites.
pub const CORPUS_DIR: &str = "sources";

/// The most this will read into memory, in total.
///
/// The corpus is the law, not a media library: 3 MB today, perhaps 30 MB in a decade.
/// A cap exists so that a folder someone dropped a backup into cannot make the window
/// wait for a gigabyte, and hitting it is reported rather than silently truncated.
pub const CORPUS_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

/// How deep the walk goes. `sources/` is flat today; a jurisdiction sub-folder is the
/// most a later layout should need.
const MAX_DEPTH: usize = 4;

/// A single file larger than this is skipped, with a note.
const FILE_LIMIT_BYTES: u64 = 32 * 1024 * 1024;

/// The corpus as it was read, and what could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusLoad {
    /// The documents, keyed by the path the pack names.
    pub corpus: Corpus,
    /// The folder that was looked at, whether or not it exists.
    pub root: Option<PathBuf>,
    /// Anything worth telling the reader about the read itself.
    pub notes: Vec<String>,
}

impl CorpusLoad {
    /// A load that looked nowhere: what a pack compiled into the binary gets when no
    /// `rules/pt/sources` folder is found either.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The empty load, as a value that lives as long as the program.
    ///
    /// A caller with no corpus — a test, or a caller that decided not to read one —
    /// needs a reference that outlives the model it builds, and an empty corpus is not
    /// something to allocate on every rebuild.
    #[must_use]
    pub fn empty() -> &'static Self {
        static EMPTY: std::sync::OnceLock<CorpusLoad> = std::sync::OnceLock::new();
        EMPTY.get_or_init(Self::none)
    }

    /// The folder, as the interface prints it.
    #[must_use]
    pub fn root_display(&self) -> Option<String> {
        self.root.as_ref().map(|root| root.display().to_string())
    }
}

/// The documents under a folder, keyed by their path relative to it.
///
/// # Errors
///
/// [`StoreError::Io`] when the folder exists and cannot be listed. A folder that does
/// not exist is an empty corpus, not an error.
pub fn read_corpus(dir: &Path) -> Result<CorpusLoad, StoreError> {
    let mut load = CorpusLoad {
        root: Some(dir.to_path_buf()),
        ..CorpusLoad::default()
    };
    if !dir.is_dir() {
        return Ok(load);
    }

    let mut found: Vec<(String, PathBuf)> = Vec::new();
    walk(dir, "", 0, &mut found, &mut load.notes)?;
    // The filesystem's order is not a fact about the corpus, and a model built twice
    // from the same folder must be identical.
    found.sort_by(|left, right| left.0.cmp(&right.0));

    let mut total = 0u64;
    for (key, path) in found {
        let size = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        if size > FILE_LIMIT_BYTES {
            load.notes.push(format!(
                "{key}: {size} bytes é grande demais para ser lido como fonte; foi ignorado."
            ));
            continue;
        }
        if total + size > CORPUS_LIMIT_BYTES {
            load.notes.push(format!(
                "o corpus passou de {} MB: a leitura parou em {key}.",
                CORPUS_LIMIT_BYTES / (1024 * 1024)
            ));
            break;
        }
        let bytes = std::fs::read(&path).map_err(|error| io_error(&path, &error))?;
        total += bytes.len() as u64;
        load.corpus.insert(key, bytes);
    }
    Ok(load)
}

/// Collect the files under a folder, keyed by a forward-slash path relative to it.
fn walk(
    dir: &Path,
    prefix: &str,
    depth: usize,
    out: &mut Vec<(String, PathBuf)>,
    notes: &mut Vec<String>,
) -> Result<(), StoreError> {
    let entries = std::fs::read_dir(dir).map_err(|error| io_error(dir, &error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error(dir, &error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // A hidden file is the operating system's, not the corpus's.
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| io_error(&path, &error))?;
        if kind.is_dir() {
            if depth + 1 > MAX_DEPTH {
                notes.push(format!("{prefix}{name}/: pasta demasiado funda; foi ignorada."));
                continue;
            }
            walk(&path, &format!("{prefix}{name}/"), depth + 1, out, notes)?;
        } else if kind.is_file() {
            out.push((format!("{prefix}{name}"), path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    type Problem = Box<dyn std::error::Error>;

    #[test]
    fn a_missing_folder_is_an_empty_corpus_and_not_an_error() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        let load = read_corpus(&dir.path().join("nao-existe"))?;
        assert!(load.corpus.is_empty());
        assert!(load.root.is_some(), "the folder we looked at is worth showing");
        assert!(load.notes.is_empty());
        Ok(())
    }

    #[test]
    fn files_are_keyed_by_the_path_the_pack_would_name() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        fs::create_dir_all(dir.path().join("cirs"))?;
        fs::write(dir.path().join("cirs").join("art60.html"), "<p>artigo</p>")?;
        fs::write(dir.path().join("topo.html"), "<p>topo</p>")?;

        let load = read_corpus(dir.path())?;
        let mut keys: Vec<&str> = load.corpus.keys().collect();
        keys.sort_unstable();
        assert_eq!(keys, ["cirs/art60.html", "topo.html"]);
        Ok(())
    }

    #[test]
    fn a_hidden_file_is_not_a_document() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        fs::write(dir.path().join(".DS_Store"), [0u8, 1, 2])?;
        fs::write(dir.path().join("real.html"), "<p>real</p>")?;

        let load = read_corpus(dir.path())?;
        assert_eq!(load.corpus.len(), 1);
        assert!(load.corpus.get("real.html").is_some());
        Ok(())
    }

    #[test]
    fn the_load_is_the_same_however_the_filesystem_orders_the_folder() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        for name in ["c.html", "a.html", "b.html"] {
            fs::write(dir.path().join(name), format!("<p>{name}</p>"))?;
        }
        let first = read_corpus(dir.path())?;
        let second = read_corpus(dir.path())?;
        let keys = |load: &CorpusLoad| load.corpus.keys().map(str::to_owned).collect::<Vec<String>>();
        assert_eq!(keys(&first), keys(&second));
        assert_eq!(keys(&first), ["a.html", "b.html", "c.html"]);
        Ok(())
    }

    #[test]
    fn the_bytes_read_are_the_bytes_on_disk() -> Result<(), Problem> {
        let dir = TempDir::new()?;
        let bytes = b"<p>linha\r\noutra</p>".to_vec();
        fs::write(dir.path().join("pagina.html"), &bytes)?;
        let load = read_corpus(dir.path())?;
        let entry = load.corpus.get("pagina.html").ok_or("the file must be read")?;
        assert_eq!(entry.bytes, bytes, "a corpus that rewrites bytes has no hashes");
        Ok(())
    }
}
