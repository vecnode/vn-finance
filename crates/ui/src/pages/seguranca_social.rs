//! Segurança Social: the quarter's contribution, derived step by step.
//!
//! This is the page where showing the arithmetic matters most. The contribution is a
//! share of a share of what was invoiced for services, and a single number with no
//! working would be indistinguishable from a guess.

use egui::Ui;
use vnf_core::estimate::{QuarterReport, SsContributionEstimate};
use vnf_core::money::{format_bp_as_percent, format_eur};
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::pages::painel::current_quarter;
use crate::theme;
use crate::widgets::{self, Def, Tone};

/// Draw the page, for the quarter chosen.
pub fn show(ui: &mut Ui, model: &DashboardModel, today: &str, quarter: i64) -> Option<Action> {
    let mut action = None;
    let quarter = quarter.clamp(1, 4);
    let current = current_quarter(today);
    widgets::section(
        ui,
        "Segurança Social",
        Some("Contribuição do trimestre"),
        Some(
            "A contribuição é calculada sobre uma quota do que faturaste em serviços, e paga-se em \
             três prestações mensais. Cada linha abaixo mostra a conta que a aplicação fez — se \
             algo aqui estiver errado, o erro está no perfil, não na conta.",
        ),
    );

    ui.horizontal_wrapped(|ui| {
        for option in [1_i64, 2, 3, 4] {
            let label = format!("{option}.º trimestre");
            if widgets::chip(ui, option == quarter, &label).clicked() {
                action = Some(Action::SetQuarter(option));
            }
        }
        if quarter == current {
            widgets::badge(ui, "em curso", Tone::Accent);
        }
    });
    ui.add_space(12.0);

    let Some(report) = model.quarters.iter().find(|report| report.quarter == quarter) else {
        widgets::empty(ui, "Sem trimestre", "Este trimestre não tem dados no exercício.");
        return action;
    };

    if model.profile.is_none() {
        widgets::empty(
            ui,
            "Falta o perfil",
            "A quota relevante e a taxa contributiva dependem do que declaraste. Sem perfil a \
             aplicação não calcula a contribuição.",
        );
        return action;
    }

    let contribution = &report.social_security;
    widgets::card(ui, |ui| {
        widgets::micro_label(ui, "a conta do trimestre", theme::palette(ui.ctx()).text_3);
        ui.add_space(8.0);
        widgets::defs(
            ui,
            &[
                Def {
                    label: "Rendimento de serviços",
                    value: &format_eur(contribution.service_income_cents),
                    small: Some(&format!(
                        "{} documentos entre {} e {}",
                        report.totals.invoice_count, report.start, report.end
                    )),
                },
                Def {
                    label: "Quota relevante",
                    value: &format_bp_as_percent(contribution.relevant_income_share_bp),
                    small: Some("a parte do rendimento de serviços sujeita a contribuição"),
                },
                Def {
                    label: "Rendimento relevante",
                    value: &format_eur(contribution.relevant_income_cents),
                    small: Some("rendimento de serviços × quota relevante"),
                },
                Def {
                    label: "Taxa contributiva",
                    value: &format_bp_as_percent(contribution.contribution_rate_bp),
                    small: Some("taxa do regime dos trabalhadores independentes"),
                },
            ],
        );
    });

    ui.add_space(14.0);
    let palette = theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(palette.accent_bg)
        .stroke(egui::Stroke::new(1.0, palette.accent_border))
        .corner_radius(theme::radius())
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::micro_label(ui, "contribuição do trimestre", palette.text_3);
            ui.label(
                egui::RichText::new(format_eur(contribution.contribution_cents))
                    .size(theme::FIGURE)
                    .strong()
                    .color(palette.text),
            );
            if !contribution.instalments_cents.is_empty() {
                let parts: Vec<String> = contribution
                    .instalments_cents
                    .iter()
                    .map(|cents| format_eur(*cents))
                    .collect();
                widgets::caption(
                    ui,
                    &format!("em {} prestações mensais: {}", parts.len(), parts.join(" · ")),
                );
            }
        });

    if !contribution.limitations.is_empty() {
        ui.add_space(12.0);
        widgets::notes(ui, Tone::Warn, &contribution.limitations);
    }

    ui.add_space(16.0);
    widgets::section(ui, "Os quatro trimestres", None, None);
    let columns = [
        widgets::Column::at("Trimestre", 130.0),
        widgets::Column::at("Docs", 56.0).right(),
        widgets::Column::new("Rendimento de serviços").right(),
        widgets::Column::at("Quota relevante", 130.0).right(),
        widgets::Column::at("Contribuição", 140.0).right(),
    ];
    widgets::table(
        ui,
        &columns,
        &model.quarters,
        |report| report.quarter == quarter,
        |cells, report| {
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}.º", report.quarter))
                        .size(theme::BODY)
                        .strong()
                        .color(theme::palette(ui.ctx()).text),
                );
            });
            cells.mono(&report.totals.invoice_count.to_string());
            cells.custom(|ui| money_right(ui, report.social_security.service_income_cents));
            cells.custom(|ui| {
                money_right(ui, report.social_security.relevant_income_cents);
            });
            cells.custom(|ui| money_right(ui, report.social_security.contribution_cents));
        },
    );

    ui.add_space(10.0);
    widgets::footnote(
        ui,
        &format!(
            "A contribuição é uma estimativa a partir do que está registado no cofre. A Segurança \
             Social calcula sobre o que lhe foi comunicado, e o valor oficial pode diferir. O \
             período de isenção do início de atividade e as reduções vivem no pacote de regras \
             pt/{}.",
            model.pack.summary.pack_version
        ),
    );
    action
}

/// The contribution of one quarter, for a caller that wants only the number.
#[must_use]
pub fn contribution_of(report: &QuarterReport) -> &SsContributionEstimate {
    &report.social_security
}

/// A figure, right-aligned the way a column of money reads.
fn money_right(ui: &mut Ui, cents: i64) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
        ui.label(
            egui::RichText::new(format_eur(cents))
                .size(theme::BODY_SMALL)
                .monospace()
                .color(theme::palette(ui.ctx()).text),
        );
    });
}
