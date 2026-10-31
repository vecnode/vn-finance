//! A vault written by the TypeScript application, opened by this one.
//!
//! `crates/store/tests/fixtures/vault-ts/` was produced by the **reference
//! implementation's own `Vault` class** (see `tools/golden/vault.ts`), not by a
//! hand-written imitation of its format. Opening it is the claim a migration has to
//! support: a folder somebody has been using for a year works in the new
//! application, with its ledger, its completions, its archive and its audit trail
//! intact.
//!
//! Every value in the fixture is invented. Nothing there is anybody's data.

use std::path::{Path, PathBuf};

use vnf_core::rules::load_embedded;
use vnf_store::documents::DocumentMeta;
use vnf_store::location::VaultLocationSource;
use vnf_store::{ModelConfig, Vault, build_model};

type Problem = Box<dyn std::error::Error>;

/// The committed fixture. Read-only: a test that wrote here would dirty the repo.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("vault-ts")
}

#[test]
fn a_vault_written_by_the_reference_opens_here() -> Result<(), Problem> {
    let vault = Vault::new(fixture());

    let profile = vault.load_profile()?.ok_or("the fixture has a profile")?;
    assert_eq!(profile.nif, "123456789");
    assert_eq!(profile.name, "Pessoa Exemplo");
    assert_eq!(profile.activity.start_date, "2019-04-01");
    assert_eq!(profile.activity.turnover_previous_year_cents, Some(500_000));
    assert_eq!(profile.tracking_start.as_deref(), Some("2026-01-01"));
    assert_eq!(profile.irs.coefficient_bp, 7500);

    let invoices = vault.load_invoices()?;
    assert_eq!(
        invoices.len(),
        2,
        "the ledger is append-only and both lines are there"
    );
    assert_eq!(invoices[0].number, "FR 2026/1");
    assert_eq!(invoices[0].client_nif.as_deref(), Some("501234560"));
    assert_eq!(invoices[0].atcud.as_deref(), Some("ABCD1234-1"));
    assert_eq!(invoices[1].number, "FR 2026/2");
    assert_eq!(invoices[1].client_country, "FI");
    assert_eq!(
        invoices[1].client_nif, None,
        "a client with no NIF is null, not empty"
    );

    let completions = vault.completions()?;
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].id, "iva.dp.trimestral@2026-05-20");
    assert_eq!(completions[0].note.as_deref(), Some("submetida no portal"));
    assert_eq!(
        vault.completed_ids()?,
        vec!["iva.dp.trimestral@2026-05-20".to_owned()]
    );

    let audit = vault.audit()?;
    let actions: Vec<&str> = audit.iter().map(|event| event.action.as_str()).collect();
    assert_eq!(
        actions,
        vec![
            "invoice.recorded",
            "invoice.recorded",
            "obligation.completed",
            "document.added",
            "vault.opened",
        ]
    );
    // A line with no detail omits the key; a line with one keeps it.
    assert_eq!(audit[0].detail.as_deref(), Some("FR 2026/1"));

    let index = vault.document_index()?;
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].kind, "fatura");
    assert_eq!(index[0].obligation_id.as_deref(), Some("iva.dp.trimestral"));
    assert_eq!(index[0].invoice_id.as_deref(), Some("inv_legado_1"));
    assert_eq!(
        index[0].original_path, None,
        "bytes arrived with no path behind them"
    );
    assert_eq!(
        vault.documented_obligation_ids()?,
        vec!["iva.dp.trimestral".to_owned()]
    );

    // The archive is addressable and its bytes still hash to what the index says.
    let found = vault
        .find_document(&index[0].file)?
        .ok_or("the index names a stored file")?;
    assert!(found.absolute.is_file(), "the archived copy is on disk");
    assert_eq!(vault.hash_file(&found.absolute)?, index[0].sha256);
    assert_eq!(
        vault.hash_bytes(&std::fs::read(&found.absolute)?),
        index[0].sha256
    );
    let by_hash = vault
        .find_document_by_hash(&index[0].sha256)?
        .ok_or("findable by content")?;
    assert_eq!(by_hash.invoice_id.as_deref(), Some("inv_legado_1"));

    let stats = vault.describe()?;
    assert!(
        stats.files >= 6,
        "profile, two ledgers, index, document, audit log"
    );
    assert!(stats.bytes > 0);
    Ok(())
}

#[test]
fn the_reference_vault_builds_a_model() -> Result<(), Problem> {
    let vault = Vault::new(fixture());
    let pack = load_embedded()?;
    let model = build_model(
        &vault,
        &ModelConfig {
            year: 2026,
            today: "2026-09-20".to_owned(),
            version: "0.2.0".to_owned(),
            runtime: "teste".to_owned(),
            pack: &pack,
            data_dir_source: VaultLocationSource::Pointer,
            pointer_path: PathBuf::new(),
        },
    )?;

    assert_eq!(
        model.profile.as_ref().map(|profile| profile.nif.as_str()),
        Some("123456789")
    );
    assert_eq!(model.invoices.len(), 2);
    assert!(
        !model.agenda.is_empty(),
        "a profile with a tracking start has obligations"
    );
    assert_eq!(model.quarters.len(), 4);
    // February's invoice in Q1, May's in Q2.
    assert_eq!(model.quarters[0].totals.base_cents, 120_000);
    assert_eq!(model.quarters[1].totals.base_cents, 350_000);
    assert_eq!(model.quarters[2].totals.base_cents, 0);
    // The two invoices are 4 700,00 EUR of national + EU turnover.
    assert_eq!(model.reserve.low_cents, 117_500);
    assert_eq!(model.vault.entries.len(), 1);
    assert!(model.meta.vault_in_use);
    assert_eq!(model.meta.pack_checksum, pack.checksum);
    assert!(
        model
            .agenda
            .iter()
            .any(|instance| instance.status == vnf_core::types::InstanceStatus::Done),
        "the completion the fixture recorded is reflected in the agenda"
    );
    Ok(())
}

#[test]
fn this_application_writes_the_same_shapes_it_reads() -> Result<(), Problem> {
    // Work on a copy: the fixture itself is read-only.
    let temporary = tempfile::tempdir()?;
    let copy = temporary.path().join("vault");
    copy_tree(&fixture(), &copy)?;
    let vault = Vault::new(&copy);

    let invoices_before = vault.load_invoices()?.len();
    let audit_before = vault.audit()?.len();
    let mut invoice = vault.load_invoices()?[0].clone();
    invoice.id = "inv_novo".to_owned();
    invoice.number = "FR 2026/3".to_owned();
    vault.append_invoice(&invoice, "2026-09-20T10:00:00.000Z")?;
    assert_eq!(vault.load_invoices()?.len(), invoices_before + 1);
    assert_eq!(vault.audit()?.len(), audit_before + 1);

    // The profile round-trips through the same JSON the reference wrote.
    let profile = vault.load_profile()?.ok_or("profile")?;
    vault.save_profile(&profile)?;
    assert_eq!(vault.load_profile()?, Some(profile));

    // The file shapes the reference relies on: one compact JSON object per line,
    // and a pretty-printed profile with a trailing newline.
    let ledger = std::fs::read_to_string(vault.path(vnf_store::location::LEDGER_INVOICES))?;
    assert_eq!(ledger.lines().count(), invoices_before + 1);
    for line in ledger.lines() {
        assert!(
            line.starts_with('{') && line.ends_with('}'),
            "one object per line: {line}"
        );
        assert!(
            !line.contains(": "),
            "JSONL lines are compact, as the reference writes them"
        );
    }
    let profile_text = std::fs::read_to_string(vault.path(vnf_store::location::PROFILE_FILE))?;
    assert!(profile_text.ends_with("}\n"));
    assert!(
        profile_text.contains("\n  \"nif\": \"123456789\""),
        "two-space indent"
    );
    Ok(())
}

#[test]
fn the_document_index_survives_a_new_entry_without_losing_the_old_one() -> Result<(), Problem> {
    let temporary = tempfile::tempdir()?;
    let copy = temporary.path().join("vault");
    copy_tree(&fixture(), &copy)?;
    let vault = Vault::new(&copy);

    vault.add_document_bytes(
        b"%PDF-1.7 segundo",
        "outro.pdf",
        DocumentMeta {
            kind: Some("declaracao"),
            obligation_id: None,
            invoice_id: None,
        },
        "2026-09-20T10:00:00.000Z",
    )?;
    let index = vault.document_index()?;
    assert_eq!(index.len(), 2, "the entry the reference wrote is still there");
    assert_eq!(index[0].invoice_id.as_deref(), Some("inv_legado_1"));
    assert_eq!(index[1].kind, "declaracao");
    Ok(())
}

/// Copy a directory tree, so a test can write without touching the fixture.
fn copy_tree(from: &Path, to: &Path) -> Result<(), Problem> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
