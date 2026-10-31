//! Writing files so that a crash cannot leave a half-written vault.
//!
//! Three rules, and the reference implementation broke the first two:
//!
//! - **A replace is atomic.** Write a uniquely-named temporary file in the same
//!   directory, `sync_all` it, rename it over the target, and sync the directory so
//!   the rename itself survives. The reference shared one `<file>.tmp` for every
//!   writer, so two processes could rename each other's bytes into place.
//! - **An append is durable.** One line, then `sync_data`. An invoice the person
//!   just recorded is worth a disk flush.
//! - **A read never guesses.** A JSONL line that is not JSON is reported with its
//!   line number, because "this file is corrupt" and "this file is empty" are
//!   different facts and only one of them is safe to ignore.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::Value;

use crate::{StoreError, io_error};

/// Distinguishes two writers in one process (and two runs that reused a pid).
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temporary name that no other writer will choose.
fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    path.with_file_name(format!(".{name}.{pid}.{unique}.tmp"))
}

/// Replace a JSON file atomically, pretty-printed with a trailing newline.
///
/// # Errors
///
/// [`StoreError::Io`] when anything in the sequence fails; the temporary file is
/// removed first, so a failure does not leave litter beside the vault.
pub fn write_json_atomic(path: &Path, value: &Value) -> Result<(), StoreError> {
    write_typed(path, value)
}

/// Replace a file atomically with a serialised value.
///
/// Serialising the struct rather than a `Value` keeps the field order the struct
/// declares, which is the order the reference writes and therefore the order a reader
/// diffing a file from each implementation sees. `serde_json::to_value` sorts keys
/// alphabetically, which would quietly change that.
///
/// # Errors
///
/// [`StoreError::Io`] when anything in the sequence fails.
pub fn write_typed<T>(path: &Path, value: &T) -> Result<(), StoreError>
where
    T: Serialize + ?Sized,
{
    let mut text = serde_json::to_string_pretty(value).map_err(|error| StoreError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    text.push('\n');
    write_bytes_atomic(path, text.as_bytes())
}

/// Replace a file atomically with the given bytes.
///
/// # Errors
///
/// [`StoreError::Io`] when anything in the sequence fails.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    let temporary = temporary_path(path);
    let write = (|| -> Result<(), StoreError> {
        let mut file = std::fs::File::create(&temporary).map_err(|error| io_error(&temporary, &error))?;
        file.write_all(bytes)
            .map_err(|error| io_error(&temporary, &error))?;
        file.sync_all().map_err(|error| io_error(&temporary, &error))?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|error| io_error(path, &error))?;
        sync_parent(path);
        Ok(())
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write
}

/// Make the rename itself durable.
///
/// On Unix this opens the directory and syncs it. On Windows a directory cannot be
/// opened as a file, and the rename is journalled by the filesystem anyway, so there
/// is nothing safe to add here.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(handle) = std::fs::File::open(parent) {
            let _ = handle.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Append one JSON line and flush it to the disk.
///
/// # Errors
///
/// [`StoreError::Io`] when the file cannot be opened or written.
pub fn append_line(path: &Path, value: &Value) -> Result<(), StoreError> {
    append_typed(path, value)
}

/// Append one serialised value as a line, keeping the struct's field order.
///
/// # Errors
///
/// [`StoreError::Io`] when the file cannot be opened or written.
pub fn append_typed<T>(path: &Path, value: &T) -> Result<(), StoreError>
where
    T: Serialize + ?Sized,
{
    let mut line = serde_json::to_string(value).map_err(|error| StoreError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    line.push('\n');
    append_text(path, &line)
}

/// Append text to a file, creating it and its folder when needed.
///
/// # Errors
///
/// [`StoreError::Io`] when the file cannot be opened or written.
pub fn append_text(path: &Path, text: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| io_error(path, &error))?;
    file.write_all(text.as_bytes())
        .map_err(|error| io_error(path, &error))?;
    file.sync_data().map_err(|error| io_error(path, &error))?;
    Ok(())
}

/// Read a JSON file, or `None` when it is not there.
///
/// # Errors
///
/// [`StoreError::NotJson`] when it exists and is not JSON.
pub fn read_json(path: &Path) -> Result<Option<Value>, StoreError> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|error| io_error(path, &error))?;
    match serde_json::from_str(&text) {
        Ok(value) => Ok(Some(value)),
        Err(error) => Err(StoreError::NotJson {
            path: path.display().to_string(),
            message: error.to_string(),
        }),
    }
}

/// Read a JSONL file, one record per non-blank line.
///
/// # Errors
///
/// [`StoreError::Corrupt`] naming the line that is not JSON, and
/// [`StoreError::Io`] when the file cannot be read.
pub fn read_jsonl(path: &Path) -> Result<Vec<Value>, StoreError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(|error| io_error(path, &error))?;
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(value) => records.push(value),
            Err(error) => {
                return Err(StoreError::Corrupt {
                    path: path.display().to_string(),
                    line: index + 1,
                    message: error.to_string(),
                });
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Problem = Box<dyn std::error::Error>;

    #[test]
    fn a_json_file_is_replaced_atomically_and_reads_back() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("nested").join("file.json");
        write_json_atomic(&target, &serde_json::json!({"a": 1}))?;
        let read = read_json(&target)?.unwrap_or(serde_json::Value::Null);
        assert_eq!(read["a"], 1);
        // And the temporary file is gone, so nothing accumulates beside the vault.
        let leftovers: Vec<String> = std::fs::read_dir(target.parent().unwrap_or(dir.path()))?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
        Ok(())
    }

    #[test]
    fn a_replacement_keeps_no_partial_state() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("file.json");
        write_json_atomic(&target, &serde_json::json!({"a": 1}))?;
        write_json_atomic(&target, &serde_json::json!({"b": 2}))?;
        let read = read_json(&target)?.unwrap_or(serde_json::Value::Null);
        assert_eq!(read.get("a"), None, "the old content is gone, not merged");
        assert_eq!(read["b"], 2);
        Ok(())
    }

    #[test]
    fn appending_adds_one_line_at_a_time() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("ledger").join("lines.jsonl");
        append_line(&target, &serde_json::json!({"n": 1}))?;
        append_line(&target, &serde_json::json!({"n": 2}))?;
        let records = read_jsonl(&target)?;
        assert_eq!(records.len(), 2);
        assert_eq!(records[1]["n"], 2);
        Ok(())
    }

    #[test]
    fn a_corrupt_line_names_itself_and_its_number() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("lines.jsonl");
        std::fs::write(&target, "{\"n\":1}\nnot json\n{\"n\":3}\n")?;
        match read_jsonl(&target) {
            Err(StoreError::Corrupt { line, path, .. }) => {
                assert_eq!(line, 2);
                assert!(path.ends_with("lines.jsonl"));
            }
            other => assert!(other.is_err(), "a corrupt line must be an error"),
        }
        Ok(())
    }

    #[test]
    fn a_file_that_is_not_there_reads_as_absent_not_empty() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        assert_eq!(read_json(&dir.path().join("nope.json"))?, None);
        assert!(read_jsonl(&dir.path().join("nope.jsonl"))?.is_empty());
        Ok(())
    }

    #[test]
    fn blank_lines_are_not_records() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("lines.jsonl");
        std::fs::write(&target, "{\"n\":1}\n\n   \n{\"n\":2}\n")?;
        assert_eq!(read_jsonl(&target)?.len(), 2);
        Ok(())
    }
}
