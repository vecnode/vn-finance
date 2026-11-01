//! The invoices issued this year.
//!
//! A receipt is the document the client withholds tax from, so the table shows the
//! four numbers that matter on one: what was invoiced, the IVA charged, the IRS the
//! client kept, and what the client actually paid.

use egui::Ui;
use vnf_core::estimate::{Invoice, InvoiceStatus, VatTreatment, invoice_totals};
use vnf_core::money::format_eur;
use vnf_store::model::{DashboardModel, VaultEntry};

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Column, Kpi, Tone};

/// One row: the invoice, and the document the vault holds for it.
struct Row<'a> {
    invoice: &'a Invoice,
    document: Option<&'a VaultEntry>,
}

/// The table's columns.
///
/// Nine of them in the width of one page, so every fixed width is the width its content
/// actually needs — a money column is `3500,00 €` wide, not a round hundred — and the
/// client takes what is left. IVA and Retenção are optional: both are already stated in
/// the five figures above, and dropping them is what lets the table stay a table in a
/// small window instead of nine columns of two letters.
fn columns() -> [Column; 9] {
    [
        Column::at("N.º", 92.0),
        Column::at("Data", 82.0),
        Column::new("Cliente"),
        Column::at("Base", 82.0).right(),
        Column::at("IVA", 72.0).right().optional(),
        Column::at("Retenção", 82.0).right().optional(),
        Column::at("Líquido", 82.0).right(),
        Column::at("PDF", 46.0),
        Column::at("Estado", 80.0),
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
    widgets::kpi_rows(
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
    // Wrapped, not a plain row: the sentence beside the button is longer than the space
    // left over on a narrow window, and a label in a horizontal layout does not wrap.
    ui.horizontal_wrapped(|ui| {
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

    // The document the vault holds for each invoice, by the id the index records. One
    // pass, not one lookup per row: the table is drawn from what the model returned.
    let rows: Vec<Row<'_>> = invoices
        .iter()
        .map(|invoice| Row {
            invoice,
            document: model
                .vault
                .entries
                .iter()
                .find(|entry| entry.invoice_id.as_deref() == Some(invoice.id.as_str())),
        })
        .collect();

    let palette = theme::palette(ui.ctx());
    widgets::table(
        ui,
        &columns(),
        &rows,
        |_| false,
        |cells, row| {
            let invoice = row.invoice;
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(&invoice.number)
                        .size(theme::BODY)
                        .strong()
                        .color(palette.text),
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
                        .color(palette.text),
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
            cells.figure(&format_eur(totals.base_cents));
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(format_eur(totals.iva_cents))
                        .size(theme::BODY_SMALL)
                        .monospace()
                        .color(palette.text),
                );
                widgets::caption(ui, &vat_label(invoice.vat_treatment, invoice.iva_rate_bp));
            });
            cells.figure(&format_eur(totals.retention_cents));
            cells.figure(&format_eur(totals.net_receivable_cents));
            cells.custom(|ui| match row.document {
                Some(entry) => {
                    ui.label(
                        egui::RichText::new("PDF")
                            .size(theme::BODY_SMALL)
                            .strong()
                            .color(palette.ok),
                    )
                    .on_hover_text(&entry.file);
                    widgets::caption(ui, &short_hash(&entry.sha256));
                }
                None if invoice.payment_proof_in_vault => {
                    ui.label(
                        egui::RichText::new("PDF")
                            .size(theme::BODY_SMALL)
                            .strong()
                            .color(palette.ok),
                    );
                    widgets::caption(ui, "no cofre");
                }
                None => {
                    ui.label(
                        egui::RichText::new("—")
                            .size(theme::BODY_SMALL)
                            .color(palette.text_4),
                    );
                }
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
fn short_hash(sha256: &str) -> String {
    format!("sha256 {}", &sha256[..12.min(sha256.len())])
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
