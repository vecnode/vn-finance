//! Importing a fatura-recibo from a PDF.
//!
//! The flow is deliberately three steps, and the middle one is the point: read the
//! document, show what was read **including what could not be read**, let the person
//! correct it, and only then write. A form that pre-filled itself from a half-read
//! document and offered a Save button would be an invoice nobody checked.

use vnf_core::estimate::{InvoiceStatus, VatTreatment};
use vnf_core::money::{format_eur, parse_eur_to_cents};
use vnf_core::receipt::{ReceiptConfirmation, ReceiptDraft};
use vnf_core::types::IsoDate;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Tone};

/// Where the form is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Stage {
    /// Nothing read yet: the person names a file.
    Idle,
    /// The document was read, and these are the values to confirm.
    #[default]
    Confirm,
    /// It was written.
    Done,
}

/// The receipt form's state.
#[derive(Debug, Clone, Default)]
pub struct Form {
    /// Whether the window is showing.
    pub open: bool,
    /// Where it is.
    pub stage: Stage,
    /// The path the person typed.
    pub path: String,
    /// What the document said, once it has been read.
    pub draft: Option<ReceiptDraft>,
    /// The name the document was archived under, when it was.
    pub document_file: Option<String>,
    /// What the person will register — the document's values, editable.
    pub number: String,
    /// The issue date.
    pub date: String,
    /// The client's name.
    pub client_name: String,
    /// The client's NIF.
    pub client_nif: String,
    /// The client's country.
    pub client_country: String,
    /// What was supplied.
    pub description: String,
    /// The taxable base, in euros as typed.
    pub base: String,
    /// The VAT rate, in percent as typed.
    pub iva_rate: String,
    /// The withholding rate, in percent as typed.
    pub retention_rate: String,
    /// Why it cannot be registered, after a save attempt.
    pub problems: Vec<String>,
    /// What is worth knowing but is not a refusal.
    pub warnings: Vec<String>,
}

impl Form {
    /// Open the form, empty.
    #[must_use]
    pub fn starting() -> Self {
        Self {
            open: true,
            stage: Stage::Idle,
            client_country: "PT".to_owned(),
            iva_rate: String::new(),
            retention_rate: String::new(),
            ..Self::default()
        }
    }

    /// Fill the fields from a document that has just been read.
    pub fn read(&mut self, draft: ReceiptDraft, document_file: Option<String>) {
        self.stage = Stage::Confirm;
        self.number = draft.number.clone().unwrap_or_default();
        self.date = draft.date.clone().unwrap_or_default();
        self.client_name = draft.customer.name.clone().unwrap_or_default();
        self.client_nif = draft.customer.nif.clone().unwrap_or_default();
        self.client_country = draft
            .customer_country_hint
            .clone()
            .unwrap_or_else(|| "PT".to_owned());
        self.description = draft.description.clone().unwrap_or_default();
        self.base = draft.base_cents.map(format_eur).unwrap_or_default();
        self.iva_rate = draft.iva_rate_bp.map(percent_text).unwrap_or_default();
        self.retention_rate = draft
            .retention_cents
            .and_then(|cents| vnf_core::receipt::retention_bp_for(draft.base_cents.unwrap_or(0), cents))
            .map(percent_text)
            .unwrap_or_default();
        self.problems = draft.problems.clone();
        self.warnings = Vec::new();
        self.document_file = document_file;
        self.draft = Some(draft);
    }

    /// What the person is confirming, ready for the core.
    #[must_use]
    pub fn confirmation(&self) -> ReceiptConfirmation {
        ReceiptConfirmation {
            number: non_empty(&self.number),
            date: non_empty(&self.date).map(|date| IsoDate::from(date.as_str())),
            client_name: non_empty(&self.client_name),
            client_nif: non_empty(&self.client_nif),
            client_country: non_empty(&self.client_country),
            description: non_empty(&self.description),
            base_cents: parse_eur_to_cents(&self.base).ok(),
            iva_rate_bp: percent_bp(&self.iva_rate),
            retention_bp: percent_bp(&self.retention_rate),
            // Left to the core: it derives the treatment from the country and the rate,
            // which is the same rule every other surface uses.
            vat_treatment: None,
            status: None,
            atcud: None,
        }
    }

    /// Whether the form is ready to submit.
    #[must_use]
    pub fn complete(&self) -> bool {
        !self.base.trim().is_empty()
            && !self.date.trim().is_empty()
            && !self.client_name.trim().is_empty()
            && !self.client_country.trim().is_empty()
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// A rate in basis points, read as a percentage the person types.
fn percent_bp(text: &str) -> Option<i64> {
    let cleaned = text.trim().replace(',', ".").replace('%', "");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    cleaned
        .parse::<f64>()
        .ok()
        .map(|value| (value * 100.0).round() as i64)
}

/// Basis points as a percentage without the sign, which is what a field shows.
fn percent_text(bp: i64) -> String {
    let whole = bp / 100;
    let fraction = (bp % 100).abs();
    if fraction == 0 {
        format!("{whole}")
    } else {
        format!("{whole},{fraction:02}")
    }
}

/// Draw the form, if it is open.
pub fn show(ctx: &egui::Context, form: &mut Form) -> Option<Action> {
    if !form.open {
        return None;
    }
    let mut action = None;
    let palette = theme::palette(ctx);
    let response = egui::Modal::new(egui::Id::unique("receipt-form")).show(ctx, |ui| {
        ui.set_width(680.0);
        widgets::section(
            ui,
            "Registar fatura a partir do PDF",
            Some("lê o documento, não o adivinha"),
            Some(
                "A aplicação lê o texto do PDF e mostra o que conseguiu ler. Os campos vazios são \
                 campos que o documento não disse: preenche-os a partir do documento. Nada é \
                 registado sem confirmares.",
            ),
        );

        if form.stage == Stage::Idle {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut form.path)
                        .hint_text("C:\\caminho\\para\\fatura.pdf")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Ler o documento").clicked() && !form.path.trim().is_empty() {
                    action = Some(Action::ParseReceipt {
                        path: form.path.trim().to_owned(),
                    });
                }
                if widgets::button(ui, "Cancelar").clicked() {
                    action = Some(Action::CloseReceipt);
                }
            });
            return;
        }

        // --- what the document said -------------------------------------------
        if let Some(draft) = &form.draft {
            let tone = if draft.problems.is_empty() {
                Tone::Ok
            } else {
                Tone::Warn
            };
            widgets::accented_card(ui, tone, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {} · {}",
                        draft.kind.clone().unwrap_or_else(|| "documento".to_owned()),
                        draft.number.clone().unwrap_or_else(|| "sem número".to_owned()),
                        draft.date.clone().unwrap_or_else(|| "sem data".to_owned())
                    ))
                    .size(theme::BODY)
                    .strong(),
                );
                if let Some(file) = &form.document_file {
                    widgets::caption(ui, &format!("arquivado no cofre como {file}"));
                }
            });
            if !draft.problems.is_empty() {
                ui.add_space(8.0);
                widgets::notes(ui, Tone::Warn, &draft.problems);
            }
            if !draft.checks.is_empty() {
                ui.add_space(8.0);
                widgets::card(ui, |ui| {
                    widgets::micro_label(ui, "a conta do documento", theme::palette(ui.ctx()).text_3);
                    for check in &draft.checks {
                        ui.horizontal_wrapped(|ui| {
                            widgets::badge(
                                ui,
                                if check.ok { "confere" } else { "não confere" },
                                if check.ok { Tone::Ok } else { Tone::Danger },
                            );
                            ui.label(
                                egui::RichText::new(&check.label)
                                    .size(theme::BODY_SMALL)
                                    .color(theme::palette(ui.ctx()).text_2),
                            );
                        });
                        widgets::caption(ui, &check.detail);
                    }
                });
            }
        }

        // --- what will be registered ------------------------------------------
        ui.add_space(12.0);
        widgets::micro_label(ui, "o que vai ser registado", palette.text_3);
        ui.add_space(6.0);
        egui::Grid::new("receipt-fields")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                form_field(ui, "Número", &mut form.number, "FR 2026/1", 180.0);
                form_field(ui, "Data de emissão", &mut form.date, "2026-03-15", 130.0);
                form_field(
                    ui,
                    "Cliente",
                    &mut form.client_name,
                    "Empresa Exemplo, Lda.",
                    320.0,
                );
                form_field(ui, "NIF do cliente", &mut form.client_nif, "501234560", 130.0);
                form_field(ui, "País", &mut form.client_country, "PT", 60.0);
                form_field(ui, "Descrição", &mut form.description, "Consultoria", 400.0);
                form_field(ui, "Valor ilíquido", &mut form.base, "1 000,00 €", 140.0);
                form_field(ui, "Taxa de IVA (%)", &mut form.iva_rate, "23", 70.0);
                form_field(ui, "Retenção (%)", &mut form.retention_rate, "23", 70.0);
            });

        if !form.problems.is_empty() {
            ui.add_space(8.0);
            widgets::notes(ui, Tone::Danger, &form.problems);
        }
        if !form.warnings.is_empty() {
            ui.add_space(8.0);
            widgets::notes(ui, Tone::Warn, &form.warnings);
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Registar no livro").clicked() {
                action = Some(Action::SaveReceipt {
                    confirmation: Box::new(form.confirmation()),
                });
            }
            if widgets::button(ui, "Voltar").clicked() {
                action = Some(Action::CloseReceipt);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::caption(ui, "o PDF fica no cofre · o registo fica no livro, que só cresce");
            });
        });
    });

    if response.should_close() {
        action = Some(Action::CloseReceipt);
    }
    action
}

/// One labelled field.
fn form_field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, width: f32) {
    widgets::micro_label(ui, label, theme::palette(ui.ctx()).text_3);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(width),
    );
    ui.end_row();
}

/// The status label for an invoice the document said was paid.
#[must_use]
pub fn status_for(draft: &ReceiptDraft) -> InvoiceStatus {
    if draft.declares_paid {
        InvoiceStatus::Paid
    } else {
        InvoiceStatus::Issued
    }
}

/// The VAT treatment a country and a rate imply, for the form's preview line.
#[must_use]
pub fn treatment_hint(country: &str, rate_bp: Option<i64>) -> VatTreatment {
    match vnf_core::countries::scope_of(country) {
        vnf_core::countries::TurnoverScope::National => {
            if rate_bp.is_some_and(|rate| rate > 0) {
                VatTreatment::IvaPt
            } else {
                VatTreatment::IsentoArt53
            }
        }
        vnf_core::countries::TurnoverScope::Eu => VatTreatment::AutoliquidacaoUe,
        vnf_core::countries::TurnoverScope::NonEu => VatTreatment::Exportacao,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_is_read_as_a_percentage() {
        assert_eq!(percent_bp("23"), Some(2300));
        assert_eq!(percent_bp("23,5"), Some(2350));
        assert_eq!(percent_bp("0"), Some(0));
        assert_eq!(percent_bp(""), None);
        assert_eq!(percent_text(2300), "23");
        assert_eq!(percent_text(2350), "23,50");
    }

    #[test]
    fn an_empty_form_is_not_ready() {
        let form = Form::starting();
        assert!(!form.complete());
    }

    #[test]
    fn a_country_and_a_rate_imply_a_treatment() {
        assert_eq!(treatment_hint("PT", Some(2300)), VatTreatment::IvaPt);
        assert_eq!(treatment_hint("PT", Some(0)), VatTreatment::IsentoArt53);
        assert_eq!(treatment_hint("PT", None), VatTreatment::IsentoArt53);
        assert_eq!(treatment_hint("FI", None), VatTreatment::AutoliquidacaoUe);
        assert_eq!(treatment_hint("US", None), VatTreatment::Exportacao);
    }
}
