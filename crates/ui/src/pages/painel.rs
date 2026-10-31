//! The dashboard: what is due, what it adds up to, and the fiscal situation.
//!
//! Three questions, in the order a person has them when they open the application:
//! is anything wrong or late; how much have I made and what is my share; and is the
//! framework the agenda is built on still the one I declared.

use egui::Ui;
use vnf_core::estimate::{PeriodTotals, QuarterReport};
use vnf_core::flags::{Flag, FlagSeverity};
use vnf_core::money::{format_bp_as_percent, format_eur};
use vnf_core::profile::ProblemLevel;
use vnf_core::types::{CaeRole, IrsRegime, IvaRegime};
use vnf_store::model::DashboardModel;

use crate::app::Page;
use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Def, Kpi, Tone};

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel, today: &str, year: i64) -> Option<Action> {
    let mut action = None;
    ui.add_space(4.0);
    if let Some(requested) = alert_strip(ui, model) {
        action = Some(requested);
    }
    ui.add_space(18.0);

    match &model.profile {
        Some(_) => {
            figures(ui, model, year);
            ui.add_space(22.0);
            alert_list(ui, model);
            ui.add_space(22.0);
            framework(ui, model, today);
        }
        None => onboarding(ui, model, &mut action),
    }
    ui.add_space(24.0);
    action
}

/// The bar at the top: the one thing worth saying before anything else.
fn alert_strip(ui: &mut Ui, model: &DashboardModel) -> Option<Action> {
    let summary = &model.flag_summary;
    let operational = model
        .agenda
        .iter()
        .filter(|instance| {
            matches!(
                instance.status,
                vnf_core::types::InstanceStatus::Overdue | vnf_core::types::InstanceStatus::DueSoon
            )
        })
        .count();
    let overdue = model
        .agenda
        .iter()
        .filter(|instance| instance.status == vnf_core::types::InstanceStatus::Overdue)
        .count();

    if summary.total == 0 {
        widgets::accented_card(ui, Tone::Ok, |ui| {
            ui.horizontal(|ui| {
                widgets::severity(ui, "· sem alertas", Tone::Ok);
                ui.label(
                    egui::RichText::new("Nada a assinalar")
                        .size(theme::FIGURE_SMALL)
                        .strong(),
                );
            });
            widgets::caption(
                ui,
                &format!(
                    "{operational} obrigações nos próximos 30 dias · {overdue} em atraso · motor de \
                     regras pt/{} executado localmente",
                    model.pack.summary.pack_version
                ),
            );
        });
        return None;
    }

    let tone = if summary.urgent > 0 {
        Tone::Danger
    } else {
        Tone::Warn
    };
    let palette = theme::palette(ui.ctx());
    let mut requested = None;
    widgets::accented_card(ui, tone, |ui| {
        ui.horizontal_wrapped(|ui| {
            widgets::severity(
                ui,
                if summary.urgent > 0 {
                    "▲ atenção"
                } else {
                    "● a rever"
                },
                tone,
            );
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} urgentes · {} de atenção · {} informativos",
                        summary.urgent, summary.attention, summary.info
                    ))
                    .size(theme::FIGURE_SMALL)
                    .strong()
                    .color(palette.text),
                );
                widgets::caption(
                    ui,
                    &format!(
                        "{operational} obrigações nos próximos 30 dias · {overdue} em atraso · {} \
                         verificações ativas",
                        summary.total
                    ),
                );
            });
        });
        ui.add_space(6.0);
        if widgets::primary_button(ui, "Ver o plano").clicked() {
            requested = Some(Action::GoTo(Page::Agenda));
        }
    });
    requested
}

/// The five figures.
fn figures(ui: &mut Ui, model: &DashboardModel, year: i64) {
    let gross = total(model, |totals| totals.base_cents);
    let retained = total(model, |totals| totals.retencao_sofrida_cents);
    let iva = total(model, |totals| totals.iva_liquidado_cents);
    let documents: usize = model
        .quarters
        .iter()
        .map(|quarter| quarter.totals.invoice_count)
        .sum();
    let current_quarter = current_quarter(&model.meta.today);
    let quarter = model
        .quarters
        .iter()
        .find(|report| report.quarter == current_quarter)
        .or_else(|| model.quarters.first());

    let label_gross = format!("Rendimento bruto {year}");
    let label_ss = format!("Contribuições SS (T{current_quarter})");
    let foot_documents = format!("{documents} documento(s)");
    let foot_regime = format!(
        "regime: {}",
        model
            .profile
            .as_ref()
            .map(|profile| regime_label(profile.iva.regime))
            .unwrap_or("—")
    );
    let value_gross = format_eur(gross);
    let value_retained = format_eur(retained);
    let value_iva = format_eur(iva);
    let value_ss = quarter
        .map(|report| format_eur(report.social_security.contribution_cents))
        .unwrap_or_else(|| "—".to_owned());
    let ss_support = quarter.map(|report| {
        format!(
            "{} do rendimento de serviços, à taxa de {}",
            format_bp_as_percent(report.social_security.relevant_income_share_bp),
            format_bp_as_percent(report.social_security.contribution_rate_bp)
        )
    });
    let ss_foot =
        quarter.map(|report| format!("{}.º trimestre · {}–{}", report.quarter, report.start, report.end));
    let reserve = format!(
        "{} – {}",
        format_eur(model.reserve.low_cents),
        format_eur(model.reserve.high_cents)
    );
    let reserve_foot = format!("estimativa · regras pt/{}", model.pack.summary.pack_version);
    let reserve_support =
        "Intervalo indicativo a reservar para IRS e Segurança Social do rendimento do exercício";

    let items = vec![
        Kpi {
            label: &label_gross,
            value: &value_gross,
            support: Some("Base de incidência dos recibos emitidos no exercício"),
            foot: Some(&foot_documents),
        },
        Kpi {
            label: "Retenções na fonte",
            value: &value_retained,
            support: None,
            foot: Some("soma dos quatro trimestres"),
        },
        Kpi {
            label: "IVA liquidado (acum.)",
            value: &value_iva,
            support: None,
            foot: Some(&foot_regime),
        },
        Kpi {
            label: &label_ss,
            value: &value_ss,
            support: ss_support.as_deref(),
            foot: ss_foot.as_deref(),
        },
        Kpi {
            label: "Reserva recomendada",
            value: &reserve,
            support: Some(reserve_support),
            foot: Some(&reserve_foot),
        },
    ];

    widgets::section(
        ui,
        "Indicadores do exercício",
        Some(&format!(
            "Acumulado a {} · base: faturação registada no cofre local",
            pt_date(&model.meta.today)
        )),
        Some(
            "Um resumo do ano até hoje. Nenhum destes valores é uma previsão: todos saem das faturas \
             que registaste e das constantes do pacote de regras, e cada um diz em que se baseia.",
        ),
    );
    widgets::kpi_row(ui, &items);
    if model.invoices.is_empty() {
        ui.add_space(8.0);
        widgets::footnote(
            ui,
            "Os valores a zero significam que ainda não há faturação registada neste exercício — não \
             que o cálculo tenha falhado.",
        );
    }
}

/// The list of flags, each with the law it rests on.
fn alert_list(ui: &mut Ui, model: &DashboardModel) {
    if model.flags.is_empty() {
        return;
    }
    widgets::section(
        ui,
        "Alertas do sistema fiscal",
        Some(&format!(
            "{} verificações ativas · motor de regras pt/{} · executado localmente, sem ligação à \
             internet",
            model.flag_summary.total, model.pack.summary.pack_version
        )),
        None,
    );
    widgets::card(ui, |ui| {
        let palette = theme::palette(ui.ctx());
        for (index, flag) in model.flags.iter().enumerate() {
            if index > 0 {
                ui.add_space(10.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().hline(
                    rect.x_range(),
                    rect.center().y,
                    egui::Stroke::new(1.0, palette.border),
                );
                ui.add_space(10.0);
            }
            flag_row(ui, flag);
        }
    });
}

/// One flag.
fn flag_row(ui: &mut Ui, flag: &Flag) {
    let palette = theme::palette(ui.ctx());
    let tone = match flag.severity {
        FlagSeverity::Urgent => Tone::Danger,
        FlagSeverity::Attention => Tone::Warn,
        FlagSeverity::Info => Tone::Slate,
    };
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::Vec2::new(96.0, 0.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                widgets::severity(
                    ui,
                    match flag.severity {
                        FlagSeverity::Urgent => "urgente",
                        FlagSeverity::Attention => "atenção",
                        FlagSeverity::Info => "informativo",
                    },
                    tone,
                );
            },
        );
        ui.allocate_ui_with_layout(
            egui::Vec2::new(ui.available_width(), 0.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.label(
                    egui::RichText::new(&flag.title)
                        .size(theme::BODY)
                        .strong()
                        .color(palette.text),
                );
                ui.scope(|ui| {
                    ui.set_max_width(820.0);
                    ui.label(
                        egui::RichText::new(&flag.detail)
                            .size(theme::BODY_SMALL)
                            .color(palette.text_2),
                    );
                });
                if !flag.legal_basis.is_empty() || !flag.source_ids.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        if !flag.legal_basis.is_empty() {
                            widgets::caption(ui, &flag.legal_basis.join(" · "));
                        }
                        for source in &flag.source_ids {
                            widgets::badge(ui, source, Tone::Slate);
                        }
                    });
                }
            },
        );
    });
}

/// The declared framework, which is what decides the agenda.
fn framework(ui: &mut Ui, model: &DashboardModel, today: &str) {
    let Some(profile) = &model.profile else {
        return;
    };
    let palette = theme::palette(ui.ctx());
    let principal = profile
        .activity
        .cae
        .iter()
        .find(|entry| entry.role == CaeRole::Principal)
        .map(|entry| format!("{} — {}", entry.code, entry.description))
        .unwrap_or_else(|| "Nenhum CAE principal registado no perfil.".to_owned());
    let secondary: Vec<String> = profile
        .activity
        .cae
        .iter()
        .filter(|entry| entry.role == CaeRole::Secondary)
        .map(|entry| format!("{} — {}", entry.code, entry.description))
        .collect();

    let start_small = profile
        .tracking_start
        .as_ref()
        .map(|start| format!("Acompanhamento registado a partir de {start}"));
    let coefficient_small = format!(
        "Coeficiente registado no perfil: {}",
        vnf_core::money::format_bp_as_coefficient(profile.irs.coefficient_bp)
    );
    let secondary_text = secondary.join(" · ");
    let mut rows = vec![
        Def {
            label: "CAE principal",
            value: &principal,
            small: None,
        },
        Def {
            label: "Regime IRS",
            value: regime_irs(profile.irs.regime),
            small: Some(&coefficient_small),
        },
        Def {
            label: "IVA",
            value: regime_label(profile.iva.regime),
            small: Some("Regime de IVA registado no perfil · alterações são declarativas"),
        },
        Def {
            label: "Início de atividade",
            value: &profile.activity.start_date,
            small: start_small.as_deref(),
        },
    ];
    if !secondary.is_empty() {
        rows.insert(
            1,
            Def {
                label: "CAE secundário",
                value: &secondary_text,
                small: None,
            },
        );
    }
    let turnover = match profile.activity.turnover_previous_year_cents {
        Some(cents) => format_eur(cents),
        None => "ano anterior em falta".to_owned(),
    };
    rows.push(Def {
        label: "Volume de negócios",
        value: &turnover,
        small: Some("Declarado por ti; nunca inferido do que está registado"),
    });

    widgets::section(
        ui,
        "Enquadramento fiscal",
        Some(&format!(
            "Cartão de identidade da atividade · hoje {}",
            pt_date(today)
        )),
        Some(
            "É a tua situação declarada às Finanças, e é o que decide que obrigações aparecem na \
             agenda. Corrigir aqui um valor muda a agenda; deixar um valor em branco faz a aplicação \
             dizer que não consegue avaliar essa parte, em vez de a assumir.",
        ),
    );
    widgets::card(ui, |ui| widgets::defs(ui, &rows));

    if !model.profile_problems.is_empty() {
        ui.add_space(10.0);
        widgets::accented_card(ui, Tone::Warn, |ui| {
            for problem in &model.profile_problems {
                ui.horizontal_top(|ui| {
                    let tone = if problem.level == ProblemLevel::Error {
                        Tone::Danger
                    } else {
                        Tone::Warn
                    };
                    widgets::severity(
                        ui,
                        if problem.level == ProblemLevel::Error {
                            "erro"
                        } else {
                            "aviso"
                        },
                        tone,
                    );
                    ui.label(
                        egui::RichText::new(&problem.message)
                            .size(theme::BODY_SMALL)
                            .color(palette.text_2),
                    );
                });
            }
        });
    }
    if !model.missing_inputs.is_empty() {
        ui.add_space(10.0);
        widgets::notes(ui, Tone::Warn, &model.missing_inputs);
    }
}

/// The first run: what is missing, and where it goes.
fn onboarding(ui: &mut Ui, model: &DashboardModel, action: &mut Option<Action>) {
    let palette = theme::palette(ui.ctx());
    widgets::section(
        ui,
        "Primeiros passos",
        Some("O painel não inventa dados"),
        Some(
            "Faltam os dados que só tu tens: os da tua declaração de início de atividade. Enquanto \
             não estiverem aqui, a aplicação prefere não mostrar nada a mostrar estimativas \
             inventadas.",
        ),
    );
    widgets::accented_card(ui, Tone::Accent, |ui| {
        ui.label(
            egui::RichText::new("O cofre está vazio")
                .size(theme::FIGURE_SMALL)
                .strong()
                .color(palette.text),
        );
        ui.add_space(4.0);
        ui.scope(|ui| {
            ui.set_max_width(720.0);
            ui.label(
                egui::RichText::new(
                    "Este cofre ainda não tem perfil. Com o perfil preenchido, a aplicação passa a \
                     saber que obrigações a lei te impõe, quando vencem, e quanto deves reservar \
                     para IVA, IRS e Segurança Social.",
                )
                .size(theme::BODY)
                .color(palette.text_2),
            );
        });
        ui.add_space(6.0);
        for line in [
            "o teu NIF e o nome da atividade",
            "o regime de IVA e o regime de IRS que declaraste",
            "a data de início de atividade",
            "o volume de negócios do ano anterior, se quiseres avaliar o art. 53.º",
        ] {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.0, palette.accent);
                ui.label(
                    egui::RichText::new(line)
                        .size(theme::BODY_SMALL)
                        .color(palette.text_2),
                );
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Preencher o perfil").clicked() {
                *action = Some(Action::OpenProfile);
            }
            if widgets::button(ui, "Recarregar").clicked() {
                *action = Some(Action::Reload);
            }
        });
    });
    ui.add_space(12.0);
    widgets::footnote(
        ui,
        &format!(
            "O cofre está em {}. O pacote de regras é o pt/{} ({} fontes, {} obrigações).",
            model.meta.data_dir,
            model.pack.summary.pack_version,
            model.pack.summary.sources,
            model.pack.summary.obligations
        ),
    );
}

/// Sum one figure across the four quarters.
fn total(model: &DashboardModel, pick: impl Fn(&PeriodTotals) -> i64) -> i64 {
    model.quarters.iter().map(|quarter| pick(&quarter.totals)).sum()
}

/// The quarter the date falls in.
#[must_use]
pub fn current_quarter(today: &str) -> i64 {
    let month: i64 = today.get(5..7).and_then(|month| month.parse().ok()).unwrap_or(1);
    ((month + 2) / 3).clamp(1, 4)
}

/// `2026-09-20` as `20/09/2026`.
fn pt_date(iso: &str) -> String {
    match (iso.get(8..10), iso.get(5..7), iso.get(0..4)) {
        (Some(day), Some(month), Some(year)) => format!("{day}/{month}/{year}"),
        _ => iso.to_owned(),
    }
}

/// The label for an IVA regime.
fn regime_label(regime: IvaRegime) -> &'static str {
    match regime {
        IvaRegime::IsentoArt53 => "Isenção do art. 53.º do CIVA",
        IvaRegime::Trimestral => "Regime normal — declaração trimestral",
        IvaRegime::Mensal => "Regime normal — declaração mensal",
    }
}

/// The label for an IRS regime.
fn regime_irs(regime: IrsRegime) -> &'static str {
    match regime {
        IrsRegime::Simplificado => "Regime simplificado",
        IrsRegime::Organizada => "Contabilidade organizada",
    }
}

/// The quarters of a year, in order.
#[must_use]
pub fn quarters_of(model: &DashboardModel, year: i64) -> Vec<&QuarterReport> {
    model
        .quarters
        .iter()
        .filter(|quarter| quarter.year == year)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quarter_comes_from_the_month() {
        assert_eq!(current_quarter("2026-01-05"), 1);
        assert_eq!(current_quarter("2026-03-31"), 1);
        assert_eq!(current_quarter("2026-04-01"), 2);
        assert_eq!(current_quarter("2026-09-20"), 3);
        assert_eq!(current_quarter("2026-12-31"), 4);
    }

    #[test]
    fn a_date_is_written_the_portuguese_way() {
        assert_eq!(pt_date("2026-09-20"), "20/09/2026");
        assert_eq!(pt_date("invalida"), "invalida");
    }

    #[test]
    fn the_regime_labels_are_the_declared_ones() {
        assert_eq!(
            regime_label(IvaRegime::IsentoArt53),
            "Isenção do art. 53.º do CIVA"
        );
        assert_eq!(regime_irs(IrsRegime::Simplificado), "Regime simplificado");
    }
}
