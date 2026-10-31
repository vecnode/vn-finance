//! IVA: what was charged, quarter by quarter.
//!
//! The page answers one question — how much IVA did I put on invoices this year — and
//! then says where the declaration is in the agenda, because a figure without its
//! deadline is half a fact.

use egui::Ui;
use vnf_core::money::{format_bp_as_percent, format_eur};
use vnf_core::types::{InstanceStatus, IvaRegime};
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::pages::painel::current_quarter;
use crate::theme;
use crate::widgets::{self, Column, Tone};

/// The table's columns.
fn columns() -> [Column; 5] {
    [
        Column::at("Trimestre", 150.0),
        Column::at("Docs", 60.0).right(),
        Column::new("Base tributável").right(),
        Column::at("IVA liquidado", 150.0).right(),
        Column::at("Retenção sofrida", 150.0).right(),
    ]
}

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel, today: &str) -> Option<Action> {
    let action = None;
    let regime = model.profile.as_ref().map(|profile| profile.iva.regime);
    let current = current_quarter(today);
    let declared = match regime {
        Some(IvaRegime::IsentoArt53) => "Isenção do art. 53.º do CIVA",
        Some(IvaRegime::Mensal) => "Regime normal — declaração mensal",
        Some(IvaRegime::Trimestral) => "Regime normal — declaração trimestral",
        None => "Sem perfil",
    };

    widgets::section(
        ui,
        "IVA",
        Some(&format!("{declared} · {}.º trimestre em curso", current)),
        Some(
            "O IVA que cobraste aos clientes. É dinheiro que passou por ti: fica em dívida até à \
             declaração periódica, e é por isso que a reserva recomendada também o tem em conta.",
        ),
    );

    let total: i64 = model
        .quarters
        .iter()
        .map(|quarter| quarter.totals.iva_liquidado_cents)
        .sum();
    let base: i64 = model
        .quarters
        .iter()
        .map(|quarter| quarter.totals.base_cents)
        .sum();
    let withheld: i64 = model
        .quarters
        .iter()
        .map(|quarter| quarter.totals.retencao_sofrida_cents)
        .sum();
    ui.horizontal_wrapped(|ui| {
        widgets::caption(ui, &format!("Base do exercício: {}", format_eur(base)));
        ui.separator();
        widgets::caption(ui, &format!("IVA liquidado: {}", format_eur(total)));
        ui.separator();
        widgets::caption(ui, &format!("Retenções sofridas: {}", format_eur(withheld)));
        if let Some(rate) = model.defaults.iva_normal_bp {
            ui.separator();
            widgets::caption(ui, &format!("Taxa normal: {}", format_bp_as_percent(rate)));
        }
    });
    ui.add_space(12.0);

    widgets::table(
        ui,
        &columns(),
        &model.quarters,
        |quarter| quarter.quarter == current,
        |cells, quarter| {
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}.º trimestre", quarter.quarter))
                        .size(theme::BODY)
                        .strong()
                        .color(theme::palette(ui.ctx()).text),
                );
                widgets::caption(ui, &format!("{} – {}", quarter.start, quarter.end));
            });
            cells.mono(&quarter.totals.invoice_count.to_string());
            cells.custom(|ui| money_right(ui, quarter.totals.base_cents));
            cells.custom(|ui| money_right(ui, quarter.totals.iva_liquidado_cents));
            cells.custom(|ui| money_right(ui, quarter.totals.retencao_sofrida_cents));
        },
    );

    ui.add_space(14.0);
    // The declarations that carry these figures, from the same agenda the calendar
    // page shows — one source, not a second copy of the dates.
    let declarations: Vec<&vnf_core::types::ObligationInstance> = model
        .agenda
        .iter()
        .filter(|instance| instance.tax.eq_ignore_ascii_case("IVA"))
        .collect();
    if declarations.is_empty() {
        widgets::footnote(
            ui,
            "Não há declarações de IVA na agenda deste ano: com isenção do art. 53.º não há \
             declaração periódica a entregar.",
        );
    } else {
        widgets::card(ui, |ui| {
            widgets::micro_label(ui, "declarações na agenda", theme::palette(ui.ctx()).text_3);
            for instance in declarations {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} · {}", instance.title, instance.due_date))
                            .size(theme::BODY_SMALL)
                            .color(theme::palette(ui.ctx()).text_2),
                    );
                    let (label, tone) = match instance.status {
                        InstanceStatus::Overdue => ("em atraso", Tone::Danger),
                        InstanceStatus::DueSoon => ("a vencer", Tone::Warn),
                        InstanceStatus::Done => ("tratada", Tone::Ok),
                        InstanceStatus::Future => ("agendada", Tone::Slate),
                        InstanceStatus::Untracked => ("histórico", Tone::Na),
                        InstanceStatus::NotApplicable => ("não aplicável", Tone::Na),
                    };
                    widgets::pill(ui, label, tone);
                });
            }
        });
    }

    ui.add_space(10.0);
    widgets::footnote(
        ui,
        "Uma autoliquidação intracomunitária e uma exportação aparecem com IVA a zero: o imposto é \
         devido no país do cliente, não aqui. Se passares o limiar de operações intracomunitárias, \
         o pacote de regras acrescenta as declarações próprias à agenda.",
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
