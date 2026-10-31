//! The agenda: what the law obliges, and when.
//!
//! The panel this replaces put the calendar in a five-column table with a filter bar
//! above it. The port keeps both, and keeps the two things that made the table
//! readable: the obligation column carries the period underneath the title, and a row
//! that falls due today is tinted rather than shouting.

use egui::Ui;
use vnf_core::types::{InstanceStatus, ObligationInstance, Verification};
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Column, Tone};

/// Which obligations the table shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// Everything the year contains.
    #[default]
    All,
    /// Past its date and not handled.
    Overdue,
    /// Due within the window.
    Soon,
    /// Marked handled.
    Done,
    /// The rule does not apply to this profile.
    NotApplicable,
}

impl Filter {
    /// Every filter, in the order the chips appear.
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Overdue,
        Self::Soon,
        Self::Done,
        Self::NotApplicable,
    ];

    /// What the chip says.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Tudo",
            Self::Overdue => "Em atraso",
            Self::Soon => "Próximos 30 dias",
            Self::Done => "Tratadas",
            Self::NotApplicable => "Não aplicável",
        }
    }

    /// Whether an instance survives this filter.
    #[must_use]
    pub fn keeps(self, instance: &ObligationInstance) -> bool {
        match self {
            Self::All => true,
            Self::Overdue => instance.status == InstanceStatus::Overdue,
            Self::Soon => instance.status == InstanceStatus::DueSoon,
            Self::Done => instance.status == InstanceStatus::Done,
            Self::NotApplicable => instance.status == InstanceStatus::NotApplicable,
        }
    }

    /// How many instances it would show, for the chip's counter.
    #[must_use]
    pub fn count(self, model: &DashboardModel) -> usize {
        model
            .agenda
            .iter()
            .filter(|instance| self.keeps(instance))
            .count()
    }
}

/// The table's columns.
fn columns() -> [Column; 6] {
    [
        Column::at("Estado", 104.0),
        Column::new("Obrigação"),
        Column::at("Vence", 104.0),
        Column::at("Base legal", 150.0),
        Column::at("Docs", 56.0).right(),
        Column::at("Ação", 124.0),
    ]
}

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel, filter: Filter, today: &str) -> Option<Action> {
    let mut action = None;
    widgets::section(
        ui,
        "Agenda fiscal",
        Some(&format!("{} obrigações no exercício", model.agenda.len())),
        Some(
            "Cada linha é uma obrigação que a lei te impõe este ano, com o artigo em que se \
             baseia. «Tratada» regista que já cumpriste; não submete nada nem fala com o portal.",
        ),
    );

    ui.horizontal_wrapped(|ui| {
        for option in Filter::ALL {
            let label = format!("{} · {}", option.label(), option.count(model));
            if widgets::chip(ui, option == filter, &label).clicked() {
                action = Some(Action::SetFilter(option));
            }
        }
    });
    ui.add_space(10.0);

    let shown: Vec<&ObligationInstance> = model
        .agenda
        .iter()
        .filter(|instance| filter.keeps(instance))
        .collect();

    if shown.is_empty() {
        widgets::empty(
            ui,
            "Nada nesta vista",
            match filter {
                Filter::Overdue => "Não há nada em atraso. É a melhor linha desta página.",
                Filter::Soon => "Nada nos próximos 30 dias.",
                Filter::Done => "Ainda não marcaste nada como tratado.",
                Filter::NotApplicable => "Todas as regras do pacote se aplicam ao teu perfil.",
                Filter::All => "Não há obrigações para este ano com o perfil atual.",
            },
        );
        return action;
    }

    widgets::table(
        ui,
        &columns(),
        &shown,
        |instance| instance.due_date.as_str() == today,
        |cells, instance| {
            cells.custom(|ui| status_pill(ui, instance.status));
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(&instance.title)
                        .size(theme::BODY)
                        .strong()
                        .color(theme::palette(ui.ctx()).text),
                );
                if !instance.period_label.is_empty() {
                    ui.label(
                        egui::RichText::new(&instance.period_label)
                            .size(theme::CAPTION)
                            .color(theme::palette(ui.ctx()).text_3),
                    );
                }
                if let Some(reason) = &instance.not_applicable_reason {
                    ui.label(
                        egui::RichText::new(reason)
                            .size(theme::CAPTION)
                            .color(theme::palette(ui.ctx()).text_4),
                    );
                }
            });
            cells.custom(|ui| {
                ui.label(
                    egui::RichText::new(&instance.due_date)
                        .size(theme::MONO)
                        .monospace()
                        .color(theme::palette(ui.ctx()).text_2),
                );
                if instance.provisional {
                    widgets::badge(ui, "provisório", Tone::Warn);
                }
            });
            cells.custom(|ui| {
                if instance.legal_basis.is_empty() {
                    cells_nothing(ui);
                } else {
                    ui.label(
                        egui::RichText::new(instance.legal_basis.join(" · "))
                            .size(theme::CAPTION)
                            .color(theme::palette(ui.ctx()).text_3),
                    );
                }
                if instance.verification != Verification::Verified {
                    widgets::badge(ui, verification_label(instance.verification), Tone::Warn);
                }
            });
            cells.custom(|ui| {
                if instance.documents_to_keep.is_empty() {
                    cells_nothing(ui);
                } else {
                    widgets::badge(ui, &instance.documents_to_keep.len().to_string(), Tone::Slate);
                }
            });
            cells.custom(|ui| {
                if is_actionable(instance.status) {
                    if widgets::button(ui, "Marcar tratada").clicked() {
                        action = Some(Action::Complete {
                            id: instance.id.clone(),
                            rule_id: instance.rule_id.clone(),
                            due_date: instance.due_date.clone(),
                        });
                    }
                } else if instance.status == InstanceStatus::Done {
                    widgets::caption(ui, "tratada");
                } else if instance.status == InstanceStatus::NotApplicable {
                    widgets::caption(ui, "—");
                }
            });
        },
    );

    ui.add_space(10.0);
    let overdue = model
        .agenda
        .iter()
        .filter(|i| i.status == InstanceStatus::Overdue)
        .count();
    let soon = model
        .agenda
        .iter()
        .filter(|i| i.status == InstanceStatus::DueSoon)
        .count();
    let untracked = model
        .agenda
        .iter()
        .filter(|i| i.status == InstanceStatus::Untracked)
        .count();
    widgets::footnote(
        ui,
        &format!(
            "{overdue} em atraso · {soon} nos próximos 30 dias · {untracked} antes do início do \
             acompanhamento (histórico, não falha). O prazo é ajustado quando cai ao fim de semana \
             ou num feriado registado no pacote de regras."
        ),
    );
    action
}

/// A dash, drawn the way a filled cell is.
fn cells_nothing(ui: &mut Ui) {
    ui.label(
        egui::RichText::new("—")
            .size(theme::BODY_SMALL)
            .color(theme::palette(ui.ctx()).text_4),
    );
}

/// The status pill for an instance.
fn status_pill(ui: &mut Ui, status: InstanceStatus) {
    let (label, tone) = match status {
        InstanceStatus::Overdue => ("em atraso", Tone::Danger),
        InstanceStatus::DueSoon => ("a vencer", Tone::Warn),
        InstanceStatus::Future => ("agendada", Tone::Slate),
        InstanceStatus::Done => ("tratada", Tone::Ok),
        InstanceStatus::Untracked => ("histórico", Tone::Na),
        InstanceStatus::NotApplicable => ("não aplicável", Tone::Na),
    };
    widgets::pill(ui, label, tone);
}

/// The badge for a verification state that is not the happy one.
fn verification_label(verification: Verification) -> &'static str {
    match verification {
        Verification::Verified => "verificada",
        Verification::Partial => "parcial",
        Verification::Unverified => "por verificar",
        Verification::Stale => "desatualizada",
    }
}

/// Whether the person can still do something about it.
fn is_actionable(status: InstanceStatus) -> bool {
    matches!(
        status,
        InstanceStatus::Overdue | InstanceStatus::DueSoon | InstanceStatus::Future
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(status: InstanceStatus) -> ObligationInstance {
        ObligationInstance {
            id: "iva.dp.trimestral@2026-05-20".to_owned(),
            rule_id: "iva.dp.trimestral".to_owned(),
            kind: vnf_core::types::ObligationKind::Declare,
            authority: vnf_core::types::Authority::At,
            tax: "IVA".to_owned(),
            title: "Declaração periódica de IVA".to_owned(),
            period_label: "1.º trimestre".to_owned(),
            period_start: None,
            period_end: None,
            due_date: "2026-05-20".to_owned(),
            due_date_source: vnf_core::types::DueDateSource::Official,
            adjusted_for_weekend: false,
            provisional: false,
            status,
            not_applicable_reason: None,
            verification: Verification::Verified,
            verify_note: None,
            legal_basis: vec!["Art. 41.º CIVA".to_owned()],
            source_ids: vec![],
            portal_url: None,
            documents_to_keep: vec![],
            penalty_note: None,
            discrepancies: vec![],
            checklists: vec![],
        }
    }

    #[test]
    fn the_filters_partition_the_agenda() {
        let instances: Vec<ObligationInstance> = [
            InstanceStatus::Overdue,
            InstanceStatus::DueSoon,
            InstanceStatus::Future,
            InstanceStatus::Done,
            InstanceStatus::Untracked,
            InstanceStatus::NotApplicable,
        ]
        .into_iter()
        .map(instance)
        .collect();
        for filter in Filter::ALL {
            let kept: Vec<&ObligationInstance> = instances.iter().filter(|item| filter.keeps(item)).collect();
            // Every filter that names a status keeps exactly that status.
            if filter != Filter::All {
                assert_eq!(kept.len(), 1, "{:?} kept {}", filter, kept.len());
            } else {
                assert_eq!(kept.len(), instances.len());
            }
        }
    }

    #[test]
    fn every_filter_says_what_it_shows() {
        for filter in Filter::ALL {
            assert!(!filter.label().is_empty());
        }
    }

    #[test]
    fn only_unfinished_work_is_actionable() {
        assert!(is_actionable(InstanceStatus::Overdue));
        assert!(is_actionable(InstanceStatus::DueSoon));
        assert!(is_actionable(InstanceStatus::Future));
        assert!(!is_actionable(InstanceStatus::Done));
        assert!(!is_actionable(InstanceStatus::NotApplicable));
        // History is not work: the person adopted the tool in September.
        assert!(!is_actionable(InstanceStatus::Untracked));
    }
}
