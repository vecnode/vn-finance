//! The law: every source the pack cites, and every obligation it declares.
//!
//! This page exists so that no figure elsewhere in the application is unfalsifiable.
//! A number on the IVA page traces back to a constant, the constant to a source, and
//! the source to a URL with a retrieval date.

use egui::Ui;
use vnf_core::money::format_bp_as_percent;
use vnf_core::rules::FreshnessStatus;
use vnf_core::types::{PeriodicityType, SourceTier, Verification};
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Column, Kpi, Tone};

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel) -> Option<Action> {
    let action = None;
    let summary = &model.pack.summary;
    widgets::section(
        ui,
        "Regras & fontes",
        Some(&format!("pacote pt/{} · {}", summary.pack_version, summary.year)),
        Some(
            "Esta aplicação não sabe direito fiscal: sabe ler um pacote de regras que diz onde cada \
             taxa, prazo e limite está escrito. Aqui está tudo o que o pacote afirma, e a fonte de \
             cada afirmação.",
        ),
    );

    let version = summary.pack_version.clone();
    let year = summary.year.to_string();
    let sources = summary.sources.to_string();
    let obligations = summary.obligations.to_string();
    let verified = format!("{} de {}", summary.verified, summary.obligations);
    widgets::kpi_row(
        ui,
        &[
            Kpi {
                label: "Versão do pacote",
                value: &version,
                support: None,
                foot: Some("embutido"),
            },
            Kpi {
                label: "Ano",
                value: &year,
                support: None,
                foot: None,
            },
            Kpi {
                label: "Fontes",
                value: &sources,
                support: None,
                foot: Some(&format!("{} oficiais", summary.official_sources)),
            },
            Kpi {
                label: "Obrigações",
                value: &obligations,
                support: None,
                foot: Some(&format!("{} sem data fixa", summary.event_driven)),
            },
            Kpi {
                label: "Verificadas",
                value: &verified,
                support: None,
                foot: Some(&format!(
                    "{} parciais · {} por verificar · {} desatualizadas",
                    summary.partial, summary.unverified, summary.stale
                )),
            },
        ],
    );
    ui.add_space(14.0);

    let tone = match model.pack.freshness.status {
        FreshnessStatus::Current => Tone::Ok,
        FreshnessStatus::Stale => Tone::Warn,
        FreshnessStatus::Future => Tone::Slate,
    };
    widgets::accented_card(ui, tone, |ui| {
        ui.label(
            egui::RichText::new(&model.pack.freshness.message)
                .size(theme::BODY)
                .strong(),
        );
        widgets::caption(
            ui,
            &format!("checksum {}", &summary.checksum[..12.min(summary.checksum.len())]),
        );
    });

    if !model.pack.problems.is_empty() {
        ui.add_space(12.0);
        widgets::card(ui, |ui| {
            widgets::micro_label(ui, "o validador notou", theme::palette(ui.ctx()).text_3);
            for problem in &model.pack.problems {
                ui.label(
                    egui::RichText::new(&problem.message)
                        .size(theme::BODY_SMALL)
                        .color(theme::palette(ui.ctx()).text_2),
                );
            }
        });
    }

    ui.add_space(18.0);
    widgets::section(
        ui,
        "Fontes citadas",
        Some(&format!("{} documentos", model.rules.sources.len())),
        None,
    );
    let source_columns = [
        Column::at("Id", 120.0),
        Column::at("Tipo", 90.0),
        Column::at("Autoridade", 150.0),
        Column::new("Título"),
        Column::at("Obtida", 104.0),
    ];
    widgets::table(
        ui,
        &source_columns,
        &model.rules.sources,
        |_| false,
        |cells, source| {
            cells.mono(&source.id);
            cells.custom(|ui| {
                let (label, tone) = match source.tier {
                    SourceTier::Official => ("oficial", Tone::Ok),
                    SourceTier::Secondary => ("secundária", Tone::Slate),
                };
                widgets::badge(ui, label, tone);
            });
            cells.text(&source.authority);
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(&source.title)
                        .size(theme::BODY_SMALL)
                        .color(theme::palette(ui.ctx()).text),
                );
                widgets::caption(ui, &source.url);
            });
            cells.mono(&source.retrieved_at);
        },
    );

    ui.add_space(18.0);
    widgets::section(
        ui,
        "Obrigações declaradas",
        Some(&format!("{} no pacote", model.rules.obligations.len())),
        None,
    );
    let obligation_columns = [
        Column::at("Id", 190.0),
        Column::at("Imposto", 80.0),
        Column::new("Obrigação"),
        Column::at("Periodicidade", 130.0),
        Column::at("Base legal", 150.0),
    ];
    widgets::table(
        ui,
        &obligation_columns,
        &model.rules.obligations,
        |_| false,
        |cells, rule| {
            cells.mono(&rule.id);
            cells.text(&rule.tax);
            cells.text(&rule.title);
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(periodicity_label(rule.periodicity.kind))
                        .size(theme::BODY_SMALL)
                        .color(theme::palette(ui.ctx()).text_2),
                );
                if let Some(lag) = rule.periodicity.lag_months {
                    widgets::caption(ui, &format!("{lag} mês(es) após o período"));
                }
            });
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(rule.legal_basis.join(" · "))
                        .size(theme::CAPTION)
                        .color(theme::palette(ui.ctx()).text_3),
                );
            });
        },
    );

    if !model.rules.todo.is_empty() {
        ui.add_space(16.0);
        widgets::section(ui, "O que o pacote ainda não resolve", None, None);
        widgets::card(ui, |ui| widgets::notes(ui, Tone::Warn, &model.rules.todo));
    }

    ui.add_space(12.0);
    let normal = model
        .defaults
        .iva_normal_bp
        .map(format_bp_as_percent)
        .unwrap_or_else(|| "—".to_owned());
    let intermediate = model
        .defaults
        .iva_intermediate_bp
        .map(format_bp_as_percent)
        .unwrap_or_else(|| "—".to_owned());
    let reduced = model
        .defaults
        .iva_reduced_bp
        .map(format_bp_as_percent)
        .unwrap_or_else(|| "—".to_owned());
    widgets::footnote(
        ui,
        &format!(
            "Taxas de IVA lidas do pacote: normal {normal}, intermédia {intermediate}, reduzida \
             {reduced}. Onde o pacote tem um valor nulo, a aplicação recusa o cálculo em vez de \
             assumir um número — e é por isso que algumas linhas dizem «não estimável»."
        ),
    );
    action
}

/// How often a rule recurs.
#[must_use]
pub fn periodicity_label(kind: PeriodicityType) -> &'static str {
    match kind {
        PeriodicityType::Monthly => "mensal",
        PeriodicityType::Quarterly => "trimestral",
        PeriodicityType::Annual => "anual",
        PeriodicityType::Continuous => "permanente",
        PeriodicityType::EventDriven => "por evento",
    }
}

/// The verification badge for an obligation.
#[must_use]
pub fn verification_label(verification: Verification) -> (&'static str, Tone) {
    match verification {
        Verification::Verified => ("verificada", Tone::Ok),
        Verification::Partial => ("parcial", Tone::Warn),
        Verification::Unverified => ("por verificar", Tone::Warn),
        Verification::Stale => ("desatualizada", Tone::Danger),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_periodicity_has_a_portuguese_label() {
        for kind in [
            PeriodicityType::Monthly,
            PeriodicityType::Quarterly,
            PeriodicityType::Annual,
            PeriodicityType::Continuous,
            PeriodicityType::EventDriven,
        ] {
            assert!(!periodicity_label(kind).is_empty());
        }
    }

    #[test]
    fn an_unverified_rule_never_looks_verified() {
        assert_eq!(verification_label(Verification::Verified).1, Tone::Ok);
        assert_ne!(verification_label(Verification::Unverified).1, Tone::Ok);
        assert_ne!(verification_label(Verification::Stale).1, Tone::Ok);
    }
}
