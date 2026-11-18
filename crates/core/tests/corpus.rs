//! The shipped pack, checked against the shipped corpus.
//!
//! Every other test in `sources` uses a page written for it. This one uses the two
//! artefacts that actually ship: `rules/pt/2026.json` and the documents under
//! `rules/pt/sources/`. It is the test that fails when a quote was transcribed wrong,
//! when a document was re-fetched and its hash moved, or when a source was bound to a
//! file that is not in the folder — the three ways the evidence layer goes quietly
//! wrong.
//!
//! It reads the repository, which is not something the domain does; it is a test of an
//! artefact, so it reads the artefact.

use std::error::Error;
use std::path::{Path, PathBuf};

use vnf_core::rules::{load_file, load_rule_pack};
use vnf_core::sources::{Corpus, SourceState, check};

/// The repository root, from this crate's own directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn pack_path() -> PathBuf {
    repo_root().join("rules").join("pt").join("2026.json")
}

fn corpus_dir() -> PathBuf {
    repo_root().join("rules").join("pt").join("sources")
}

/// The corpus as the store would read it: every file, keyed by its name.
fn corpus(dir: &Path) -> Result<Corpus, Box<dyn Error>> {
    let mut corpus = Corpus::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "README.md" || name == "fetch.json" {
            continue;
        }
        corpus.insert(name, std::fs::read(entry.path())?);
    }
    Ok(corpus)
}

#[test]
fn every_source_the_shipped_pack_binds_verifies_against_its_copy() -> Result<(), Box<dyn Error>> {
    let loaded = load_file(&pack_path())?;
    let corpus = corpus(&corpus_dir())?;
    let report = check(&loaded.pack, &corpus);

    assert!(
        !report.has_errors(),
        "the shipped pack and its corpus disagree: {:#?}",
        report.problems
    );
    assert_eq!(
        report.absent, 0,
        "a bound source must have its file: {:#?}",
        report.problems
    );
    assert_eq!(report.failed, 0, "nothing may fail: {:#?}", report.problems);
    assert!(
        report.verified >= 18,
        "the corpus has 18 official documents, verified {}",
        report.verified
    );
    assert!(report.files >= 18, "the folder holds {} files", report.files);
    assert!(
        report.bytes > 1_000_000,
        "1 MB of law at least, got {}",
        report.bytes
    );
    Ok(())
}

#[test]
fn the_bound_sources_are_the_ones_that_carry_quotes() -> Result<(), Box<dyn Error>> {
    let loaded = load_file(&pack_path())?;

    let bound = loaded
        .pack
        .sources
        .iter()
        .filter(|source| source.file.is_some())
        .count();
    assert_eq!(bound, 18, "every official document is bound");

    let passages: usize = loaded
        .pack
        .sources
        .iter()
        .map(|source| source.passages.len())
        .sum();
    assert!(passages >= 8, "the passages a reader can check, found {passages}");

    // A passage that no rule cites is a quote kept for its own sake; the pack should
    // not accumulate those, because the check would then pass while binding nothing.
    for source in &loaded.pack.sources {
        for passage in &source.passages {
            let cited = loaded.pack.obligations.iter().any(|rule| {
                rule.citations
                    .iter()
                    .any(|citation| citation.source_id == source.id && citation.passage_id == passage.id)
            });
            assert!(
                cited,
                "{} / {} is declared but no rule cites it",
                source.id, passage.id
            );
        }
    }
    Ok(())
}

#[test]
fn a_passage_that_is_not_in_its_copy_fails_the_check() -> Result<(), Box<dyn Error>> {
    // The check must be able to fail on the real artefacts too: change one word of one
    // quote and the report has to name it, rather than trusting the pack's own word.
    let corpus = corpus(&corpus_dir())?;

    let mut text = std::fs::read_to_string(pack_path())?;
    text = text.replacen(
        "30 de junho, independentemente",
        "31 de julho, independentemente",
        1,
    );
    let tampered = load_rule_pack(&text, "adulterado.json")?;

    let report = check(&tampered.pack, &corpus);
    assert!(report.has_errors());
    assert_eq!(report.state_of("at-cirs-art60"), SourceState::Failed);
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.message.contains("art60-n1")),
        "{:#?}",
        report.problems
    );
    Ok(())
}
