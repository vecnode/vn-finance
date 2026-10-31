//! The invoices issued this year.
//!
//! A receipt is the document the client withholds tax from, so the table shows the
//! four numbers that matter on one: what was invoiced, the IVA charged, the IRS the
//! client kept, and what the client actually paid.

use egui::Ui;
use vnf_core::estimate::{Invoice, InvoiceStatus, VatTreatment, invoice_totals};
use vnf_core::money::format_eur;
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Column, Kpi, Tone};

/// The table's columns.
fn columns() -> [Column; 8] {
    [
        Column::at("N.º", 110.0),
        Column::at("Data", 92.0),
        Column::new("Cliente"),
        Column::at("Base", 104.0).right(),
        Column::at("IVA", 96.0).right(),
        Column::at("Retenção", 96.0).right(),
        Column::at("Líquido", 104.0).right(),
        Column::at("Estado", 92.0),
    ]
}

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel, year: i64) -> Option<Action> {
    let mut action = None;
    // Only the year in course: the ledger spans years and the page is about this one.
    let prefix = format!("{year}-");
    let invoices: Vec<&Invoice> = model
        .invoices
        .iter()
        .filter(|invoice| invoice.date.starts_with(&prefix))
        .collect();

    let base: i64 = invoices.iter().map(|invoice| invoice.base_cents).sum();
    let iva: i64 = invoices
        .iter()
        .map(|invoice| invoice_totals(invoice).iva_cents)
        .sum();
    let withheld: i64 = invoices
        .iter()
        .map(|invoice| invoice_totals(invoice).retention_cents)
        .sum();
    let net: i64 = invoices
        .iter()
        .map(|invoice| invoice_totals(invoice).net_receivable_cents)
        .sum();

    let value_base = format_eur(base);
    let value_iva = format_eur(iva);
    let value_withheld = format_eur(withheld);
    let value_net = format_eur(net);
    let count = format!("{} documento(s)", invoices.len());
    widgets::section(
        ui,
        "Faturas emitidas",
        Some(&format!("Exercício {year}")),
        Some(
            "Os documentos que emitiste. O líquido é o que o cliente te paga: base mais IVA, menos \
             a retenção na fonte que ele entrega ao Estado por ti.",
        ),
    );
    widgets::kpi_row(
        ui,
        &[
            Kpi {
                label: "Documentos",
                value: &invoices.len().to_string(),
                support: Some("no exercício"),
                foot: Some(&count),
            },
            Kpi {
                label: "Base tributável",
                value: &value_base,
                support: None,
                foot: Some(&count),
            },
            Kpi {
                label: "IVA liquidado",
                value: &value_iva,
                support: None,
                foot: Some(&count),
            },
            Kpi {
                label: "Retenção sofrida",
                value: &value_withheld,
                support: None,
                foot: Some(&count),
            },
            Kpi {
                label: "Líquido a receber",
                value: &value_net,
                support: None,
                foot: Some(&count),
            },
        ],
    );
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Registar fatura a partir do PDF").clicked() {
            action = Some(Action::OpenReceipt);
        }
        widgets::caption(
            ui,
            "o documento é lido e arquivado no cofre, e só entra no livro depois de confirmares",
        );
    });
    ui.add_space(16.0);

    if invoices.is_empty() {
        widgets::empty(
            ui,
            "Sem faturas neste exercício",
            "Um cofre novo não tem faturação. As faturas entram aqui quando as registares; os \
             totais acima ficam a zero enquanto não houver nenhuma.",
        );
        return action;
    }

    widgets::table(
        ui,
        &columns(),
        &invoices,
        |_| false,
        |cells, invoice| {
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(&invoice.number)
                        .size(theme::BODY)
                        .strong()
                        .color(theme::palette(ui.ctx()).text),
                );
                if let Some(atcud) = &invoice.atcud {
                    widgets::caption(ui, atcud);
                }
            });
            cells.mono(&invoice.date);
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(&invoice.client_name)
                        .size(theme::BODY)
                        .color(theme::palette(ui.ctx()).text),
                );
                widgets::caption(
                    ui,
                    &match &invoice.client_nif {
                        Some(nif) => format!("{} · {}", invoice.client_country, nif),
                        None => invoice.client_country.clone(),
                    },
                );
            });
            let totals = invoice_totals(invoice);
            cells.custom(|ui| money_right(ui, totals.base_cents));
            cells.custom(|ui| {
                money_right(ui, totals.iva_cents);
                widgets::caption(ui, &vat_label(invoice.vat_treatment, invoice.iva_rate_bp));
            });
            cells.custom(|ui| money_right(ui, totals.retention_cents));
            cells.custom(|ui| {
                money_right(ui, totals.net_receivable_cents);
            });
            cells.custom(|ui| status_pill(ui, invoice.status));
        },
    );

    ui.add_space(10.0);
    widgets::footnote(
        ui,
        "O IVA e a retenção são calculados a partir da base e das taxas de cada documento, com \
         arredondamento ao cêntimo. Uma autoliquidação comunitária tem IVA a zero por quem a recebe \
         se autoliquida; o valor entra na declaração, não no recibo.",
    );
    let _ = &mut action;
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

/// The status pill for an invoice.
fn status_pill(ui: &mut Ui, status: InvoiceStatus) {
    let (label, tone) = match status {
        InvoiceStatus::Paid => ("paga", Tone::Ok),
        InvoiceStatus::Issued => ("emitida", Tone::Slate),
        InvoiceStatus::Pending => ("a vencer", Tone::Warn),
    };
    widgets::pill(ui, label, tone);
}

/// How the VAT was treated, in the words the invoice uses.
fn vat_label(treatment: VatTreatment, rate_bp: i64) -> String {
    match treatment {
        VatTreatment::IvaPt => format!("{} IVA", vnf_core::money::format_bp_as_percent(rate_bp)),
        VatTreatment::AutoliquidacaoUe => "autoliquidação".to_owned(),
        VatTreatment::IsentoArt53 => "isento art. 53.º".to_owned(),
        VatTreatment::Exportacao => "exportação".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_eu_invoice_is_named_as_a_reverse_charge() {
        assert_eq!(vat_label(VatTreatment::AutoliquidacaoUe, 0), "autoliquidação");
        assert_eq!(vat_label(VatTreatment::IsentoArt53, 0), "isento art. 53.º");
    }

    #[test]
    fn a_rate_is_written_the_portuguese_way() {
        // The reference writes no space before the sign, and the golden parity test
        // holds this formatter to it.
        assert_eq!(vat_label(VatTreatment::IvaPt, 2300), "23% IVA");
    }
}
