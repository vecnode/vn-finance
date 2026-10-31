//! The window draws.
//!
//! A headless test cannot judge a layout, but it can prove the one thing that no
//! amount of type-checking proves: the pages lay out and paint without panicking, in
//! both palettes, from a real vault. `Context::run_ui` runs one egui pass and returns
//! the shapes it produced, so this needs no window and runs on every platform in CI.
//!
//! What it catches: a widget that indexes past its slice, a column width that goes
//! negative, a `Frame` nested inside its own panel, a page that reads a field the
//! model left empty, and any eventual panic in text layout.

use egui::{Pos2, RawInput, Rect, vec2};

use vnf_store::location::Env;
use vnf_store::model::DashboardModel;
use vnf_ui::app::{Failure, Page};
use vnf_ui::pages::profile::Form;
use vnf_ui::{App, Session};

type Problem = Box<dyn std::error::Error>;

const TODAY: &str = "2026-09-20";

/// One egui pass over the whole window.
///
/// The texture delta has to be consumed: epaint keeps a guard that panics when a
/// delta is dropped unapplied, because in a real renderer that would mean a glyph
/// never reached the GPU. A test has no GPU, so it clears them.
fn draw(app: &mut App) -> usize {
    let ctx = egui::Context::default();
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 900.0))),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.draw(ui));
    let shapes = output.shapes.len();
    output.textures_delta.clear();
    shapes
}

/// A session over a temporary folder.
fn session(dir: &std::path::Path) -> Result<Session, Problem> {
    let env = Env::for_test(&[("HOME", &dir.display().to_string())], false);
    Ok(Session::open_in(dir, &env, "0.2.0")?)
}

/// A vault with a profile and two invoices, so the pages have something to draw.
fn seeded(dir: &std::path::Path) -> Result<(), Problem> {
    let session = session(dir)?;
    let profile = vnf_core::profile::create_default_profile(&vnf_core::profile::ProfileInput {
        nif: "123456789".to_owned(),
        name: "Pessoa Exemplo".to_owned(),
        start_date: Some("2019-04-01".to_owned()),
        iva_regime: Some(vnf_core::types::IvaRegime::Trimestral),
        turnover_previous_year_cents: Some(500_000),
        ..Default::default()
    })?;
    session.vault().save_profile(&profile)?;
    let now = vnf_store::clock::now_iso8601();
    for (number, date, base) in [
        ("FR 2026/1", "2026-02-10", 120_000_i64),
        ("FR 2026/2", "2026-05-04", 350_000),
    ] {
        let invoice = vnf_core::estimate::Invoice {
            id: format!("inv_{number}"),
            number: number.to_owned(),
            date: date.to_owned(),
            client_name: "ACME, Lda.".to_owned(),
            client_nif: Some("501234560".to_owned()),
            client_country: "PT".to_owned(),
            description: "Consultoria".to_owned(),
            base_cents: base,
            iva_rate_bp: 2300,
            vat_treatment: vnf_core::estimate::VatTreatment::IvaPt,
            retention_bp: 2300,
            atcud: None,
            status: vnf_core::estimate::InvoiceStatus::Issued,
            payment_proof_in_vault: false,
        };
        session.vault().append_invoice(&invoice, &now)?;
    }
    Ok(())
}

/// A window over a temporary folder.
fn app_for(dir: &std::path::Path, dark: bool) -> Result<App, Problem> {
    Ok(App::build(session(dir)?, TODAY, 2026, dark))
}

#[test]
fn an_empty_vault_draws_the_onboarding() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    let mut app = app_for(dir.path(), false)?;
    let shapes = draw(&mut app);
    assert!(
        shapes > 100,
        "the window drew {shapes} shapes; the rail alone is dozens"
    );
    Ok(())
}

#[test]
fn a_filled_vault_draws_the_dashboard() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path(), false)?;
    let shapes = draw(&mut app);
    // A floor, not a target: what matters is that a whole page produced a page's worth
    // of shapes, so a layout that collapses to nothing fails while a restyle does not.
    assert!(
        shapes > 100,
        "the dashboard with figures and alerts drew {shapes} shapes"
    );
    Ok(())
}

#[test]
fn both_palettes_draw() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let light = draw(&mut app_for(dir.path(), false)?);
    let dark = draw(&mut app_for(dir.path(), true)?);
    assert!(light > 0 && dark > 0);
    // The content should be very nearly the same: only the colours changed.
    let ratio = light as f32 / dark as f32;
    assert!(
        (0.8..1.25).contains(&ratio),
        "light {light} vs dark {dark} shapes"
    );
    Ok(())
}

#[test]
fn every_page_draws_without_panicking() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path(), false)?;
    for page in Page::ALL {
        app.go(page);
        let shapes = draw(&mut app);
        assert!(shapes > 50, "{} drew only {shapes} shapes", page.label());
    }
    Ok(())
}

#[test]
fn every_page_draws_without_a_profile_too() -> Result<(), Problem> {
    // The pages that need a profile say so; the rest must keep working without one.
    let dir = tempfile::tempdir()?;
    let mut app = app_for(dir.path(), false)?;
    for page in Page::ALL {
        app.go(page);
        let shapes = draw(&mut app);
        assert!(
            shapes > 50,
            "{} must draw without a profile ({shapes})",
            page.label()
        );
    }
    Ok(())
}

#[test]
fn the_window_redraws_after_a_reload() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path(), false)?;
    let first = draw(&mut app);
    app.reload();
    let second = draw(&mut app);
    assert!(first > 0 && second > 0);
    // A second pass over the same model must not drift.
    assert_eq!(first, second, "the same model must draw the same way twice");
    Ok(())
}

#[test]
fn the_refusal_window_draws() -> Result<(), Problem> {
    // The window shown when the vault cannot be opened at all, which is the one path
    // a person reaches only when something is already wrong.
    let ctx = egui::Context::default();
    let mut failure = Failure::for_test(
        "o cofre não pode viver dentro do repositório da aplicação.".to_owned(),
        TODAY,
    );
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| failure.draw(ui));
    let shapes = output.shapes.len();
    output.textures_delta.clear();
    assert!(shapes > 5, "the refusal window drew {shapes} shapes");
    Ok(())
}

#[test]
fn the_profile_form_draws_over_the_page() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    let mut app = app_for(dir.path(), false)?;
    app.open_profile();
    let shapes = draw(&mut app);
    assert!(shapes > 100, "the form over an empty vault drew {shapes} shapes");
    // And drawing it twice must not change it: the form is state, not a recomputation.
    let again = draw(&mut app);
    assert_eq!(shapes, again);
    Ok(())
}

#[test]
fn the_profile_form_is_filled_from_the_vault() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let session = session(dir.path())?;
    let profile = session
        .vault()
        .load_profile()?
        .ok_or("the fixture has a profile")?;
    let form = Form::from_profile(Some(&profile));
    assert_eq!(form.nif, "123456789");
    assert_eq!(form.name, "Pessoa Exemplo");
    assert_eq!(form.start_date, "2019-04-01");
    assert_eq!(form.iva, vnf_core::types::IvaRegime::Trimestral);
    assert_eq!(
        form.coefficient_bp(),
        Some(7500),
        "the coefficient round-trips as percent"
    );
    Ok(())
}

#[test]
fn a_completed_obligation_is_written_and_shown() -> Result<(), Problem> {
    use vnf_ui::pages::Action;

    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path(), false)?;
    app.go(Page::Agenda);
    let done = |app: &App| {
        app.model()
            .map(|model| {
                model
                    .agenda
                    .iter()
                    .filter(|instance| instance.status == vnf_core::types::InstanceStatus::Done)
                    .count()
            })
            .unwrap_or_default()
    };
    assert_eq!(done(&app), 0, "a fresh vault has nothing marked handled");
    let target = app
        .model()
        .and_then(|model| model.agenda.first().cloned())
        .ok_or("the seeded profile must produce an agenda")?;
    // The window performs the write the page asked for, through the session.
    app.perform(Action::Complete {
        id: target.id.clone(),
        rule_id: target.rule_id.clone(),
        due_date: target.due_date.clone(),
    });
    assert_eq!(
        done(&app),
        1,
        "marking one handled must show up in the rebuilt model"
    );
    // And the page it is on still draws.
    assert!(draw(&mut app) > 50);
    Ok(())
}

/// The bytes of one of the golden PDF fixtures, which the reference itself parsed.
///
/// Reusing them means the window's flow is tested against a document that is already
/// known to read correctly, rather than against a second invented one.
fn golden_pdf(case: usize) -> Result<Vec<u8>, Problem> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("core")
        .join("tests")
        .join("golden")
        .join("pdf.json");
    let fixture: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let hex = fixture["cases"][case]["bytesHex"]
        .as_str()
        .ok_or("the fixture has hex bytes")?;
    Ok(hex
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            let high = char::from(*pair.first()?).to_digit(16)?;
            let low = char::from(*pair.get(1)?).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()
        })
        .collect())
}

#[test]
fn a_pdf_imported_through_the_window_lands_in_the_ledger() -> Result<(), Problem> {
    use vnf_ui::pages::Action;

    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let document = dir.path().join("fatura.pdf");
    std::fs::write(&document, golden_pdf(0)?)?;

    let mut app = app_for(dir.path(), false)?;
    assert_eq!(
        app.model().map(|model| model.invoices.len()),
        Some(2),
        "two seeded invoices"
    );

    app.go(Page::Recibos);
    app.perform(Action::ParseReceipt {
        path: document.display().to_string(),
    });

    // What the document said reached the form, including the figures.
    let form = app.receipt_form();
    assert!(form.open);
    assert_eq!(form.date, "2026-03-15");
    assert_eq!(form.client_name, "Empresa Exemplo, Lda.");
    assert_eq!(form.iva_rate, "23");
    assert!(!form.base.is_empty(), "the base must have been read");
    assert!(
        form.document_file.is_some(),
        "the PDF is archived before anything is recorded"
    );

    // And confirming it writes one invoice.
    let confirmation = form.confirmation();
    app.perform(Action::SaveReceipt {
        confirmation: Box::new(confirmation),
    });
    assert_eq!(
        app.model().map(|model| model.invoices.len()),
        Some(3),
        "the confirmed document must be one more ledger line"
    );

    let added = app
        .model()
        .and_then(|model| model.invoices.last().cloned())
        .ok_or("the ledger has a last line")?;
    assert_eq!(added.base_cents, 100_000);
    assert_eq!(added.iva_rate_bp, 2300);
    assert_eq!(added.retention_bp, 0, "the document says Sem retenção");
    assert_eq!(added.client_country, "PT");
    assert!(added.payment_proof_in_vault, "the archived PDF is the proof");

    // The page redraws with the new line.
    assert!(draw(&mut app) > 50);
    Ok(())
}

#[test]
fn a_document_issued_by_somebody_else_is_refused_by_the_window() -> Result<(), Problem> {
    use vnf_ui::pages::Action;

    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let document = dir.path().join("alheia.pdf");
    std::fs::write(&document, golden_pdf(4)?)?;

    let mut app = app_for(dir.path(), false)?;
    app.perform(Action::ParseReceipt {
        path: document.display().to_string(),
    });
    let confirmation = app.receipt_form().confirmation();
    app.perform(Action::SaveReceipt {
        confirmation: Box::new(confirmation),
    });

    assert_eq!(
        app.model().map(|model| model.invoices.len()),
        Some(2),
        "nothing may be added: the issuer is not the profile"
    );
    assert!(
        app.receipt_form()
            .problems
            .iter()
            .any(|problem| problem.contains("RECEBESTE") || problem.contains("não pode entrar")),
        "the refusal must say why: {:?}",
        app.receipt_form().problems
    );
    Ok(())
}

/// The model a page draws from, so a test can check what it would draw.
#[allow(dead_code)]
fn model_of(session: &Session) -> Result<DashboardModel, Problem> {
    Ok(session.reload(TODAY, 2026)?)
}
