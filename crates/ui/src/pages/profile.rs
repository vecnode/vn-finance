//! The profile form: the numbers only the person can supply.
//!
//! Every field here is a declaration, not a deduction. The form therefore never
//! guesses: an empty field stays an empty field, and the page that needed it says which
//! check it could not run rather than filling in a default that looks authoritative.

use vnf_core::money::{format_bp_as_percent, format_eur, parse_eur_to_cents};
use vnf_core::profile::ProfileDraft;
use vnf_core::types::{IrsRegime, IvaRegime, TaxProfile};

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Tone};

/// What the form holds while it is open.
#[derive(Debug, Clone)]
pub struct Form {
    /// Whether the window is showing.
    pub open: bool,
    /// The tax number.
    pub nif: String,
    /// The name.
    pub name: String,
    /// When the activity began, as typed.
    pub start_date: String,
    /// When tracking began, as typed.
    pub tracking_start: String,
    /// The IVA regime.
    pub iva: IvaRegime,
    /// The IRS regime.
    pub irs: IrsRegime,
    /// The simplified-regime coefficient, as a percentage the person types.
    pub coefficient: String,
    /// Last year's turnover, in euros as typed.
    pub turnover_previous: String,
    /// This year's expected turnover, in euros as typed.
    pub turnover_current: String,
    /// Tax residence in Portugal.
    pub resident_pt: bool,
    /// CIRS category B.
    pub category_b: bool,
    /// The start-up contribution exemption.
    pub startup_exemption: bool,
    /// Whether this saves over an existing profile rather than creating one.
    pub editing: bool,
    /// What the person last asked to do, shown as a message under the buttons.
    pub message: Option<(String, Tone)>,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            open: false,
            nif: String::new(),
            name: String::new(),
            start_date: String::new(),
            tracking_start: String::new(),
            iva: IvaRegime::Trimestral,
            irs: IrsRegime::Simplificado,
            coefficient: "75".to_owned(),
            turnover_previous: String::new(),
            turnover_current: String::new(),
            resident_pt: true,
            category_b: true,
            startup_exemption: false,
            editing: false,
            message: None,
        }
    }
}

impl Form {
    /// A form filled from the profile in the vault, or empty for a first run.
    #[must_use]
    pub fn from_profile(profile: Option<&TaxProfile>) -> Self {
        let mut form = Self::default();
        let Some(profile) = profile else {
            return form;
        };
        form.editing = true;
        form.nif = profile.nif.clone();
        form.name = profile.name.clone();
        form.start_date = profile.activity.start_date.clone();
        form.tracking_start = profile.tracking_start.clone().unwrap_or_default();
        form.iva = profile.iva.regime;
        form.irs = profile.irs.regime;
        // The stored value is basis points; the person thinks in percent.
        form.coefficient = format_bp_as_percent(profile.irs.coefficient_bp)
            .replace('%', "")
            .trim()
            .replace(' ', "");
        form.turnover_previous = profile
            .activity
            .turnover_previous_year_cents
            .map(format_eur)
            .unwrap_or_default();
        form.turnover_current = profile
            .activity
            .turnover_current_year_expected_cents
            .map(format_eur)
            .unwrap_or_default();
        form.resident_pt = profile.resident_pt;
        form.category_b = profile.activity.category_b;
        form.startup_exemption = profile.ss.startup_exemption_active;
        form
    }

    /// A form for a caller that wants one, open and filled from what it holds.
    #[must_use]
    pub fn editing(profile: Option<&TaxProfile>) -> Self {
        let mut form = Self::from_profile(profile);
        form.open = true;
        form
    }

    /// The coefficient as basis points, if what was typed is a number.
    #[must_use]
    pub fn coefficient_bp(&self) -> Option<i64> {
        let cleaned = self.coefficient.trim().replace(',', ".").replace('%', "");
        let cleaned = cleaned.trim().replace(' ', "");
        if cleaned.is_empty() {
            return None;
        }
        cleaned
            .parse::<f64>()
            .ok()
            .map(|value| (value * 100.0).round() as i64)
    }

    /// The draft to hand to `build_profile`.
    #[must_use]
    pub fn to_draft(&self) -> ProfileDraft {
        ProfileDraft {
            nif: Some(self.nif.trim().to_owned()),
            name: Some(self.name.trim().to_owned()),
            iva_regime: Some(self.iva),
            irs_regime: Some(self.irs),
            start_date: Some(self.start_date.trim().to_owned()),
            tracking_start: if self.tracking_start.trim().is_empty() {
                None
            } else {
                Some(self.tracking_start.trim().to_owned())
            },
            turnover_previous_year_cents: Some(parse_eur_to_cents(self.turnover_previous.trim()).ok()),
            turnover_current_year_expected_cents: Some(parse_eur_to_cents(self.turnover_current.trim()).ok()),
            cae: None,
            category_b: Some(self.category_b),
            resident_pt: Some(self.resident_pt),
            is_company: Some(false),
            intra_community_operations: None,
            intra_community_operations_above_50k: None,
            exports: None,
            startup_exemption_active: Some(self.startup_exemption),
        }
    }

    /// What is wrong with what has been typed, before it reaches the core.
    ///
    /// The core validates the meaning; this catches the things a text field makes
    /// possible and a form should not submit — a bad tax number, a bad date, a turnover
    /// that is not money.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !vnf_core::nif::is_valid_pt_tax_number(&self.nif) {
            problems.push("O NIF não é válido (9 dígitos, com o dígito de controlo correto).".to_owned());
        }
        if self.name.trim().is_empty() {
            problems.push("Falta o nome da atividade.".to_owned());
        }
        if vnf_core::dates::parse_iso(self.start_date.trim()).is_err() {
            problems.push("A data de início de atividade tem de ser AAAA-MM-DD.".to_owned());
        }
        if !self.tracking_start.trim().is_empty()
            && vnf_core::dates::parse_iso(self.tracking_start.trim()).is_err()
        {
            problems.push("A data de início de acompanhamento tem de ser AAAA-MM-DD.".to_owned());
        }
        if self.coefficient_bp().is_none_or(|bp| !(0..=10_000).contains(&bp)) {
            problems.push("O coeficiente tem de ser uma percentagem entre 0 e 100.".to_owned());
        }
        for (label, value) in [
            ("volume do ano anterior", &self.turnover_previous),
            ("volume esperado deste ano", &self.turnover_current),
        ] {
            if !value.trim().is_empty() && parse_eur_to_cents(value.trim()).is_err() {
                problems.push(format!("O {label} não é um valor em euros que se leia."));
            }
        }
        problems
    }
}

/// Draw the form, if it is open.
pub fn show(ctx: &egui::Context, form: &mut Form) -> Option<Action> {
    if !form.open {
        return None;
    }
    let mut action = None;
    let palette = theme::palette(ctx);
    let response = egui::Modal::new(egui::Id::unique("profile-form")).show(ctx, |ui| {
        ui.set_width(620.0);
        widgets::section(
            ui,
            if form.editing {
                "Editar o perfil"
            } else {
                "Preencher o perfil"
            },
            Some("dados da declaração de início de atividade"),
            Some(
                "Estes valores são declarações, não deduções. A aplicação não os adivinha: se um \
                     ficar vazio, as verificações que dependem dele dizem que não conseguiram correr.",
            ),
        );

        egui::Grid::new("profile-fields")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                widgets::micro_label(ui, "NIF", palette.text_3);
                ui.add(
                    egui::TextEdit::singleline(&mut form.nif)
                        .hint_text("123456789")
                        .desired_width(200.0),
                );
                ui.end_row();

                widgets::micro_label(ui, "Nome da atividade", palette.text_3);
                ui.add(
                    egui::TextEdit::singleline(&mut form.name)
                        .hint_text("Pessoa Exemplo")
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();

                widgets::micro_label(ui, "Início de atividade", palette.text_3);
                ui.add(
                    egui::TextEdit::singleline(&mut form.start_date)
                        .hint_text("2019-04-01")
                        .desired_width(200.0),
                );
                ui.end_row();

                widgets::micro_label(ui, "Acompanhamento desde", palette.text_3);
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut form.tracking_start)
                            .hint_text("2026-01-01")
                            .desired_width(200.0),
                    );
                    widgets::caption(ui, "antes desta data, o que venceu é histórico e não falha");
                });
                ui.end_row();

                widgets::micro_label(ui, "Regime de IVA", palette.text_3);
                ui.horizontal_wrapped(|ui| {
                    for (regime, label) in [
                        (IvaRegime::IsentoArt53, "Isenção art. 53.º"),
                        (IvaRegime::Trimestral, "Trimestral"),
                        (IvaRegime::Mensal, "Mensal"),
                    ] {
                        if widgets::chip(ui, form.iva == regime, label).clicked() {
                            form.iva = regime;
                        }
                    }
                });
                ui.end_row();

                widgets::micro_label(ui, "Regime de IRS", palette.text_3);
                ui.horizontal_wrapped(|ui| {
                    for (regime, label) in [
                        (IrsRegime::Simplificado, "Simplificado"),
                        (IrsRegime::Organizada, "Organizada"),
                    ] {
                        if widgets::chip(ui, form.irs == regime, label).clicked() {
                            form.irs = regime;
                        }
                    }
                });
                ui.end_row();

                widgets::micro_label(ui, "Coeficiente", palette.text_3);
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut form.coefficient).desired_width(90.0));
                    if let Some(bp) = form.coefficient_bp() {
                        widgets::caption(ui, &format!("lido como {}", format_bp_as_percent(bp)));
                    }
                });
                ui.end_row();

                widgets::micro_label(ui, "Volume do ano anterior", palette.text_3);
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut form.turnover_previous)
                            .hint_text("0,00 €")
                            .desired_width(180.0),
                    );
                    widgets::caption(ui, "só em Portugal; é o que decide a isenção do art. 53.º");
                });
                ui.end_row();

                widgets::micro_label(ui, "Volume esperado deste ano", palette.text_3);
                ui.add(
                    egui::TextEdit::singleline(&mut form.turnover_current)
                        .hint_text("0,00 €")
                        .desired_width(180.0),
                );
                ui.end_row();

                widgets::micro_label(ui, "Condições", palette.text_3);
                ui.vertical(|ui| {
                    ui.checkbox(&mut form.resident_pt, "Residente fiscal em Portugal");
                    ui.checkbox(
                        &mut form.category_b,
                        "Rendimentos da categoria B (recibos verdes)",
                    );
                    ui.checkbox(
                        &mut form.startup_exemption,
                        "Isenção de contribuições no início de atividade",
                    );
                });
                ui.end_row();
            });

        let problems = form.problems();
        if !problems.is_empty() {
            ui.add_space(8.0);
            widgets::notes(ui, Tone::Danger, &problems);
        }
        if let Some((message, tone)) = &form.message {
            ui.add_space(8.0);
            widgets::notes(ui, *tone, std::slice::from_ref(message));
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let can_save = problems.is_empty();
            if widgets::primary_button(ui, "Guardar").clicked() {
                match form.coefficient_bp() {
                    Some(coefficient_bp) if can_save => {
                        action = Some(Action::SaveProfile {
                            draft: Box::new(form.to_draft()),
                            coefficient_bp,
                        });
                    }
                    _ if can_save => {
                        form.message = Some(("O coeficiente tem de ser um número.".to_owned(), Tone::Danger));
                    }
                    _ => {
                        form.message = Some(("Corrige os campos assinalados.".to_owned(), Tone::Danger));
                    }
                }
            }
            if widgets::button(ui, "Cancelar").clicked() {
                action = Some(Action::CloseProfile);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::caption(ui, "Guardar escreve no cofre · Cancelar não altera nada");
            });
        });
    });

    // Escape and a click on the backdrop close it, like any dialog.
    if response.should_close() {
        action = Some(Action::CloseProfile);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_form_says_what_is_missing() {
        let form = Form::default();
        let problems = form.problems();
        assert!(problems.iter().any(|problem| problem.contains("NIF")));
        assert!(problems.iter().any(|problem| problem.contains("nome")));
        assert!(problems.iter().any(|problem| problem.contains("data")));
        assert_eq!(
            problems.len(),
            3,
            "a blank form complains about exactly those: {problems:?}"
        );
    }

    /// A form that would be savable but for whatever the test changes.
    fn filled() -> Form {
        Form {
            nif: "123456789".to_owned(),
            name: "Pessoa Exemplo".to_owned(),
            start_date: "2019-04-01".to_owned(),
            ..Form::default()
        }
    }

    #[test]
    fn a_filled_form_is_savable() {
        let form = filled();
        assert!(form.problems().is_empty(), "{:?}", form.problems());
    }

    #[test]
    fn an_impossible_coefficient_is_refused() {
        let mut form = filled();
        form.coefficient = "150".to_owned();
        assert!(
            form.problems()
                .iter()
                .any(|problem| problem.contains("coeficiente"))
        );
        form.coefficient = "75".to_owned();
        assert!(form.problems().is_empty());
    }

    #[test]
    fn a_turnover_that_is_not_money_is_refused() {
        let mut form = filled();
        form.turnover_previous = "muito".to_owned();
        assert!(
            form.problems()
                .iter()
                .any(|problem| problem.contains("volume do ano anterior"))
        );
    }
}
