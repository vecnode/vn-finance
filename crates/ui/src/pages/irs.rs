//! IRS: what was withheld, and what to keep aside.
//!
//! The reserve is a **range**, not a figure, and the page says so in the same breath
//! as showing it. The withheld amounts are facts from the receipts; the reserve is an
//! estimate over the year's income and is labelled as one.

use egui::Ui;
use vnf_core::money::{format_bp_as_coefficient, format_eur};
use vnf_core::types::IrsRegime;
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Column, Def, Tone};

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel) -> Option<Action> {
    let action = None;
    let regime = model.profile.as_ref().map(|profile| profile.irs.regime);
    let label = match regime {
        Some(IrsRegime::Simplificado) => "Regime simplificado",
        Some(IrsRegime::Organizada) => "Contabilidade organizada",
        None => "Sem perfil",
    };
    widgets::section(
        ui,
        "IRS",
        Some(label),
        Some(
            "O IRS não se paga todos os meses: os clientes retêm uma parte em cada recibo e o \
             acerto faz-se na declaração do ano seguinte. A reserva é o que deves ter de lado para \
             esse acerto.",
        ),
    );

    let withheld: i64 = model
        .quarters
        .iter()
        .map(|quarter| quarter.totals.retencao_sofrida_cents)
        .sum();
    let gross: i64 = model
        .quarters
        .iter()
        .map(|quarter| quarter.totals.base_cents)
        .sum();
    let coefficient = model.profile.as_ref().map(|profile| profile.irs.coefficient_bp);

    let palette = theme::palette(ui.ctx());
    let low = format_eur(model.reserve.low_cents);
    let high = format_eur(model.reserve.high_cents);
    egui::Frame::new()
        .fill(palette.accent_bg)
        .stroke(egui::Stroke::new(1.0, palette.accent_border))
        .corner_radius(theme::radius())
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::micro_label(
                ui,
                "reserva recomendada para IRS e Segurança Social",
                palette.text_3,
            );
            ui.label(
                egui::RichText::new(format!("{low} – {high}"))
                    .size(theme::FIGURE)
                    .strong()
                    .color(palette.text),
            );
            widgets::caption(
                ui,
                "Intervalo, não previsão: um número único sugeriria uma precisão que os dados não \
                 têm.",
            );
        });

    ui.add_space(14.0);
    let gross_text = format_eur(gross);
    let withheld_text = format_eur(withheld);
    let mut rows = vec![
        Def {
            label: "Rendimento bruto",
            value: &gross_text,
            small: Some("base de incidência dos recibos do exercício"),
        },
        Def {
            label: "Retenções sofridas",
            value: &withheld_text,
            small: Some("IRS que os clientes entregaram por ti"),
        },
    ];
    let coefficient_text = coefficient.map(format_bp_as_coefficient);
    if let Some(text) = &coefficient_text {
        rows.push(Def {
            label: "Coeficiente",
            value: text,
            small: Some("declarado no perfil · o coeficiente legal está no pacote de regras"),
        });
    }
    widgets::card(ui, |ui| widgets::defs(ui, &rows));

    if !model.reserve.basis.is_empty() || !model.reserve.limitations.is_empty() {
        ui.add_space(12.0);
        widgets::card(ui, |ui| {
            if !model.reserve.basis.is_empty() {
                widgets::micro_label(ui, "como foi calculada", theme::palette(ui.ctx()).text_3);
                widgets::notes(ui, Tone::Slate, &model.reserve.basis);
                ui.add_space(6.0);
            }
            if !model.reserve.limitations.is_empty() {
                widgets::micro_label(ui, "o que não inclui", theme::palette(ui.ctx()).text_3);
                widgets::notes(ui, Tone::Warn, &model.reserve.limitations);
            }
        });
    }

    ui.add_space(16.0);
    widgets::section(ui, "Retenções por trimestre", None, None);
    let columns = [
        Column::at("Trimestre", 130.0),
        Column::at("Docs", 56.0).right(),
        Column::new("Base tributável").right(),
        Column::at("Retenção sofrida", 150.0).right(),
        Column::at("Taxa média", 110.0).right(),
    ];
    widgets::table(
        ui,
        &columns,
        &model.quarters,
        |_| false,
        |cells, quarter| {
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}.º trimestre", quarter.quarter))
                        .size(theme::BODY)
                        .color(theme::palette(ui.ctx()).text),
                );
            });
            cells.mono(&quarter.totals.invoice_count.to_string());
            cells.custom(|ui| money_right(ui, quarter.totals.base_cents));
            cells.custom(|ui| money_right(ui, quarter.totals.retencao_sofrida_cents));
            // A derived rate, so the person can see whether a receipt was withheld at
            // the rate they expected.
            let effective = if quarter.totals.base_cents > 0 {
                vnf_core::money::format_bp_as_percent(
                    (quarter.totals.retencao_sofrida_cents * 10_000) / quarter.totals.base_cents,
                )
            } else {
                "—".to_owned()
            };
            cells.custom(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    ui.label(
                        egui::RichText::new(effective)
                            .size(theme::BODY_SMALL)
                            .monospace()
                            .color(theme::palette(ui.ctx()).text_3),
                    );
                });
            });
        },
    );

    ui.add_space(10.0);
    let declaration = model.agenda.iter().find(|instance| {
        instance.tax.eq_ignore_ascii_case("IRS") && instance.kind == vnf_core::types::ObligationKind::Declare
    });
    if let Some(instance) = declaration {
        widgets::caption(
            ui,
            &format!("Declaração de IRS: {} — {}", instance.title, instance.due_date),
        );
    }
    widgets::footnote(
        ui,
        "O IRS é anual e a taxa depende do rendimento total do agregado, que esta aplicação não \
         conhece. Uma retenção de 23 % num recibo não significa uma taxa de 23 % no fim: é um \
         adiantamento.",
    );
    action
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
