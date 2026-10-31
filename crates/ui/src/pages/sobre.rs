//! About: what this application does, and what it refuses to do.
//!
//! The six guarantees are the model's, not this page's, so the documentation and the
//! window cannot drift apart on the promise. Everything else here is the licence, the
//! version, and where the source lives.

use egui::Ui;
use vnf_store::model::DashboardModel;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Def, Tone};

/// The licence the application is distributed under.
pub const LICENCE: &str = "AGPL-3.0-only";

/// Where the source lives.
pub const REPOSITORY: &str = "https://github.com/vecnode/vn-finance";

/// Draw the page.
pub fn show(ui: &mut Ui, model: &DashboardModel) -> Option<Action> {
    let action = None;
    widgets::section(
        ui,
        "Sobre",
        Some(&format!("vn-finance {}", model.meta.version)),
        Some(
            "Um assistente fiscal local para trabalhadores independentes em Portugal. Calcula o que \
             a lei obriga e quanto deves reservar. Não substitui um contabilista e não fala com \
             nenhum serviço público.",
        ),
    );

    widgets::section(ui, "O que esta aplicação nunca faz", Some("seis promessas"), None);
    widgets::card(ui, |ui| {
        for (index, line) in model.guarantees.iter().enumerate() {
            ui.horizontal_top(|ui| {
                let palette = theme::palette(ui.ctx());
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                ui.painter()
                    .circle_stroke(rect.center(), 6.0, egui::Stroke::new(1.0, palette.ok));
                ui.painter().circle_filled(rect.center(), 2.5, palette.ok);
                ui.scope(|ui| {
                    ui.set_max_width(760.0);
                    ui.label(
                        egui::RichText::new(line)
                            .size(theme::BODY_SMALL)
                            .color(palette.text_2),
                    );
                });
            });
            if index + 1 < model.guarantees.len() {
                ui.add_space(6.0);
            }
        }
    });

    ui.add_space(16.0);
    widgets::section(
        ui,
        "O que faz",
        None,
        Some(
            "Constrói a agenda do ano a partir do perfil e do pacote de regras; calcula IVA, \
             retenções, contribuições e uma reserva a partir dos recibos registados; arquiva \
             comprovativos com o hash do conteúdo; e mostra a fonte de cada número.",
        ),
    );

    ui.add_space(12.0);
    let files = model.vault.files.to_string();
    let obligations = model.pack.summary.obligations.to_string();
    let sources = model.pack.summary.sources.to_string();
    let checksum = if model.meta.pack_checksum.len() >= 12 {
        model.meta.pack_checksum[..12].to_owned()
    } else {
        model.meta.pack_checksum.clone()
    };
    widgets::card(ui, |ui| {
        widgets::defs(
            ui,
            &[
                Def {
                    label: "Versão",
                    value: &model.meta.version,
                    small: Some(LICENCE),
                },
                Def {
                    label: "Código",
                    value: REPOSITORY,
                    small: Some("o código é o mesmo que corre"),
                },
                Def {
                    label: "Pacote de regras",
                    value: &model.pack.summary.pack_version,
                    small: Some(&format!("{obligations} obrigações · {sources} fontes")),
                },
                Def {
                    label: "Checksum do pacote",
                    value: &checksum,
                    small: None,
                },
                Def {
                    label: "Cofre",
                    value: &model.meta.data_dir,
                    small: Some(&format!("{files} ficheiro(s) · só neste computador")),
                },
            ],
        );
    });

    ui.add_space(14.0);
    widgets::accented_card(ui, Tone::Slate, |ui| {
        ui.label(
            egui::RichText::new("Sem telemetria, sem contas, sem servidores")
                .size(theme::BODY)
                .strong(),
        );
        widgets::caption(
            ui,
            "A aplicação não tem código de rede. Não há um pedido a fazer, nem um servidor para \
             onde o fazer. Uma cópia de segurança é copiar a pasta do cofre.",
        );
    });

    ui.add_space(12.0);
    widgets::footnote(
        ui,
        "Os valores e prazos vêm do pacote de regras embutido no binário, com a data de recolha de \
         cada fonte. Um pacote desatualizado não inventa datas: diz que não as tem.",
    );
    let _ = ui;
    action
}
