//! The vault: where the files live, and what is in it.
//!
//! This page is where the privacy claim becomes checkable rather than asserted: the
//! folder, how it was chosen, whether it sits somewhere dangerous, and every archived
//! file with its content hash.

use egui::Ui;
use vnf_store::model::DashboardModel;
use vnf_store::risk::RiskLevel;

use crate::pages::Action;
use crate::theme;
use crate::widgets::{self, Column, Def, Tone};

/// The table's columns.
fn columns() -> [Column; 5] {
    [
        Column::new("Ficheiro"),
        Column::at("Tipo", 96.0),
        Column::at("Arquivado", 168.0),
        Column::at("Obrigação", 150.0),
        Column::at("Fatura", 120.0),
    ]
}

/// Draw the page.
pub fn show(
    ui: &mut Ui,
    model: &DashboardModel,
    archive_path: &mut String,
    archive_kind: &str,
) -> Option<Action> {
    let mut action = None;
    widgets::section(
        ui,
        "Cofre de documentos",
        Some(&format!(
            "{} ficheiro(s) · {}",
            model.vault.files,
            bytes(model.vault.bytes)
        )),
        Some(
            "Tudo o que a aplicação guarda vive nesta pasta, neste computador. Não há sincronização \
             nem servidor: uma cópia de segurança é copiar a pasta, e nada sai daqui sozinho.",
        ),
    );

    let source = match model.meta.data_dir_source {
        vnf_store::location::VaultLocationSource::Pointer => "escolhido por ti e memorizado",
        vnf_store::location::VaultLocationSource::Env => "variável de ambiente",
        vnf_store::location::VaultLocationSource::Flag => "indicada ao abrir",
        vnf_store::location::VaultLocationSource::Default => "localização predefinida",
    };
    let risk = match model.meta.data_dir_risk.level {
        RiskLevel::None => "fora de qualquer repositório git",
        RiskLevel::Warning => "dentro de um repositório git — confirma que está ignorado",
        RiskLevel::Fatal => "dentro do repositório da aplicação — recusado",
    };
    widgets::card(ui, |ui| {
        widgets::defs(
            ui,
            &[
                Def {
                    label: "Pasta do cofre",
                    value: &model.meta.data_dir,
                    small: Some(source),
                },
                Def {
                    label: "Escolha memorizada em",
                    value: if model.meta.data_dir_pointer_path.is_empty() {
                        "—"
                    } else {
                        &model.meta.data_dir_pointer_path
                    },
                    small: Some("o ficheiro que aponta para a pasta que escolheste"),
                },
                Def {
                    label: "Risco",
                    value: risk,
                    small: model.meta.data_dir_risk.message.as_deref(),
                },
            ],
        );
    });

    ui.add_space(16.0);
    if model.vault.entries.is_empty() {
        widgets::empty(
            ui,
            "Sem documentos arquivados",
            "Guarda aqui o comprovativo de cada obrigação que cumpres. Um recibo em PDF também \
             serve de prova de pagamento.",
        );
    } else {
        widgets::section(
            ui,
            "Arquivo",
            Some(&format!("{} entradas", model.vault.entries.len())),
            None,
        );
        widgets::table(
            ui,
            &columns(),
            &model.vault.entries,
            |_| false,
            |cells, entry| {
                cells.custom(|ui| {
                    ui.label(
                        egui::RichText::new(&entry.file)
                            .size(theme::MONO)
                            .monospace()
                            .color(theme::palette(ui.ctx()).text),
                    );
                    widgets::caption(
                        ui,
                        &format!("sha256 {}", &entry.sha256[..12.min(entry.sha256.len())]),
                    );
                });
                cells.custom(|ui| widgets::badge(ui, &entry.kind, Tone::Slate));
                cells.mono(&entry.added_at);
                cells.custom(|ui| match &entry.obligation_id {
                    Some(id) => widgets::badge(ui, id, Tone::Accent),
                    None => widgets::caption(ui, "—"),
                });
                cells.custom(|ui| match &entry.invoice_id {
                    Some(id) => widgets::badge(ui, id, Tone::Slate),
                    None => widgets::caption(ui, "—"),
                });
            },
        );
    }

    ui.add_space(16.0);
    widgets::section(
        ui,
        "Arquivar um documento",
        Some("por caminho no computador"),
        Some(
            "Indica o caminho de um ficheiro que já tens. A aplicação copia-o para o cofre, com um \
             nome que inclui o hash do conteúdo, e regista-o no índice. O original fica onde está.",
        ),
    );
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(archive_path)
                    .hint_text("C:\\caminho\\para\\comprovativo.pdf")
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Body),
            );
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            widgets::micro_label(ui, "tipo", theme::palette(ui.ctx()).text_3);
            for kind in ["comprovativo", "declaracao", "fatura", "recibo"] {
                widgets::badge(
                    ui,
                    kind,
                    if kind == archive_kind {
                        Tone::Accent
                    } else {
                        Tone::Slate
                    },
                );
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let ready = !archive_path.trim().is_empty();
            if widgets::primary_button(ui, "Arquivar").clicked() && ready {
                action = Some(Action::Archive {
                    path: archive_path.trim().to_owned(),
                    kind: archive_kind.to_owned(),
                });
            }
            if !ready {
                widgets::caption(ui, "indica primeiro o caminho do ficheiro");
            }
            if widgets::button(ui, "Recarregar").clicked() {
                action = Some(Action::Reload);
            }
        });
    });

    ui.add_space(12.0);
    widgets::footnote(
        ui,
        "O índice guarda o hash de cada ficheiro, por isso um documento alterado no disco deixa de \
         corresponder ao registo. A aplicação não apaga originais nem move nada fora do cofre.",
    );
    action
}

/// Bytes, in the units a person reads.
#[must_use]
pub fn bytes(count: u64) -> String {
    if count < 1024 {
        format!("{count} B")
    } else if count < 1024 * 1024 {
        format!("{:.1} kB", count as f64 / 1024.0)
    } else {
        format!("{:.1} MB", count as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_written_in_units_a_person_reads() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2.0 kB");
        assert_eq!(bytes(3 * 1024 * 1024), "3.0 MB");
    }
}
