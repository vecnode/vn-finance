//! The view model.
//!
//! One function assembles everything the window shows, from the same core the
//! command line would use. It exists so that the interface layer contains no tax
//! logic whatsoever: every number it displays is computed here, by the deterministic
//! core, and the interface only decides how to lay it out.
//!
//! The model is deliberately one value rather than a dozen queries. A local
//! application does not have a network problem to solve, and a single consistent
//! snapshot means the window can never show an agenda from one moment beside flags
//! from another.
//!
//! Two blocks the reference carried are gone with the assistant: `update` (the
//! rule-pack proposal state) and `key` (the masked credential). Nothing computes
//! them, nothing shows them, and keeping them would keep a promise this build does
//! not make.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use vnf_core::calendar::build_agenda;
use vnf_core::dates::DateError;
use vnf_core::estimate::{Invoice, QuarterReport, ReserveEstimate, estimate_reserve, quarter_report};
use vnf_core::flags::{Flag, FlagInput, FlagSummary, compute_flags, pack_health_flags, summarise_flags};
use vnf_core::profile::{ProfileProblem, default_cae, missing_profile_inputs, validate_profile};
use vnf_core::rules::{Freshness, PackSummary, constant_bp, freshness, summarise_pack};
use vnf_core::types::{
    AgendaOptions, CaeEntry, IrsRegime, IvaRegime, LoadedPack, ObligationInstance, ObligationRule,
    PackProblem, RuleSource, TaxProfile,
};

use crate::StoreError;
use crate::documents::DocumentEntry;
use crate::location::{VaultLocationSource, looks_like_vault};
use crate::risk::{DataDirRisk, check_data_dir_risk};
use crate::vault::Vault;

/// What this application never does, in one place, so the interface and the
/// documentation cannot drift apart on the promise.
///
/// The reference's fifth sentence was about the rule-pack update request. This build
/// has no network code at all, so the honest sentence is stronger.
pub const GUARANTEES: [&str; 6] = [
    "Não fala com o Portal das Finanças, com a Segurança Social, com o VIES nem com qualquer outro \
     serviço público.",
    "Não faz login em lado nenhum, não recolhe páginas e não automatiza portais.",
    "Não entrega declarações nem paga nada por ti.",
    "Não sincroniza: os dados vivem numa só pasta, neste computador.",
    "Não abre uma única ligação à Internet: esta aplicação não tem código de rede.",
    "Não substitui um contabilista certificado.",
];

/// Why the model could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// The vault could not be read.
    Store(StoreError),
    /// A date the computation needed is not an ISO date.
    Date(DateError),
}

impl core::fmt::Display for ModelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Date(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<StoreError> for ModelError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<DateError> for ModelError {
    fn from(error: DateError) -> Self {
        Self::Date(error)
    }
}

/// Everything the model needs that is not in the vault.
pub struct ModelConfig<'a> {
    /// The civil year to report on.
    pub year: i64,
    /// The date the person believes it is today.
    pub today: String,
    /// The application's version.
    pub version: String,
    /// The runtime line the Diagnóstico page shows.
    pub runtime: String,
    /// The rule pack in use.
    pub pack: &'a LoadedPack,
    /// Where the vault folder came from.
    pub data_dir_source: VaultLocationSource,
    /// The pointer file that remembers a chosen folder.
    pub pointer_path: PathBuf,
}

/// A labelled choice the profile form offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// The value stored in the profile.
    pub value: String,
    /// What the form calls it.
    pub label: String,
}

/// The closed vocabularies the profile form offers.
///
/// They travel in the model for the same reason the tax rates do: a select box is a
/// statement about what the AT accepts, and the interface must not be the place
/// where that list lives. The labels are here too, so a regime cannot be called two
/// different things in two places.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileOptions {
    /// The IVA regimes, in the order the AT lists them.
    pub iva_regimes: Vec<Choice>,
    /// The IRS regimes.
    pub irs_regimes: Vec<Choice>,
    /// The CAE the form offers when nobody names one.
    pub default_cae: CaeEntry,
}

/// The rates an interface needs for a sensible default, from the pack and never
/// from a literal in the interface.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardDefaults {
    /// The normal IVA rate.
    pub iva_normal_bp: Option<i64>,
    /// The intermediate IVA rate.
    pub iva_intermediate_bp: Option<i64>,
    /// The reduced IVA rate.
    pub iva_reduced_bp: Option<i64>,
    /// The resident professional-services withholding rate.
    pub withholding_resident_bp: Option<i64>,
    /// The non-resident rate.
    pub withholding_non_resident_bp: Option<i64>,
}

/// Facts about the folder the application is looking at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardMeta {
    /// The application's version.
    pub version: String,
    /// The date the model was built for.
    pub today: String,
    /// The civil year.
    pub year: i64,
    /// The vault folder.
    pub data_dir: String,
    /// The pack's own version.
    pub pack_version: String,
    /// The pack's checksum.
    pub pack_checksum: String,
    /// Where the pack came from.
    pub pack_path: String,
    /// The runtime line the Diagnóstico page shows.
    pub runtime: String,
    /// Whether the folder sits in a repository it should not.
    pub data_dir_risk: DataDirRisk,
    /// Where the folder came from.
    pub data_dir_source: VaultLocationSource,
    /// The pointer file that remembers a chosen folder.
    pub data_dir_pointer_path: String,
    /// Whether the folder already holds anything.
    pub vault_in_use: bool,
}

/// The pack, as the interface reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardPack {
    /// The counts.
    pub summary: PackSummary,
    /// Whether the pack covers the year in course.
    pub freshness: Freshness,
    /// Everything the validator noticed.
    pub problems: Vec<PackProblem>,
}

/// One archived document, as the Cofre shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntry {
    /// The stored name.
    pub file: String,
    /// The content hash.
    pub sha256: String,
    /// When it was archived.
    pub added_at: String,
    /// What kind of document it is.
    pub kind: String,
    /// The obligation it documents.
    pub obligation_id: Option<String>,
    /// The invoice it became.
    pub invoice_id: Option<String>,
}

/// What is archived, and how much.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultView {
    /// The index entries.
    pub entries: Vec<VaultEntry>,
    /// How many files the folder holds.
    pub files: usize,
    /// Their total size.
    pub bytes: u64,
}

/// The law the interface cites.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesView {
    /// The sources the pack cites.
    pub sources: Vec<RuleSource>,
    /// The obligations it declares.
    pub obligations: Vec<ObligationRule>,
    /// What the maintainers know is unfinished.
    pub todo: Vec<String>,
}

/// The whole picture, in one value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardModel {
    /// Facts about the folder and the runtime.
    pub meta: DashboardMeta,
    /// What the application never does.
    pub guarantees: Vec<String>,
    /// The rates a new invoice starts from.
    pub defaults: DashboardDefaults,
    /// The closed vocabularies the profile form offers.
    pub profile_options: ProfileOptions,
    /// The stored profile, when there is one.
    pub profile: Option<TaxProfile>,
    /// What is wrong with it.
    pub profile_problems: Vec<ProfileProblem>,
    /// The inputs only the person can supply.
    pub missing_inputs: Vec<String>,
    /// The pack.
    pub pack: DashboardPack,
    /// What is due.
    pub agenda: Vec<ObligationInstance>,
    /// What is worth looking at.
    pub flags: Vec<Flag>,
    /// How many flags of each severity.
    pub flag_summary: FlagSummary,
    /// The four quarters of the year.
    pub quarters: Vec<QuarterReport>,
    /// How much cash to keep aside.
    pub reserve: ReserveEstimate,
    /// The ledger.
    pub invoices: Vec<Invoice>,
    /// The archive.
    pub vault: VaultView,
    /// The law the figures rest on.
    pub rules: RulesView,
}

/// Assemble the model.
///
/// With no profile there is nothing to schedule, alert or estimate: the interface
/// shows onboarding rather than a screen full of zeros pretending to be data.
///
/// # Errors
///
/// [`ModelError`] when the vault cannot be read or a date it holds is not an ISO date.
pub fn build_model(vault: &Vault, config: &ModelConfig<'_>) -> Result<DashboardModel, ModelError> {
    let pack = &config.pack.pack;
    let profile = vault.load_profile()?;
    let invoices = vault.load_invoices()?;
    let summary = summarise_pack(config.pack);

    let agenda = match &profile {
        None => Vec::new(),
        Some(profile) => build_agenda(
            pack,
            profile,
            &AgendaOptions {
                year: config.year,
                today: config.today.clone(),
                due_soon_days: Some(30),
                completed_ids: vault.completed_ids()?,
                track_from: profile.tracking_start.clone(),
                holidays: Vec::new(),
            },
        )?,
    };

    let flags = match &profile {
        None => pack_health_flags(pack, &config.today)?,
        Some(profile) => {
            let mut flags = compute_flags(FlagInput {
                pack,
                profile,
                invoices: &invoices,
                instances: &agenda,
                today: config.today.clone(),
                documented_obligation_ids: vault.documented_obligation_ids()?,
                due_soon_days: Some(30),
            })?;
            flags.extend(pack_health_flags(pack, &config.today)?);
            flags
        }
    };
    let flag_summary = summarise_flags(&flags);

    let quarters = [1_i64, 2, 3, 4]
        .into_iter()
        .map(|quarter| quarter_report(pack, &invoices, config.year, quarter))
        .collect::<Result<Vec<QuarterReport>, DateError>>()?;
    let reserve = estimate_reserve(pack, &invoices, &config.today, 2500, 3500);

    let stats = vault.describe()?;
    let entries: Vec<VaultEntry> = vault.document_index()?.into_iter().map(vault_entry).collect();

    let meta = DashboardMeta {
        version: config.version.clone(),
        today: config.today.clone(),
        year: config.year,
        data_dir: vault.dir().display().to_string(),
        pack_version: pack.pack_version.clone(),
        pack_checksum: config.pack.checksum.clone(),
        pack_path: config.pack.path.clone(),
        runtime: config.runtime.clone(),
        data_dir_risk: check_data_dir_risk(vault.dir()),
        data_dir_source: config.data_dir_source,
        data_dir_pointer_path: vault_pointer_path_display(config),
        vault_in_use: looks_like_vault(vault.dir()) || profile.is_some() || !invoices.is_empty(),
    };

    Ok(DashboardModel {
        meta,
        guarantees: GUARANTEES.iter().map(|line| (*line).to_owned()).collect(),
        defaults: DashboardDefaults {
            iva_normal_bp: constant_bp(pack, &["iva", "rates", "normal"]),
            iva_intermediate_bp: constant_bp(pack, &["iva", "rates", "intermediaire"]),
            iva_reduced_bp: constant_bp(pack, &["iva", "rates", "reduzida"]),
            withholding_resident_bp: constant_bp(
                pack,
                &["irs", "withholding", "professionalServicesResident"],
            ),
            withholding_non_resident_bp: constant_bp(pack, &["irs", "withholding", "nonResident"]),
        },
        profile_options: profile_options(),
        profile_problems: profile
            .as_ref()
            .map(|profile| validate_profile(profile, &config.today))
            .unwrap_or_default(),
        missing_inputs: profile.as_ref().map(missing_profile_inputs).unwrap_or_default(),
        profile,
        pack: DashboardPack {
            summary,
            freshness: freshness(pack, &config.today)?,
            problems: config.pack.problems.clone(),
        },
        agenda,
        flags,
        flag_summary,
        quarters,
        reserve,
        invoices,
        vault: VaultView {
            entries,
            files: stats.files,
            bytes: stats.bytes,
        },
        rules: RulesView {
            sources: pack.sources.clone(),
            obligations: pack.obligations.clone(),
            todo: pack.todo.clone(),
        },
    })
}

/// The pointer path, as a string for the model.
fn vault_pointer_path_display(config: &ModelConfig<'_>) -> String {
    if config.pointer_path.as_os_str().is_empty() {
        String::new()
    } else {
        config.pointer_path.display().to_string()
    }
}

/// Turn one index entry into the shape the Cofre shows.
fn vault_entry(entry: DocumentEntry) -> VaultEntry {
    VaultEntry {
        file: entry.file,
        sha256: entry.sha256,
        added_at: entry.added_at,
        kind: entry.kind,
        obligation_id: entry.obligation_id,
        invoice_id: entry.invoice_id,
    }
}

/// The choices the profile form offers.
#[must_use]
pub fn profile_options() -> ProfileOptions {
    ProfileOptions {
        iva_regimes: vec![
            Choice {
                value: regime_name(IvaRegime::IsentoArt53).to_owned(),
                label: "Isenção do art. 53.º do CIVA".to_owned(),
            },
            Choice {
                value: regime_name(IvaRegime::Trimestral).to_owned(),
                label: "Regime normal — declaração trimestral".to_owned(),
            },
            Choice {
                value: regime_name(IvaRegime::Mensal).to_owned(),
                label: "Regime normal — declaração mensal".to_owned(),
            },
        ],
        irs_regimes: vec![
            Choice {
                value: irs_regime_name(IrsRegime::Simplificado).to_owned(),
                label: "Regime simplificado".to_owned(),
            },
            Choice {
                value: irs_regime_name(IrsRegime::Organizada).to_owned(),
                label: "Contabilidade organizada".to_owned(),
            },
        ],
        default_cae: default_cae(),
    }
}

/// The wire name of an IVA regime.
fn regime_name(regime: IvaRegime) -> &'static str {
    match regime {
        IvaRegime::IsentoArt53 => "isento_art53",
        IvaRegime::Trimestral => "trimestral",
        IvaRegime::Mensal => "mensal",
    }
}

/// The wire name of an IRS regime.
fn irs_regime_name(regime: IrsRegime) -> &'static str {
    match regime {
        IrsRegime::Simplificado => "simplificado",
        IrsRegime::Organizada => "organizada",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnf_core::estimate::{InvoiceStatus, VatTreatment};
    use vnf_core::rules::load_embedded;

    type Problem = Box<dyn std::error::Error>;

    const TODAY: &str = "2026-09-20";

    fn config<'a>(pack: &'a LoadedPack) -> ModelConfig<'a> {
        ModelConfig {
            year: 2026,
            today: TODAY.to_owned(),
            version: "0.2.0".to_owned(),
            runtime: "teste".to_owned(),
            pack,
            data_dir_source: VaultLocationSource::Default,
            pointer_path: PathBuf::from("/tmp/vault-location.json"),
        }
    }

    fn profile() -> Result<TaxProfile, Problem> {
        Ok(vnf_core::profile::create_default_profile(
            &vnf_core::profile::ProfileInput {
                nif: "123456789".to_owned(),
                name: "Pessoa Exemplo".to_owned(),
                start_date: Some("2019-04-01".to_owned()),
                iva_regime: Some(IvaRegime::Trimestral),
                ..Default::default()
            },
        )?)
    }

    fn invoice(number: &str, date: &str) -> Invoice {
        Invoice {
            id: format!("inv_{number}"),
            number: number.to_owned(),
            date: date.to_owned(),
            client_name: "Cliente Exemplo".to_owned(),
            client_nif: Some("501234560".to_owned()),
            client_country: "PT".to_owned(),
            description: "Consultoria".to_owned(),
            base_cents: 100_000,
            iva_rate_bp: 2300,
            vat_treatment: VatTreatment::IvaPt,
            retention_bp: 2300,
            atcud: None,
            status: InvoiceStatus::Issued,
            payment_proof_in_vault: false,
        }
    }

    #[test]
    fn an_empty_vault_shows_onboarding_rather_than_zeros() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let pack = load_embedded()?;
        let model = build_model(&vault, &config(&pack))?;
        assert_eq!(model.profile, None);
        assert!(model.agenda.is_empty(), "no profile means no agenda at all");
        assert!(model.profile_problems.is_empty());
        assert!(model.missing_inputs.is_empty());
        // The four quarters and the reserve still exist, at zero: the pages that
        // show them can render, they just have nothing to show.
        assert_eq!(model.quarters.len(), 4);
        assert!(
            model
                .quarters
                .iter()
                .all(|quarter| quarter.totals.base_cents == 0)
        );
        assert_eq!(model.reserve.low_cents, 0);
        assert!(!model.meta.vault_in_use, "an empty folder is not in use");
        // And the only complaint is the pack's own health, not the person's.
        assert_eq!(model.flag_summary.total, 0);
        Ok(())
    }

    #[test]
    fn a_filled_vault_assembles_the_whole_picture() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        vault.save_profile(&profile()?)?;
        vault.append_invoice(&invoice("FR 1", "2026-02-01"), TODAY)?;
        vault.append_invoice(&invoice("FR 2", "2026-05-01"), TODAY)?;
        let pack = load_embedded()?;
        let model = build_model(&vault, &config(&pack))?;

        assert!(model.profile.is_some());
        assert_eq!(model.invoices.len(), 2);
        assert!(!model.agenda.is_empty());
        assert_eq!(model.quarters.len(), 4);
        // Two invoices in Q1 and Q2, both at 1 000,00 EUR.
        assert_eq!(model.quarters[0].totals.base_cents, 100_000);
        assert_eq!(model.quarters[1].totals.base_cents, 100_000);
        assert_eq!(model.quarters[2].totals.base_cents, 0);
        assert_eq!(model.reserve.low_cents, 50_000);
        assert!(model.meta.vault_in_use);
        assert_eq!(model.meta.pack_checksum, pack.checksum);
        assert_eq!(model.rules.obligations.len(), 20);
        Ok(())
    }

    #[test]
    fn the_defaults_come_from_the_pack_and_are_never_literals() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let pack = load_embedded()?;
        let model = build_model(&vault, &config(&pack))?;
        assert_eq!(
            model.defaults.iva_normal_bp,
            constant_bp(&pack.pack, &["iva", "rates", "normal"])
        );
        assert_eq!(
            model.defaults.withholding_resident_bp,
            constant_bp(
                &pack.pack,
                &["irs", "withholding", "professionalServicesResident"]
            )
        );
        Ok(())
    }

    #[test]
    fn the_profile_options_are_the_closed_vocabularies() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let pack = load_embedded()?;
        let model = build_model(&vault, &config(&pack))?;
        let iva: Vec<&str> = model
            .profile_options
            .iva_regimes
            .iter()
            .map(|c| c.value.as_str())
            .collect();
        assert_eq!(iva, vec!["isento_art53", "trimestral", "mensal"]);
        let irs: Vec<&str> = model
            .profile_options
            .irs_regimes
            .iter()
            .map(|c| c.value.as_str())
            .collect();
        assert_eq!(irs, vec!["simplificado", "organizada"]);
        assert_eq!(model.profile_options.default_cae.code, "62010");
        Ok(())
    }

    #[test]
    fn a_completed_obligation_reaches_the_model() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        vault.save_profile(&profile()?)?;
        let pack = load_embedded()?;
        let before = build_model(&vault, &config(&pack))?;
        let target = before
            .agenda
            .iter()
            .find(|instance| instance.status == vnf_core::types::InstanceStatus::Overdue)
            .ok_or("the fixture must have an overdue instance")?
            .id
            .clone();
        vault.mark_completed(&target, "iva.dp.trimestral", "2026-05-20", None, TODAY)?;
        let after = build_model(&vault, &config(&pack))?;
        let marked = after
            .agenda
            .iter()
            .find(|instance| instance.id == target)
            .ok_or("the instance must still be listed")?;
        assert_eq!(marked.status, vnf_core::types::InstanceStatus::Done);
        Ok(())
    }

    #[test]
    fn a_document_reaches_the_cofre_view() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let archived = vault.add_document_bytes(
            b"%PDF",
            "fatura.pdf",
            crate::documents::DocumentMeta {
                kind: Some("fatura"),
                obligation_id: Some("iva.dp.trimestral"),
                invoice_id: None,
            },
            TODAY,
        )?;
        let pack = load_embedded()?;
        let model = build_model(&vault, &config(&pack))?;
        assert_eq!(model.vault.entries.len(), 1);
        assert_eq!(model.vault.entries[0].file, archived.file);
        assert_eq!(model.vault.entries[0].kind, "fatura");
        assert_eq!(
            model.vault.entries[0].obligation_id.as_deref(),
            Some("iva.dp.trimestral")
        );
        assert!(model.vault.files >= 2, "the document and the index");
        assert!(model.vault.bytes > 0);
        Ok(())
    }

    #[test]
    fn the_model_serialises_with_the_names_the_interface_reads() -> Result<(), Problem> {
        let dir = tempfile::tempdir()?;
        let vault = Vault::new(dir.path());
        let pack = load_embedded()?;
        let model = build_model(&vault, &config(&pack))?;
        let value = serde_json::to_value(&model)?;
        // The four blocks a page reads first.
        assert!(value.get("meta").is_some());
        assert!(value.get("profile").is_some(), "null is a value, not an absence");
        assert!(value.get("quarters").is_some());
        assert!(value.get("flagSummary").is_some());
        assert!(value.get("rules").is_some());
        assert_eq!(value["meta"]["dataDirSource"].as_str(), Some("default"));
        assert_eq!(value["meta"]["dataDirRisk"]["level"].as_str(), Some("none"));
        assert!(
            value["guarantees"]
                .as_array()
                .is_some_and(|lines| lines.len() == 6)
        );
        Ok(())
    }
}
