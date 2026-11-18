//! The pack the window runs on, end to end.
//!
//! `store::pack` is tested where the filesystem is; this file tests the seam above
//! it — that a session opened for a year resolves the pack for that year, carries its
//! origin onto the model, and marks a fallback as a fallback. The exit criterion of
//! F0 is one sentence: a new year's pack is a file, not a rebuild.

use std::path::Path;

use vnf_core::rules::{EMBEDDED_PACKS, FreshnessStatus};
use vnf_store::location::Env;
use vnf_store::pack::PackOrigin;
use vnf_ui::Session;

type Problem = Box<dyn std::error::Error>;

/// The shipped pack, retitled as another year — a calendar nobody wrote, which is
/// exactly what a copy-pasted pack is.
fn pack_for(year: i64) -> Result<String, Problem> {
    let text = EMBEDDED_PACKS
        .first()
        .map(|entry| entry.text)
        .ok_or("the binary carries no embedded pack")?;
    Ok(text
        .replace("\"year\": 2026", &format!("\"year\": {year}"))
        .replace("\"packVersion\": \"2026.1.2\"", "\"packVersion\": \"2099.1.0\""))
}

fn session_for(dir: &Path, pack: Option<&Path>, year: i64) -> Result<Session, Problem> {
    let home = dir.display().to_string();
    let pointed_at = pack.map(|pack| pack.display().to_string());
    let mut variables: Vec<(&str, &str)> = vec![("HOME", &home)];
    if let Some(pointed_at) = &pointed_at {
        variables.push(("VN_FINANCE_RULES", pointed_at));
    }
    let env = Env::for_test(&variables, false);
    Ok(Session::open_in(dir, &env, env!("CARGO_PKG_VERSION"), year)?)
}

#[test]
fn a_pack_named_by_the_environment_is_the_one_the_window_runs_on() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    let pack = dir.path().join("pt-2027.json");
    std::fs::write(&pack, pack_for(2027)?)?;

    let session = session_for(dir.path(), Some(&pack), 2027)?;
    assert_eq!(session.pack().pack.year, 2027);
    assert_eq!(session.pack().pack.pack_version, "2099.1.0");
    assert_eq!(session.pack_origin(), PackOrigin::Env);

    // The model the pages read carries the same two facts: the law on screen is the
    // 2027 file, and it is that year's own pack.
    let model = session.reload("2027-01-10", 2027)?;
    assert!(model.pack.exact);
    assert_eq!(model.pack.origin, PackOrigin::Env);
    assert_eq!(model.pack.summary.year, 2027);
    assert_eq!(model.pack.freshness.status, FreshnessStatus::Current);
    assert!(
        model.pack.rejected.is_empty(),
        "nothing was refused: {:?}",
        model.pack.rejected
    );
    Ok(())
}

/// A profile, so the agenda has something to be built from: an empty vault has no
/// agenda, and a test that asserts nothing about one would pass for the wrong reason.
fn seed_profile(session: &Session) -> Result<(), Problem> {
    let profile = vnf_core::profile::create_default_profile(&vnf_core::profile::ProfileInput {
        nif: "123456789".to_owned(),
        name: "Pessoa Exemplo".to_owned(),
        start_date: Some("2019-04-01".to_owned()),
        iva_regime: Some(vnf_core::types::IvaRegime::Trimestral),
        ..Default::default()
    })?;
    session.vault().save_profile(&profile)?;
    Ok(())
}

#[test]
fn a_year_without_a_pack_is_served_the_newest_one_and_says_so() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    let session = session_for(dir.path(), None, 2027)?;
    seed_profile(&session)?;

    let newest = EMBEDDED_PACKS
        .iter()
        .map(|entry| entry.year)
        .max()
        .unwrap_or_default();
    assert_eq!(session.pack().pack.year, newest);
    assert_eq!(session.pack_origin(), PackOrigin::Embedded);

    let model = session.reload("2027-01-10", 2027)?;
    assert!(
        !model.pack.exact,
        "a fallback must never present itself as the year asked for"
    );
    assert_eq!(model.pack.freshness.status, FreshnessStatus::Stale);
    assert!(!model.agenda.is_empty());
    assert!(
        model
            .agenda
            .iter()
            .filter(|instance| instance.status != vnf_core::types::InstanceStatus::NotApplicable)
            .all(|instance| instance.provisional),
        "every date in a year without a pack is provisional"
    );
    Ok(())
}

#[test]
fn a_refused_pack_leaves_the_window_open_on_the_pack_it_can_trust() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    let pack = dir.path().join("partido.json");
    std::fs::write(&pack, "{\"jurisdiction\":\"PT\",\"year\":2027")?;

    let session = session_for(dir.path(), Some(&pack), 2027)?;
    assert_eq!(session.pack_origin(), PackOrigin::Embedded);

    let model = session.reload("2027-01-10", 2027)?;
    assert_eq!(model.pack.rejected.len(), 1);
    let refused = model.pack.rejected.first().ok_or("the refusal must be kept")?;
    assert!(refused.path.ends_with("partido.json"), "path {}", refused.path);
    // The refusal is a sentence the Regras page can print, not a bare code.
    assert!(refused.problem.contains("JSON"), "problem {}", refused.problem);
    // And the law is still there: the window did not open empty-handed.
    assert_eq!(model.pack.origin, PackOrigin::Embedded);
    assert_eq!(model.pack.summary.obligations, 20);
    assert!(!model.pack.exact);
    Ok(())
}
