//! Diagnóstico: the profile, the vault and the pack, side by side.
//!
//! The command line this replaces had a `doctor` command, and the panel had a page
//! that answered the same questions without a terminal. That page is this one: when
//! something looks wrong, everything that decides what the application believes is on
//! one screen, with the value it is using.

use egui::Ui;
use vnf_core::money::format_bp_as_coefficient;
use vnf_core::types::{CaeRole, IrsRegime, IvaRegime};
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Def, Tone};

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel) -> Option<Action> {
    let mut action = None;
    widgets::section(
        ui,
        "Diagnóstico",
        Some(&format!(
            "vn-finance {} · {}",
            model.meta.version, model.meta.runtime
        )),
        Some(
            "Tudo o que a aplicação assume, num sítio. Se um número noutra página parecer errado, o \
             valor que o produziu está aqui.",
        ),
    );

    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Preencher o perfil").clicked() {
            action = Some(Action::OpenProfile);
        }
        if widgets::button(ui, "Recarregar").clicked() {
            action = Some(Action::Reload);
        }
    });
    ui.add_space(12.0);

    // Bound before the rows so the table can borrow them for as long as it needs.
    let principal = model
        .profile
        .as_ref()
        .and_then(|profile| {
            profile
                .activity
                .cae
                .iter()
                .find(|entry| entry.role == CaeRole::Principal)
                .map(|entry| format!("{} — {}", entry.code, entry.description))
        })
        .unwrap_or_else(|| "—".to_owned());
    let coefficient = model.profile.as_ref().map(|profile| {
        format!(
            "coeficiente {}",
            format_bp_as_coefficient(profile.irs.coefficient_bp)
        )
    });

    let profile_rows = match &model.profile {
        Some(profile) => vec![
            Def {
                label: "NIF",
                value: &profile.nif,
                small: None,
            },
            Def {
                label: "Nome",
                value: &profile.name,
                small: None,
            },
            Def {
                label: "Natureza",
                value: if profile.is_company {
                    "pessoa coletiva"
                } else {
                    "pessoa singular"
                },
                small: None,
            },
            Def {
                label: "Residência fiscal",
                value: if profile.resident_pt {
                    "Portugal"
                } else {
                    "fora de Portugal"
                },
                small: None,
            },
            Def {
                label: "CAE principal",
                value: &principal,
                small: None,
            },
            Def {
                label: "Início de atividade",
                value: &profile.activity.start_date,
                small: profile.tracking_start.as_deref(),
            },
            Def {
                label: "Regime de IVA",
                value: iva_label(profile.iva.regime),
                small: None,
            },
            Def {
                label: "Regime de IRS",
                value: irs_label(profile.irs.regime),
                small: coefficient.as_deref(),
            },
        ],
        None => Vec::new(),
    };

    if profile_rows.is_empty() {
        widgets::empty(
            ui,
            "Sem perfil neste cofre",
            "Sem os dados da declaração de início de atividade, a aplicação não consegue avaliar \
             que obrigações se aplicam. Usa «Preencher o perfil» para começar.",
        );
    } else {
        widgets::section(ui, "Perfil", Some("a tua declaração, não uma inferência"), None);
        widgets::card(ui, |ui| widgets::defs(ui, &profile_rows));
    }

    if !model.profile_problems.is_empty() {
        ui.add_space(12.0);
        widgets::card(ui, |ui| {
            widgets::micro_label(ui, "problemas no perfil", theme::palette(ui.ctx()).text_3);
            for problem in &model.profile_problems {
                let tone = if problem.level == vnf_core::profile::ProblemLevel::Error {
                    Tone::Danger
                } else {
                    Tone::Warn
                };
                ui.horizontal_wrapped(|ui| {
                    widgets::severity(
                        ui,
                        if problem.level == vnf_core::profile::ProblemLevel::Error {
                            "erro"
                        } else {
                            "aviso"
                        },
                        tone,
                    );
                    ui.label(egui::RichText::new(&problem.message).size(theme::BODY_SMALL));
                });
            }
        });
    }

    ui.add_space(16.0);
    widgets::section(ui, "Cofre", None, None);
    let summary = &model.pack.summary;
    let stats = format!("{} ficheiro(s)", model.vault.files);
    let bytes = crate::pages::cofre::bytes(model.vault.bytes);
    let in_use = if model.meta.vault_in_use {
        "sim"
    } else {
        "não (vazio)"
    };
    let risk = format!("{:?}", model.meta.data_dir_risk.level);
    let entries = model.vault.entries.len().to_string();
    widgets::card(ui, |ui| {
        widgets::defs(
            ui,
            &[
                Def {
                    label: "Pasta",
                    value: &model.meta.data_dir,
                    small: None,
                },
                Def {
                    label: "Conteúdo",
                    value: &stats,
                    small: Some(&bytes),
                },
                Def {
                    label: "Em uso",
                    value: in_use,
                    small: None,
                },
                Def {
                    label: "Documentos no índice",
                    value: &entries,
                    small: None,
                },
                Def {
                    label: "Risco de localização",
                    value: &risk,
                    small: model.meta.data_dir_risk.message.as_deref(),
                },
                Def {
                    label: "Hoje",
                    value: &model.meta.today,
                    small: Some("o dia que a agenda usa, em Europe/Lisbon"),
                },
            ],
        );
    });

    ui.add_space(16.0);
    widgets::section(ui, "Pacote de regras", None, None);
    let pack_version = summary.pack_version.clone();
    let pack_year = summary.year.to_string();
    let pack_sources = summary.sources.to_string();
    let pack_obligations = summary.obligations.to_string();
    let verified = format!("{} verificadas · {} parciais", summary.verified, summary.partial);
    widgets::card(ui, |ui| {
        widgets::defs(
            ui,
            &[
                Def {
                    label: "Versão",
                    value: &pack_version,
                    small: Some(&pack_year),
                },
                Def {
                    label: "Checksum",
                    value: &model.meta.pack_checksum,
                    small: None,
                },
                Def {
                    label: "Origem",
                    value: &model.meta.pack_path,
                    small: None,
                },
                Def {
                    label: "Fontes",
                    value: &pack_sources,
                    small: Some(&verified),
                },
                Def {
                    label: "Obrigações",
                    value: &pack_obligations,
                    small: None,
                },
                Def {
                    label: "Estado",
                    value: &model.pack.freshness.message,
                    small: None,
                },
            ],
        );
    });

    if !model.missing_inputs.is_empty() {
        ui.add_space(12.0);
        widgets::section(
            ui,
            "O que falta",
            Some("sem isto, há verificações que não correm"),
            None,
        );
        widgets::notes(ui, Tone::Warn, &model.missing_inputs);
    }

    ui.add_space(12.0);
    widgets::footnote(
        ui,
        "Este diagnóstico é o mesmo que a versão anterior produzia no terminal: lê o perfil, o \
         cofre e o pacote, e não escreve nada. Nada aqui é enviado para lado nenhum.",
    );
    action
}

fn iva_label(regime: IvaRegime) -> &'static str {
    match regime {
        IvaRegime::IsentoArt53 => "Isenção do art. 53.º do CIVA",
        IvaRegime::Trimestral => "Regime normal — trimestral",
        IvaRegime::Mensal => "Regime normal — mensal",
    }
}

fn irs_label(regime: IrsRegime) -> &'static str {
    match regime {
        IrsRegime::Simplificado => "Regime simplificado",
        IrsRegime::Organizada => "Contabilidade organizada",
    }
}
