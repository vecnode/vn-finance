//! The law: every source the pack cites, and every obligation it declares.
//!
//! This page exists so that no figure elsewhere in the application is unfalsifiable.
//! A number on the IVA page traces back to a constant, the constant to a source, and
//! the source to a URL with a retrieval date.

use egui::Ui;
use vnf_core::money::format_bp_as_percent;
use vnf_core::rollover::PackDiff;
use vnf_core::rules::FreshnessStatus;
use vnf_core::sources::SourceState;
use vnf_core::types::{PeriodicityType, ProblemLevel, SourceTier, Verification};
use vnf_store::model::{CorpusView, DashboardModel};
use vnf_store::pack::PackOrigin;

use crate::pages::Action;
use crate::pages::cofre::bytes as human_bytes;
use crate::theme;
use crate::widgets::{self, Column, Def, Kpi, Tone};

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
    // Where the pack came from, and whether it is this year's: a pack compiled in is
    // the fallback, and a pack read from a folder is the one a person can update
    // without waiting for a release. Saying which is which is the whole point of the
    // line, so it is not a decoration.
    let origin = origin_label(model.pack.origin);
    let origin_foot = if model.pack.exact {
        origin.to_owned()
    } else {
        format!("{origin} · não é o ano pedido")
    };
    widgets::kpi_rows(
        ui,
        &[
            Kpi {
                label: "Versão do pacote",
                value: &version,
                support: None,
                foot: Some(&origin_foot),
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

    if let Some(changes) = &model.rules.changes {
        ui.add_space(14.0);
        changes_section(ui, changes);
    }

    if !model.pack.rejected.is_empty() {
        ui.add_space(12.0);
        widgets::accented_card(ui, Tone::Danger, |ui| {
            ui.label(
                egui::RichText::new("Um pacote de regras foi encontrado e recusado")
                    .size(theme::BODY)
                    .strong(),
            );
            for entry in &model.pack.rejected {
                widgets::caption(ui, &entry.path);
                ui.label(
                    egui::RichText::new(&entry.problem)
                        .size(theme::BODY_SMALL)
                        .color(theme::palette(ui.ctx()).text_2),
                );
            }
            widgets::caption(
                ui,
                "A janela abriu com o pacote descrito acima. Corrige ou remove o ficheiro recusado: \
                 um pacote inválido nunca é usado a meias, e o ano que ele devia cobrir fica \
                 marcado como provisório.",
            );
        });
    }

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
    corpus_section(ui, &model.rules.corpus);

    ui.add_space(18.0);
    widgets::section(
        ui,
        "Fontes citadas",
        Some(&format!("{} documentos", model.rules.sources.len())),
        None,
    );
    let source_columns = [
        Column::at("Id", 120.0),
        Column::at("Tipo", 74.0),
        Column::at("Autoridade", 132.0),
        Column::new("Título"),
        Column::at("Obtida", 96.0),
        Column::at("Cópia", 92.0),
    ];
    widgets::table(
        ui,
        &source_columns,
        &model.rules.sources,
        |_| false,
        |cells, row| {
            let source = &row.source;
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
            cells.custom(|ui| {
                let (label, tone) = source_state_label(row.check.state);
                widgets::badge(ui, label, tone);
                if row.check.passages_expected > 0 {
                    widgets::caption(
                        ui,
                        &format!(
                            "{}/{} passagens",
                            row.check.passages_found, row.check.passages_expected
                        ),
                    );
                }
            });
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
        Column::at("Base legal", 168.0),
    ];
    widgets::table(
        ui,
        &obligation_columns,
        &model.rules.obligations,
        |_| false,
        |cells, row| {
            let rule = &row.rule;
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
                if !row.citations.is_empty() {
                    let confirmed = row.confirmed();
                    let total = row.citations.len();
                    let (label, tone) = if confirmed == total {
                        ("citação conferida".to_owned(), Tone::Ok)
                    } else {
                        (format!("{confirmed}/{total} citações"), Tone::Warn)
                    };
                    widgets::badge(ui, &label, tone);
                }
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

/// What moved between the pack in use and the one compiled in.
///
/// A pack that arrives from a folder is a change to the law the application applies,
/// and the review of a change belongs next to the law, not in a terminal the person
/// running the window does not have. The same comparison is what `cargo xtask rules
/// diff` prints, so the two cannot disagree.
fn changes_section(ui: &mut Ui, changes: &PackDiff) {
    let subtitle = format!(
        "{} {} → {} {} · {} alteração(ões)",
        changes.from_year,
        changes.from_version,
        changes.to_year,
        changes.to_version,
        changes.total()
    );
    widgets::section(
        ui,
        "Alterações face ao pacote embutido",
        Some(&subtitle),
        Some(
            "O pacote em uso não é o que esta versão do programa traz. Esta é a diferença entre os \
             dois, para poder ser lida antes de se confiar num prazo.",
        ),
    );

    if changes.is_empty() {
        widgets::caption(
            ui,
            "O conteúdo é o mesmo; mudaram o ano, a versão ou a data de geração.",
        );
        return;
    }

    widgets::card(ui, |ui| {
        change_group(
            ui,
            "constantes",
            changes
                .constants
                .iter()
                .map(|change| {
                    format!(
                        "{}: {} → {}",
                        change.path,
                        change.from.as_deref().unwrap_or("—"),
                        change.to.as_deref().unwrap_or("—")
                    )
                })
                .collect(),
        );
        change_group(
            ui,
            "obrigações",
            changes
                .obligations_added
                .iter()
                .map(|id| format!("+ {id}"))
                .chain(changes.obligations_removed.iter().map(|id| format!("− {id}")))
                .collect(),
        );
        change_group(
            ui,
            "verificação",
            changes
                .verification
                .iter()
                .map(|change| {
                    format!(
                        "{}: {} → {}",
                        change.id,
                        vnf_core::rollover::verification_label(change.from),
                        vnf_core::rollover::verification_label(change.to)
                    )
                })
                .collect(),
        );
        change_group(
            ui,
            "datas publicadas",
            changes
                .dates
                .iter()
                .map(|change| format!("{}: {} → {}", change.id, change.from, change.to))
                .collect(),
        );
        change_group(
            ui,
            "fontes",
            changes
                .sources_added
                .iter()
                .map(|id| format!("+ {id}"))
                .chain(changes.sources_removed.iter().map(|id| format!("− {id}")))
                .chain(changes.sources_changed.iter().map(|change| {
                    format!(
                        "{} · {}: {} → {}",
                        change.id, change.field, change.from, change.to
                    )
                }))
                .collect(),
        );
    });
}

/// How many lines of one group the card shows before it stops counting.
const CHANGES_SHOWN: usize = 6;

/// One labelled group of changes, capped so the card stays a card.
fn change_group(ui: &mut Ui, title: &str, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    widgets::micro_label(ui, title, theme::palette(ui.ctx()).text_3);
    for line in lines.iter().take(CHANGES_SHOWN) {
        ui.label(
            egui::RichText::new(line)
                .size(theme::BODY_SMALL)
                .color(theme::palette(ui.ctx()).text_2),
        );
    }
    if lines.len() > CHANGES_SHOWN {
        widgets::caption(ui, &format!("e mais {}", lines.len() - CHANGES_SHOWN));
    }
}

/// The evidence: what was read, how it held up, and what is wrong with it.
///
/// This is the section that keeps the page honest. A citation is only worth the copy
/// behind it, so the page says how many sources have one, how many were compared, and
/// every passage that did not check out — in the place where the law is shown, not in
/// a log.
fn corpus_section(ui: &mut Ui, corpus: &CorpusView) {
    let total = corpus.verified + corpus.absent + corpus.failed + corpus.unproven + corpus.not_named;
    let documents = format!(
        "{} · {}",
        plural_documents(corpus.files),
        human_bytes(corpus.bytes)
    );
    widgets::section(
        ui,
        "Cópia local das fontes",
        Some(&documents),
        Some(
            "A lei que esta aplicação cita está guardada numa pasta e é comparada com o que o \
             pacote afirma: o hash de cada documento e as passagens que dele se citam. Sem esta \
             cópia, uma citação pode ser lida mas não conferida.",
        ),
    );

    if corpus.files == 0 {
        let root = corpus
            .root
            .clone()
            .unwrap_or_else(|| "rules/pt/sources".to_owned());
        widgets::empty(
            ui,
            "Sem cópia local das fontes",
            &format!(
                "Esta instalação não trouxe os documentos: esperava encontrá-los em {root}. As \
                 citações continuam a ser mostradas, mas nada neste computador as pode conferir."
            ),
        );
    } else {
        let root = corpus.root.clone().unwrap_or_else(|| "—".to_owned());
        let confirmed = format!("{} de {}", corpus.verified, total);
        let breakdown = format!(
            "{} em falta · {} não conferem · {} sem hash · {} sem cópia",
            corpus.absent, corpus.failed, corpus.unproven, corpus.not_named
        );
        widgets::card(ui, |ui| {
            widgets::defs(
                ui,
                &[
                    Def {
                        label: "Pasta",
                        value: &root,
                        small: None,
                    },
                    Def {
                        label: "Fontes conferidas",
                        value: &confirmed,
                        small: Some(&breakdown),
                    },
                ],
            );
        });
    }

    if !corpus.notes.is_empty() {
        ui.add_space(8.0);
        widgets::notes(ui, Tone::Warn, &corpus.notes);
    }

    if !corpus.problems.is_empty() {
        ui.add_space(8.0);
        widgets::card(ui, |ui| {
            widgets::micro_label(
                ui,
                "o que a conferência encontrou",
                theme::palette(ui.ctx()).text_3,
            );
            for problem in &corpus.problems {
                let colour = match problem.level {
                    ProblemLevel::Error => theme::palette(ui.ctx()).danger,
                    ProblemLevel::Warning => theme::palette(ui.ctx()).text_2,
                };
                ui.label(
                    egui::RichText::new(&problem.message)
                        .size(theme::BODY_SMALL)
                        .color(colour),
                );
            }
        });
    }
}

/// The badge for one source's local copy.
#[must_use]
pub fn source_state_label(state: SourceState) -> (&'static str, Tone) {
    match state {
        SourceState::Verified => ("conferida", Tone::Ok),
        SourceState::Unproven => ("sem hash", Tone::Warn),
        SourceState::Absent => ("em falta", Tone::Warn),
        SourceState::Failed => ("não confere", Tone::Danger),
        SourceState::NotNamed => ("sem cópia", Tone::Slate),
    }
}

/// A document count with its noun agreeing.
fn plural_documents(count: usize) -> String {
    if count == 1 {
        "1 documento".to_owned()
    } else {
        format!("{count} documentos")
    }
}

/// Where the rule pack came from, in words a person reads.///
/// "embutido no binário" and "pasta junto ao programa" are different promises: the
/// first travels with the executable and cannot be edited, the second is a file the
/// person can replace. The page must not let them look the same.
#[must_use]
pub fn origin_label(origin: PackOrigin) -> &'static str {
    match origin {
        PackOrigin::Flag => "escolhido ao abrir",
        PackOrigin::Env => "variável de ambiente",
        PackOrigin::Beside => "pasta junto ao programa",
        PackOrigin::SourceTree => "pasta do código",
        PackOrigin::Embedded => "embutido no binário",
    }
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

    #[test]
    fn every_pack_origin_says_where_the_bytes_came_from() {
        for origin in [
            PackOrigin::Flag,
            PackOrigin::Env,
            PackOrigin::Beside,
            PackOrigin::SourceTree,
            PackOrigin::Embedded,
        ] {
            assert!(!origin_label(origin).is_empty());
        }
        // The two that are most easily confused say different things.
        assert_ne!(
            origin_label(PackOrigin::Embedded),
            origin_label(PackOrigin::Beside)
        );
    }
}
