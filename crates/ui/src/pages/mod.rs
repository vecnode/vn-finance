//! The pages.
//!
//! A page draws and returns what it wants done; it never writes. That is the browser
//! panel's best property — every write produced a new model and the whole panel
//! re-rendered from it — turned into a type, so a page cannot show a figure the vault
//! has not returned.

pub mod agenda;
pub mod cofre;
pub mod diagnostico;
pub mod irs;
pub mod iva;
pub mod painel;
pub mod profile;
pub mod receipt_form;
pub mod recibos;
pub mod regras;
pub mod seguranca_social;
pub mod sobre;

use egui::Ui;
use vnf_core::profile::ProfileDraft;

use crate::app::Page;
use crate::widgets;

/// What a page asks the window to do.
///
/// Deliberately small: a page may ask for a recomputation, a different page, a filter,
/// or one specific write. Everything else is the window's business, and every write
/// goes through the session rather than through a widget.
#[derive(Debug, Clone)]
pub enum Action {
    /// Rebuild the model from the vault.
    Reload,
    /// Show another page.
    GoTo(Page),
    /// Say something in the status line.
    Notice(String),
    /// Record that an obligation was handled.
    Complete {
        /// The instance id.
        id: String,
        /// The rule it came from.
        rule_id: String,
        /// The date it fell due.
        due_date: String,
    },
    /// Which agenda view to show.
    SetFilter(agenda::Filter),
    /// Which quarter the Segurança Social page shows.
    SetQuarter(i64),
    /// Archive a file the person names by path.
    Archive {
        /// Where the file is now.
        path: String,
        /// What kind of document it is.
        kind: String,
    },
    /// Open the profile form.
    OpenProfile,
    /// Save what the form holds.
    SaveProfile {
        /// The declarations the form collected.
        draft: Box<ProfileDraft>,
        /// The simplified-regime coefficient, in basis points.
        coefficient_bp: i64,
    },
    /// Close the profile form without saving.
    CloseProfile,
    /// Ask the system to open a directory chooser.
    ChooseVault,
    /// Open the receipt-import form.
    OpenReceipt,
    /// Read a PDF and show what it said.
    ParseReceipt {
        /// Where the file is.
        path: String,
    },
    /// Register the invoice the form is confirming.
    SaveReceipt {
        /// What the person confirmed.
        confirmation: Box<vnf_core::receipt::ReceiptConfirmation>,
    },
    /// Close the receipt form without registering anything.
    CloseReceipt,
}

/// The page a person reaches when the figures they asked for need a profile.
///
/// One paragraph, in one place: five pages need the same explanation, and five copies
/// of it would drift.
pub fn needs_profile(ui: &mut Ui, page: Page, action: &mut Option<Action>) {
    widgets::section(ui, page.label(), None, Some(page.subtitle()));
    widgets::accented_card(ui, crate::widgets::Tone::Accent, |ui| {
        ui.label(
            egui::RichText::new("Esta página precisa do perfil")
                .size(crate::theme::BODY)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Os números desta página dependem do que declaraste às Finanças. Sem isso, a \
                 aplicação não consegue saber que obrigações se aplicam nem quanto deves reservar — \
                 e prefere dizê-lo a mostrar zeros que parecem dados.",
            )
            .size(crate::theme::BODY_SMALL),
        );
        ui.add_space(8.0);
        if widgets::primary_button(ui, "Preencher o perfil").clicked() {
            *action = Some(Action::OpenProfile);
        }
    });
}

/// A page that exists in the rail but has not been ported yet.
///
/// It says so rather than showing an empty card: a page that is not finished is a
/// fact, and a blank one reads as a bug.
pub fn placeholder(ui: &mut Ui, page: Page) -> Option<Action> {
    widgets::section(ui, page.label(), None, Some(page.subtitle()));
    widgets::empty(
        ui,
        "Esta página ainda está a ser portada",
        "O Resumo já funciona com os teus dados. As restantes páginas chegam nas próximas \
         etapas da migração, uma de cada vez.",
    );
    ui.add_space(12.0);
    widgets::footnote(
        ui,
        "Nada foi perdido: o cofre é o mesmo ficheiro que a versão anterior usava, e o \
         Resumo já o lê.",
    );
    None
}
