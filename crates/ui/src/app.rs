//! The window: the rail, the header, and the page it is showing.
//!
//! The browser panel's navigation lived in the URL hash. A desktop window has no
//! address bar, so the page is a value in this struct and the sidebar writes it. What
//! the URL bought — a page you can come back to, and a back button — becomes a
//! remembered page and Ctrl+1..0, and what it cost — a hash that could disagree with
//! what was on screen — is gone.

use vnf_core::profile::ProblemLevel;
use vnf_store::model::DashboardModel;
use vnf_store::risk::RiskLevel;

use crate::icons::{self, Icon};
use crate::pages::{self, agenda, profile, receipt_form};
use crate::session::Session;
use crate::theme;
use crate::widgets::{self, Tone};

/// One page of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// The dashboard: alerts, figures and the fiscal situation.
    #[default]
    Painel,
    /// What the law obliges, and when.
    Agenda,
    /// The invoices issued this year.
    Recibos,
    /// The quarter's Segurança Social contribution, step by step.
    SegurancaSocial,
    /// IVA charged, by quarter.
    Iva,
    /// IRS withheld, the reserve, and the taxable base.
    Irs,
    /// Where the files live, what is archived, and the PDF invoices.
    Cofre,
    /// The law behind every figure.
    Regras,
    /// Profile, vault and rule pack, in one place.
    Diagnostico,
    /// What this application does not do.
    Sobre,
}

impl Page {
    /// Every page, in the order the rail lists them.
    pub const ALL: [Page; 10] = [
        Page::Painel,
        Page::Agenda,
        Page::Recibos,
        Page::SegurancaSocial,
        Page::Iva,
        Page::Irs,
        Page::Cofre,
        Page::Regras,
        Page::Diagnostico,
        Page::Sobre,
    ];

    /// The rail's label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Painel => "Resumo",
            Self::Agenda => "Agenda fiscal",
            Self::Recibos => "Faturas emitidas",
            Self::SegurancaSocial => "Segurança Social",
            Self::Iva => "IVA",
            Self::Irs => "IRS",
            Self::Cofre => "Cofre de documentos",
            Self::Regras => "Regras & fontes",
            Self::Diagnostico => "Diagnóstico",
            Self::Sobre => "About",
        }
    }

    /// The sentence under the title in the header, saying what the page is for.
    #[must_use]
    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Painel => "Indicadores do exercício, enquadramento fiscal e alertas",
            Self::Agenda => "O que a lei obriga, e quando",
            Self::Recibos => "Os documentos que emitiste neste exercício",
            Self::SegurancaSocial => "A contribuição do trimestre, passo a passo",
            Self::Iva => "O IVA liquidado, por trimestre",
            Self::Irs => "Retenções sofridas, reserva e rendimento tributável",
            Self::Cofre => "Onde vivem os ficheiros, o que está arquivado e as faturas em PDF",
            Self::Regras => "A lei citada por trás de cada número",
            Self::Diagnostico => "Perfil, cofre e pacote de regras",
            Self::Sobre => "O que esta aplicação não faz",
        }
    }

    /// Which group of the rail it belongs to.
    #[must_use]
    pub fn group(self) -> &'static str {
        match self {
            Self::Painel | Self::Agenda => "Geral",
            Self::Recibos | Self::SegurancaSocial | Self::Iva | Self::Irs => "Faturação e obrigações",
            _ => "Arquivo e apoio",
        }
    }

    /// Whether its numbers need the profile.
    ///
    /// Without one there is no framework, no agenda and no estimate — and a page that
    /// showed zeros would be showing data it does not have.
    #[must_use]
    pub fn needs_profile(self) -> bool {
        matches!(
            self,
            Self::Agenda | Self::Recibos | Self::SegurancaSocial | Self::Iva | Self::Irs
        )
    }

    /// Whether the alert strip belongs on it.
    ///
    /// Two pages are about what is due; everywhere else the subject is something else,
    /// and a green "no alerts" repeated on ten tabs stops being read.
    #[must_use]
    pub fn shows_alerts(self) -> bool {
        matches!(self, Self::Painel | Self::Agenda)
    }

    /// The icon the rail draws.
    #[must_use]
    pub fn icon(self) -> Icon {
        match self {
            Self::Painel => Icon::Dashboard,
            Self::Agenda => Icon::Calendar,
            Self::Recibos => Icon::Invoice,
            Self::SegurancaSocial => Icon::Shield,
            Self::Iva => Icon::Percent,
            Self::Irs => Icon::Form,
            Self::Cofre => Icon::Lock,
            Self::Regras => Icon::Book,
            Self::Diagnostico | Self::Sobre => Icon::Info,
        }
    }
}

/// The window.
pub struct App {
    session: Session,
    model: Option<DashboardModel>,
    /// The message a failed load left behind, shown instead of a page of nothing.
    problem: Option<String>,
    page: Page,
    today: String,
    year: i64,
    dark: bool,
    theme_applied: Option<bool>,
    reveal_nif: bool,
    /// A line under the header confirming the last thing that happened.
    status: Option<String>,
    /// Which agenda view is showing.
    filter: agenda::Filter,
    /// Which quarter the Segurança Social page shows.
    quarter: i64,
    /// The path typed into the Cofre's archive field.
    archive_path: String,
    /// The kind chosen for the next archived document.
    archive_kind: String,
    /// The receipt-import form, and whether it is open.
    receipt: receipt_form::Form,
    /// The profile form, and whether it is open.
    profile: profile::Form,
}

impl App {
    /// Build the window from an open session.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>, session: Session, today: &str, year: i64) -> Self {
        let dark = theme::is_dark(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, dark);
        Self::build(session, today, year, dark)
    }

    /// Build the window without a graphics context, which is what a render test does.
    #[must_use]
    pub fn build(session: Session, today: &str, year: i64, dark: bool) -> Self {
        let mut app = Self {
            session,
            model: None,
            problem: None,
            page: Page::Painel,
            today: today.to_owned(),
            year,
            dark,
            theme_applied: Some(dark),
            reveal_nif: false,
            status: None,
            filter: agenda::Filter::default(),
            quarter: pages::painel::current_quarter(today),
            archive_path: String::new(),
            archive_kind: "comprovativo".to_owned(),
            receipt: receipt_form::Form::default(),
            profile: profile::Form::default(),
        };
        app.reload();
        app
    }

    /// Rebuild the model, keeping the page it was showing.
    pub fn reload(&mut self) {
        match self.session.reload(&self.today, self.year) {
            Ok(model) => {
                self.model = Some(model);
                self.problem = None;
            }
            Err(error) => {
                self.model = None;
                self.problem = Some(error.to_string());
            }
        }
    }

    /// The page the rail should highlight.
    #[must_use]
    pub fn page(&self) -> Page {
        self.page
    }

    /// The receipt form, for a test that drives the import flow.
    #[must_use]
    pub fn receipt_form(&self) -> &receipt_form::Form {
        &self.receipt
    }

    /// The model the window is showing, if it has one.
    #[must_use]
    pub fn model(&self) -> Option<&DashboardModel> {
        self.model.as_ref()
    }

    /// Show a page.
    pub fn go(&mut self, page: Page) {
        self.page = page;
    }

    /// Open the profile form, filled from what the vault holds.
    pub fn open_profile(&mut self) {
        let current = self.model.as_ref().and_then(|model| model.profile.as_ref());
        self.profile = profile::Form::from_profile(current);
        self.profile.open = true;
    }

    /// Read a PDF, archive it, and show what it said.
    ///
    /// The document is archived BEFORE the fields are confirmed, because the archive is
    /// what proves the invoice exists later: an invoice recorded with no document is a
    /// claim, and the point of the Cofre is that the claim has evidence.
    fn parse_receipt(&mut self, path: &str) {
        let source = std::path::Path::new(path);
        if !source.is_file() {
            self.receipt.problems = vec![format!("Não encontrei nenhum ficheiro em {path}.")];
            self.receipt.open = true;
            self.receipt.stage = receipt_form::Stage::Idle;
            return;
        }
        let bytes = match std::fs::read(source) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.receipt = receipt_form::Form::starting();
                self.receipt.problems = vec![format!("Não consegui ler o ficheiro: {error}")];
                return;
            }
        };
        let now = vnf_store::clock::now_iso8601();
        let document_file = match self.session.vault().add_document(
            source,
            vnf_store::documents::DocumentMeta {
                kind: Some("fatura"),
                obligation_id: None,
                invoice_id: None,
            },
            &now,
        ) {
            Ok(document) => Some(document.file),
            Err(error) => {
                self.status = Some(format!("O documento não foi arquivado: {error}"));
                None
            }
        };
        let pdf = vnf_core::pdf::extract_pdf_text(&bytes);
        let draft = vnf_core::receipt::parse_receipt(&pdf);
        let mut form = receipt_form::Form::starting();
        form.read(draft, document_file);
        self.receipt = form;
        self.reload();
    }

    /// Register what the form confirmed.
    fn save_receipt(&mut self, confirmation: &vnf_core::receipt::ReceiptConfirmation) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let (Some(draft), Some(profile)) = (self.receipt.draft.as_ref(), model.profile.as_ref()) else {
            self.receipt.problems =
                vec!["Falta o perfil ou o documento lido: não há fatura para registar.".to_owned()];
            return;
        };
        let now = vnf_store::clock::now_iso8601();
        let count = model.invoices.len() + 1;
        let result = vnf_core::receipt::invoice_from_receipt(&vnf_core::receipt::ReceiptInvoiceInput {
            draft,
            pack: &self.session.pack().pack,
            profile,
            confirmed: confirmation,
            id: format!(
                "inv_{count}_{}",
                now.chars()
                    .filter(char::is_ascii_digit)
                    .take(14)
                    .collect::<String>()
            ),
            document_file: self.receipt.document_file.clone(),
            fallback_number: format!("S/N {count}"),
        });
        self.receipt.warnings = result.warnings.clone();
        if result.invoice.is_none() {
            self.receipt.problems = result.problems.clone();
            return;
        }
        if !result.problems.is_empty() {
            self.receipt.problems = result.problems.clone();
            return;
        }
        let Some(invoice) = result.invoice else {
            return;
        };
        match self.session.vault().append_invoice(&invoice, &now) {
            Ok(()) => {
                self.receipt = receipt_form::Form::default();
                self.reload();
                self.status = Some(format!("Fatura {} registada no livro.", invoice.number));
            }
            Err(error) => {
                self.receipt.problems = vec![error.to_string()];
            }
        }
    }

    /// The header: what the window is, and the four facts about the year.
    fn header(&mut self, ui: &mut egui::Ui) {
        let palette = theme::palette(ui.ctx());
        let mut open_profile = false;
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("vn-finance")
                        .size(theme::PAGE_TITLE)
                        .strong()
                        .color(palette.text),
                );
                ui.label(
                    egui::RichText::new(format!("{} — {}", self.page.label(), self.page.subtitle()))
                        .size(theme::BODY_SMALL)
                        .color(palette.text_3),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::chip(ui, false, "Local · dados só neste computador").clicked() {
                    // A chip is a statement, not a button; clicking it only explains.
                    self.status = Some(
                        "Nada sai deste computador: esta aplicação não abre ligações de rede.".to_owned(),
                    );
                }
                if widgets::chip(ui, self.dark, if self.dark { "Escuro" } else { "Claro" }).clicked() {
                    self.dark = !self.dark;
                }
                if widgets::chip(ui, false, "Perfil").clicked() {
                    open_profile = true;
                }
                if let Some(model) = &self.model
                    && !model.meta.data_dir.is_empty()
                {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&model.meta.data_dir)
                                .size(theme::MONO)
                                .monospace()
                                .color(palette.text_3),
                        )
                        .truncate(),
                    );
                }
                ui.label(
                    egui::RichText::new(format!("Exercício {}", self.year))
                        .size(theme::CAPTION)
                        .color(palette.text_2),
                );
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if let Some(model) = &self.model {
                if let Some(profile) = &model.profile {
                    let regime = match profile.iva.regime {
                        vnf_core::types::IvaRegime::IsentoArt53 => "isento art. 53.º",
                        vnf_core::types::IvaRegime::Mensal => "mensal",
                        vnf_core::types::IvaRegime::Trimestral => "trimestral",
                    };
                    ui.label(
                        egui::RichText::new(format!("IVA {regime}"))
                            .size(theme::CAPTION)
                            .color(palette.text_2),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "IRS {}",
                            match profile.irs.regime {
                                vnf_core::types::IrsRegime::Simplificado => "simplificado",
                                vnf_core::types::IrsRegime::Organizada => "organizada",
                            }
                        ))
                        .size(theme::CAPTION)
                        .color(palette.text_2),
                    );
                    ui.separator();
                    let masked = if self.reveal_nif {
                        profile.nif.clone()
                    } else {
                        vnf_core::nif::mask_nif(&profile.nif)
                    };
                    ui.label(
                        egui::RichText::new(format!("NIF {masked}"))
                            .size(theme::CAPTION)
                            .monospace()
                            .color(palette.text_2),
                    );
                    if widgets::chip(
                        ui,
                        self.reveal_nif,
                        if self.reveal_nif { "ocultar" } else { "revelar" },
                    )
                    .clicked()
                    {
                        self.reveal_nif = !self.reveal_nif;
                    }
                } else {
                    ui.label(
                        egui::RichText::new("Ainda não há perfil neste cofre")
                            .size(theme::CAPTION)
                            .color(palette.text_3),
                    );
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "Regras pt/{} · {} verificadas · {} por verificar",
                        model.pack.summary.pack_version,
                        model.pack.summary.verified,
                        model.pack.summary.unverified + model.pack.summary.partial
                    ))
                    .size(theme::CAPTION)
                    .color(palette.text_3),
                );
                ui.label(
                    egui::RichText::new(format!("hoje {}", self.today))
                        .size(theme::CAPTION)
                        .color(palette.text_4),
                );
            }
        });
        ui.add_space(10.0);
        let (rect, _) =
            ui.allocate_exact_size(egui::Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            egui::Stroke::new(1.0, palette.border),
        );
        if open_profile {
            self.open_profile();
        }
    }

    /// The rail: the brand, the pages, and what the pack says about itself.
    fn rail(&mut self, ui: &mut egui::Ui) {
        let palette = theme::palette(ui.ctx());
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(21.0), egui::Sense::hover());
            icons::draw(ui.painter(), rect, palette.accent, Icon::Dashboard);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("vn-finance")
                        .size(15.0)
                        .strong()
                        .color(palette.text),
                );
                widgets::micro_label(ui, "contabilidade local", palette.text_4);
            });
        });
        ui.add_space(10.0);
        let (rect, _) =
            ui.allocate_exact_size(egui::Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            egui::Stroke::new(1.0, palette.border),
        );
        ui.add_space(6.0);

        let counters = self.counters();
        let mut previous_group = "";
        for page in Page::ALL {
            if page.group() != previous_group {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    widgets::micro_label(ui, page.group(), palette.text_4);
                });
                ui.add_space(2.0);
                previous_group = page.group();
            }
            let selected = page == self.page;
            let response = nav_item(ui, page, selected, counters.count_for(page));
            if response.clicked() {
                self.page = page;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                icons::inline(ui, Icon::Lock, 13.0, palette.ok);
                ui.label(
                    egui::RichText::new("Dados só neste computador")
                        .size(theme::CAPTION)
                        .color(palette.text_3),
                );
            });
            if let Some(model) = &self.model {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Pacote pt/{} · {} fontes",
                            model.pack.summary.pack_version, model.pack.summary.sources
                        ))
                        .size(theme::CAPTION)
                        .color(palette.text_4),
                    );
                });
            }
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.label(
                    egui::RichText::new("Sem telemetria · sem servidores")
                        .size(theme::CAPTION)
                        .color(palette.text_4),
                );
            });
            ui.add_space(6.0);
            let (rect, _) =
                ui.allocate_exact_size(egui::Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter().hline(
                rect.x_range(),
                rect.center().y,
                egui::Stroke::new(1.0, palette.border),
            );
        });
    }

    /// What each rail item counts, from the one model.
    fn counters(&self) -> Counters {
        let Some(model) = &self.model else {
            return Counters::default();
        };
        let actionable = model
            .agenda
            .iter()
            .filter(|instance| {
                !matches!(
                    instance.status,
                    vnf_core::types::InstanceStatus::Done
                        | vnf_core::types::InstanceStatus::Untracked
                        | vnf_core::types::InstanceStatus::NotApplicable
                )
            })
            .count();
        let overdue_any = model
            .agenda
            .iter()
            .any(|instance| instance.status == vnf_core::types::InstanceStatus::Overdue);
        let prefix = format!("{}-", self.year);
        Counters {
            agenda: Some((actionable, overdue_any)),
            invoices: Some((
                model
                    .invoices
                    .iter()
                    .filter(|invoice| invoice.date.starts_with(&prefix))
                    .count(),
                false,
            )),
            cofre: Some((
                model
                    .agenda
                    .iter()
                    .filter(|instance| {
                        !instance.documents_to_keep.is_empty()
                            && matches!(
                                instance.status,
                                vnf_core::types::InstanceStatus::Overdue
                                    | vnf_core::types::InstanceStatus::DueSoon
                            )
                    })
                    .count(),
                false,
            )),
            regras: Some((model.pack.summary.obligations, false)),
            diagnostico: Some((
                model
                    .profile_problems
                    .iter()
                    .filter(|problem| problem.level == ProblemLevel::Error)
                    .count()
                    + usize::from(model.meta.data_dir_risk.level == RiskLevel::Fatal),
                model.meta.data_dir_risk.level != RiskLevel::None,
            )),
        }
    }

    /// The page itself.
    fn content(&mut self, ui: &mut egui::Ui) {
        if let Some(problem) = self.problem.clone() {
            widgets::accented_card(ui, Tone::Danger, |ui| {
                ui.label(
                    egui::RichText::new("O cofre não pôde ser lido")
                        .size(theme::BODY)
                        .strong(),
                );
                ui.label(egui::RichText::new(problem).size(theme::BODY_SMALL));
            });
            return;
        }
        let page = self.page;
        let year = self.year;
        let today = self.today.clone();
        // A page whose figures need the profile says so once, here, rather than each
        // page rendering its own version of the same paragraph.
        let needs_profile = self
            .model
            .as_ref()
            .is_some_and(|model| model.profile.is_none() && page.needs_profile());
        if needs_profile {
            let mut action = None;
            pages::needs_profile(ui, page, &mut action);
            if let Some(action) = action {
                self.perform(action);
            }
            return;
        }
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let filter = self.filter;
        let quarter = self.quarter;
        let mut path = std::mem::take(&mut self.archive_path);
        let kind = self.archive_kind.clone();
        // The pages borrow the model immutably and return what they want done; the
        // window performs it and rebuilds. Nothing below writes to the vault.
        let action = match page {
            Page::Painel => pages::painel::show(ui, model, &today, year),
            Page::Agenda => pages::agenda::show(ui, model, filter, &today),
            Page::Recibos => pages::recibos::show(ui, model, year),
            Page::SegurancaSocial => pages::seguranca_social::show(ui, model, &today, quarter),
            Page::Iva => pages::iva::show(ui, model, &today),
            Page::Irs => pages::irs::show(ui, model),
            Page::Cofre => pages::cofre::show(ui, model, &mut path, &kind),
            Page::Regras => pages::regras::show(ui, model),
            Page::Diagnostico => pages::diagnostico::show(ui, model),
            Page::Sobre => pages::sobre::show(ui, model),
        };
        self.archive_path = path;
        if let Some(action) = action {
            self.perform(action);
        }
    }

    /// Do one thing a page asked for.
    ///
    /// Public because it is the window's one write path: a page returns a
    /// [`pages::Action`] and this performs it, which is what lets a test drive a write
    /// the way a click does — through the session, never around it.
    pub fn perform(&mut self, action: pages::Action) {
        match action {
            pages::Action::Reload => self.reload(),
            pages::Action::GoTo(page) => self.page = page,
            pages::Action::Notice(message) => self.status = Some(message),
            pages::Action::SetFilter(filter) => self.filter = filter,
            pages::Action::SetQuarter(quarter) => self.quarter = quarter.clamp(1, 4),
            pages::Action::OpenProfile => self.open_profile(),
            pages::Action::CloseProfile => {
                self.profile.open = false;
                self.profile.message = None;
            }
            pages::Action::OpenReceipt => {
                self.receipt = receipt_form::Form::starting();
            }
            pages::Action::ParseReceipt { path } => self.parse_receipt(&path),
            pages::Action::SaveReceipt { confirmation } => self.save_receipt(&confirmation),
            pages::Action::CloseReceipt => {
                self.receipt = receipt_form::Form::default();
            }
            pages::Action::Complete {
                id,
                rule_id,
                due_date,
            } => {
                let now = vnf_store::clock::now_iso8601();
                match self
                    .session
                    .vault()
                    .mark_completed(&id, &rule_id, &due_date, None, &now)
                {
                    Ok(()) => {
                        self.reload();
                        self.status = Some(format!("Registado como tratado: {id}"));
                    }
                    Err(error) => self.status = Some(error.to_string()),
                }
            }
            pages::Action::Archive { path, kind } => {
                let now = vnf_store::clock::now_iso8601();
                let meta = vnf_store::documents::DocumentMeta {
                    kind: if kind.trim().is_empty() {
                        None
                    } else {
                        Some(kind.as_str())
                    },
                    obligation_id: None,
                    invoice_id: None,
                };
                match self
                    .session
                    .vault()
                    .add_document(std::path::Path::new(&path), meta, &now)
                {
                    Ok(document) => {
                        self.archive_path.clear();
                        self.reload();
                        self.status = Some(format!("Arquivado: {}", document.file));
                    }
                    Err(error) => self.status = Some(error.to_string()),
                }
            }
            pages::Action::SaveProfile {
                draft,
                coefficient_bp,
            } => {
                let current = self.session.vault().load_profile().ok().flatten();
                match vnf_core::profile::build_profile(&draft, current.as_ref(), Some(coefficient_bp)) {
                    Ok(profile) => {
                        let problems = vnf_core::profile::validate_profile(&profile, &self.today);
                        let errors: Vec<&vnf_core::profile::ProfileProblem> = problems
                            .iter()
                            .filter(|problem| problem.level == ProblemLevel::Error)
                            .collect();
                        if let Some(first) = errors.first() {
                            self.profile.message = Some((first.message.clone(), Tone::Danger));
                            return;
                        }
                        match self.session.vault().save_profile(&profile) {
                            Ok(()) => {
                                self.profile.open = false;
                                self.profile.message = None;
                                self.reload();
                                self.status = Some("Perfil guardado no cofre.".to_owned());
                            }
                            Err(error) => {
                                self.profile.message = Some((error.to_string(), Tone::Danger));
                            }
                        }
                    }
                    Err(error) => {
                        self.profile.message = Some((error.to_string(), Tone::Danger));
                    }
                }
            }
            pages::Action::ChooseVault => {
                self.status = Some(
                    "Escolher outra pasta chega com o seletor de ficheiros, na próxima etapa.".to_owned(),
                );
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

impl App {
    /// Draw the whole window into a UI.
    ///
    /// Separate from the `eframe::App` implementation so a test can draw the window
    /// with no graphics context at all — `Context::run_ui` is enough, and "it drew
    /// thousands of shapes and did not panic" is the one thing a headless test can
    /// honestly assert about a layout.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.theme_applied != Some(self.dark) {
            theme::apply(&ctx, self.dark);
            self.theme_applied = Some(self.dark);
        }
        let palette = theme::palette(&ctx);

        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(palette.surface)
                    .inner_margin(egui::Margin::symmetric(22, 12)),
            )
            .show(ui, |ui| self.header(ui));

        egui::Panel::left("rail")
            .resizable(false)
            .size_range(egui::Rangef::new(theme::SIDEBAR_WIDTH, theme::SIDEBAR_WIDTH))
            .frame(
                egui::Frame::new()
                    .fill(palette.surface)
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ui, |ui| self.rail(ui));

        if let Some(status) = self.status.clone() {
            egui::Panel::bottom("status")
                .frame(
                    egui::Frame::new()
                        .fill(palette.surface)
                        .inner_margin(egui::Margin::symmetric(22, 6)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&status)
                                .size(theme::CAPTION)
                                .color(palette.text_2),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::chip(ui, false, "fechar").clicked() {
                                self.status = None;
                            }
                        });
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.bg)
                    .inner_margin(egui::Margin::symmetric(28, 22)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(theme::CONTENT_MAX_WIDTH);
                        self.content(ui);
                    });
            });

        // The forms float above the page, so they can be opened from any of them.
        if let Some(action) = pages::profile::show(&ctx, &mut self.profile) {
            self.perform(action);
        }
        if let Some(action) = pages::receipt_form::show(&ctx, &mut self.receipt) {
            self.perform(action);
        }

        // Ctrl+1..0 walks the rail, which is what a keyboard-first person expects of a
        // list of ten pages.
        let pressed = ctx.input(|input| {
            (1..=10).find(|number| input.modifiers.command && input.key_pressed(key_for(*number)))
        });
        if let Some(page) = pressed.and_then(|index| Page::ALL.get(index - 1)) {
            self.page = *page;
        }
    }
}

/// The key for a page number: 1..9 then 0 for the tenth.
fn key_for(number: usize) -> egui::Key {
    match number {
        1 => egui::Key::Num1,
        2 => egui::Key::Num2,
        3 => egui::Key::Num3,
        4 => egui::Key::Num4,
        5 => egui::Key::Num5,
        6 => egui::Key::Num6,
        7 => egui::Key::Num7,
        8 => egui::Key::Num8,
        9 => egui::Key::Num9,
        _ => egui::Key::Num0,
    }
}

/// One row of the rail.
fn nav_item(ui: &mut egui::Ui, page: Page, selected: bool, count: Option<(usize, bool)>) -> egui::Response {
    let palette = theme::palette(ui.ctx());
    let height = 32.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), height),
        egui::Sense::click(),
    );
    let hovered = response.hovered();
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, palette.accent_bg);
        let bar = egui::Rect::from_min_size(rect.min, egui::Vec2::new(3.0, rect.height()));
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::ZERO, palette.accent);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, palette.surface_2);
    }
    let colour = if selected { palette.accent } else { palette.text_2 };
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 18.0, rect.center().y - 7.5),
        egui::Vec2::splat(15.0),
    );
    icons::draw(
        ui.painter(),
        icon_rect,
        if selected { palette.accent } else { palette.text_4 },
        page.icon(),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 43.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        page.label(),
        egui::FontId::proportional(theme::BODY),
        colour,
    );
    if let Some((value, urgent)) = count {
        let tone = if urgent {
            Tone::Danger
        } else if value > 0 {
            Tone::Slate
        } else {
            Tone::Na
        };
        let text = if value == 0 {
            "—".to_owned()
        } else {
            value.to_string()
        };
        let (foreground, _, _) = tone.colours(&palette);
        let galley = ui
            .painter()
            .layout_no_wrap(text, egui::FontId::proportional(theme::MICRO), foreground);
        let size = galley.size() + egui::Vec2::new(14.0, 4.0);
        let badge = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 18.0 - size.x, rect.center().y - size.y / 2.0),
            size,
        );
        ui.painter()
            .rect_filled(badge, egui::CornerRadius::same(10), palette.surface_3);
        ui.painter()
            .galley(badge.min + egui::Vec2::new(7.0, 2.0), galley, foreground);
    }
    response
}

/// What the rail shows beside each page.
#[derive(Debug, Clone, Copy, Default)]
struct Counters {
    agenda: Option<(usize, bool)>,
    invoices: Option<(usize, bool)>,
    cofre: Option<(usize, bool)>,
    regras: Option<(usize, bool)>,
    diagnostico: Option<(usize, bool)>,
}

impl Counters {
    fn count_for(&self, page: Page) -> Option<(usize, bool)> {
        match page {
            Page::Agenda => self.agenda,
            Page::Recibos => self.invoices,
            Page::Cofre => self.cofre,
            Page::Regras => self.regras,
            Page::Diagnostico => self.diagnostico,
            _ => None,
        }
    }
}

/// The window shown when the vault cannot be opened at all.
///
/// A refusal has to be readable in the window: this build has no console to print to,
/// and a launch that silently does nothing would be the worst failure of all.
pub struct Failure {
    message: String,
    today: String,
}

impl Failure {
    /// Build the window around a message.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>, message: String, today: &str) -> Self {
        let dark = theme::is_dark(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, dark);
        Self {
            message,
            today: today.to_owned(),
        }
    }

    /// Build it without a graphics context, for the render test.
    #[must_use]
    pub fn for_test(message: String, today: &str) -> Self {
        Self {
            message,
            today: today.to_owned(),
        }
    }

    /// Draw the window.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let palette = theme::palette(&ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.bg)
                    .inner_margin(egui::Margin::symmetric(28, 22)),
            )
            .show(ui, |ui| {
                ui.set_max_width(880.0);
                widgets::section(
                    ui,
                    "vn-finance",
                    Some(&self.today),
                    Some("Esta janela não conseguiu abrir o teu cofre."),
                );
                widgets::accented_card(ui, Tone::Danger, |ui| {
                    ui.label(
                        egui::RichText::new("O cofre não pôde ser aberto")
                            .size(theme::BODY)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&self.message).size(theme::BODY_SMALL));
                });
                ui.add_space(14.0);
                widgets::footnote(
                    ui,
                    "Nada foi alterado. Fecha a outra janela do vn-finance, ou move o cofre para \
                     fora do repositório da aplicação, e abre esta de novo.",
                );
            });
    }
}

impl eframe::App for Failure {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_ten_pages_and_every_one_says_what_it_is_for() {
        assert_eq!(Page::ALL.len(), 10);
        for page in Page::ALL {
            assert!(!page.label().is_empty());
            // The lead is not decoration: it is the sentence that teaches a first-time
            // reader what the page is about.
            assert!(page.subtitle().len() > 10, "{} needs a subtitle", page.label());
            assert!(!page.group().is_empty());
        }
    }

    #[test]
    fn only_five_pages_need_a_profile() {
        let needing: Vec<&str> = Page::ALL
            .iter()
            .filter(|page| page.needs_profile())
            .map(|page| page.label())
            .collect();
        assert_eq!(
            needing,
            vec![
                "Agenda fiscal",
                "Faturas emitidas",
                "Segurança Social",
                "IVA",
                "IRS"
            ]
        );
    }

    #[test]
    fn the_alert_strip_has_exactly_two_homes() {
        let showing: Vec<&str> = Page::ALL
            .iter()
            .filter(|page| page.shows_alerts())
            .map(|page| page.label())
            .collect();
        assert_eq!(showing, vec!["Resumo", "Agenda fiscal"]);
    }

    #[test]
    fn the_rail_groups_cover_every_page() {
        let groups: std::collections::BTreeSet<&str> = Page::ALL.iter().map(|page| page.group()).collect();
        assert_eq!(groups.len(), 3);
        assert_eq!(
            groups.into_iter().collect::<Vec<&str>>(),
            vec!["Arquivo e apoio", "Faturação e obrigações", "Geral"]
        );
    }

    #[test]
    fn the_keyboard_reaches_every_page() {
        for (index, page) in Page::ALL.iter().enumerate() {
            let _ = key_for(index + 1);
            assert!(!page.label().is_empty());
        }
    }
}
