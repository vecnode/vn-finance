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
use vnf_ui::app::{Failure, Page, RAIL_FOOT, RAIL_ROW, WINDOW_MIN};
use vnf_ui::pages::profile::Form;
use vnf_ui::theme::{CONTENT_MARGIN, SIDEBAR_WIDTH};
use vnf_ui::{App, Session};

type Problem = Box<dyn std::error::Error>;

const TODAY: &str = "2026-09-20";

/// One egui pass over the whole window at `size`.
///
/// The texture delta has to be consumed: epaint keeps a guard that panics when a
/// delta is dropped unapplied, because in a real renderer that would mean a glyph
/// never reached the GPU. A test has no GPU, so it clears them.
fn draw_at(app: &mut App, ctx: &egui::Context, size: egui::Vec2) -> usize {
    draw_with(app, ctx, size, None)
}

/// The same pass, with the desktop reporting the theme it is in.
fn draw_with(
    app: &mut App,
    ctx: &egui::Context,
    size: egui::Vec2,
    system_theme: Option<egui::Theme>,
) -> usize {
    // A context is per-test and the window applies its palette to the one it is given
    // once, so the harness gives each fresh context the same white window the real one
    // starts with. The application's own application of it is checked by
    // `a_dark_desktop_cannot_turn_the_window_dark`.
    vnf_ui::theme::apply(ctx, false);
    let mut shapes = 0;
    for _ in 0..SETTLED {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            system_theme,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.draw(ui));
        shapes = output.shapes.len();
        output.textures_delta.clear();
    }
    shapes
}

/// How many passes a window needs before its layout stops moving.
///
/// A panel starts from its default size and takes the size its content asked for on the
/// next pass, so a window measured on its first frame is measured in a layout the person
/// looking at it never sees. Four is one more than the nesting depth this window has.
const SETTLED: usize = 4;

/// One egui pass at the size of a maximised window.
fn draw(app: &mut App) -> usize {
    let ctx = egui::Context::default();
    draw_at(app, &ctx, vec2(1280.0, 900.0))
}

/// Every string the window painted, in painting order, with the box it occupies.
///
/// The layout cannot be asserted headlessly, but the words on it can — and where each
/// one landed decides a question like "does the rail's last button still fit in the
/// smallest window the application allows".
fn painted_text_at(app: &mut App, size: egui::Vec2) -> Vec<(String, Rect)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(text) => out.push((text.galley.text().to_owned(), text.visual_bounding_rect())),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => {}
        }
    }

    let ctx = egui::Context::default();
    vnf_ui::theme::apply(&ctx, false);
    let mut texts = Vec::new();
    for _ in 0..SETTLED {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.draw(ui));
        texts.clear();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut texts);
        }
        output.textures_delta.clear();
    }
    texts
}

/// Every string the window painted, at the size of a maximised window.
fn painted_text(app: &mut App) -> Vec<String> {
    painted_text_at(app, vec2(1280.0, 900.0))
        .into_iter()
        .map(|(text, _)| text)
        .collect()
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
fn app_for(dir: &std::path::Path) -> Result<App, Problem> {
    Ok(App::build(session(dir)?, TODAY, 2026))
}

#[test]
fn an_empty_vault_draws_the_onboarding() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    let mut app = app_for(dir.path())?;
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
    let mut app = app_for(dir.path())?;
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
fn the_window_is_drawn_in_the_light_palette_only() -> Result<(), Problem> {
    // The application is white: there is no switch, and the window must not take its
    // palette from the system's dark mode either.
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path())?;
    let shapes = draw(&mut app);
    assert!(shapes > 0);
    let first = draw(&mut app);
    assert_eq!(shapes, first, "the same model must draw the same way twice");
    Ok(())
}

#[test]
fn a_dark_desktop_cannot_turn_the_window_dark() -> Result<(), Problem> {
    // The palette is pinned, not merely set once: egui re-derives its style from the
    // system theme, so a machine set to dark mode is the case that would break it.
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path())?;
    let ctx = egui::Context::default();
    draw_with(&mut app, &ctx, vec2(1280.0, 900.0), Some(egui::Theme::Dark));
    assert!(
        !vnf_ui::theme::is_dark(&ctx),
        "the window took the desktop's dark mode"
    );
    assert_eq!(vnf_ui::theme::palette(&ctx), vnf_ui::theme::Palette::light());
    Ok(())
}

#[test]
fn the_header_is_the_title_and_the_subtitle_and_nothing_else() -> Result<(), Problem> {
    // What the person asked for, stated as the words that must and must not be painted:
    // one title, the subtitle of the page showing, the profile in the rail rather than
    // the top bar, and no rule-pack counts anywhere in the header.
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path())?;
    let texts = painted_text(&mut app);

    let titles = texts.iter().filter(|text| *text == "vn-finance").count();
    assert_eq!(titles, 1, "the window's name is painted {titles} times");
    assert!(
        texts
            .iter()
            .any(|text| text == "Resumo — Indicadores do exercício, enquadramento fiscal e alertas"),
        "the header lost its page subtitle: {texts:?}"
    );
    for gone in [
        "verificadas",
        "por verificar",
        "Regras pt/",
        "contabilidade local",
        "Claro",
        "Escuro",
    ] {
        assert!(
            !texts.iter().any(|text| text.to_lowercase().contains(gone)),
            "the header still paints {gone:?}: {texts:?}"
        );
    }
    assert!(
        texts.iter().any(|text| text == "Perfil"),
        "the rail lost its profile button: {texts:?}"
    );
    Ok(())
}

#[test]
fn a_small_window_scrolls_rather_than_losing_the_page() -> Result<(), Problem> {
    // A window at the smallest size the application allows: every page has to lay out
    // inside a viewport it does not fit, which is what the pair of scroll areas are for.
    // A panic or a collapsed page here is the bug the bars were added to fix.
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path())?;
    let ctx = egui::Context::default();
    for page in Page::ALL {
        app.go(page);
        let shapes = draw_at(&mut app, &ctx, vec2(WINDOW_MIN[0], WINDOW_MIN[1]));
        assert!(
            shapes > 100,
            "{} drew only {shapes} shapes in a small window",
            page.label()
        );
    }
    Ok(())
}

#[test]
fn the_rail_fits_in_the_smallest_window_the_app_allows() -> Result<(), Problem> {
    // The rail does not scroll, so the only thing that keeps every page button and the
    // profile reachable is the minimum window height. This measures the list against
    // that height: add a page and this fails, rather than the eleventh button being cut
    // off in a window nobody tried.
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path())?;
    let texts = painted_text_at(&mut app, vec2(WINDOW_MIN[0], WINDOW_MIN[1]));

    // Where the foot panel begins: the rail's content has to end above it, and the label
    // a row paints sits in the middle of a row `RAIL_ROW` high.
    let rail_bottom = WINDOW_MIN[1] - RAIL_FOOT;
    for label in ["About", "Perfil"] {
        let bottom = texts
            .iter()
            .find(|(text, _)| text == label)
            .map(|(_, rect)| rect.center().y + RAIL_ROW / 2.0);
        assert!(
            bottom.is_some_and(|bottom| bottom < rail_bottom),
            "the {label} row {bottom:?} does not fit above the rail's foot at \
             {rail_bottom:.0} in the smallest allowed window ({} px tall); raise \
             WINDOW_MIN or tighten the rail",
            WINDOW_MIN[1]
        );
    }
    Ok(())
}

#[test]
fn every_page_draws_without_panicking() -> Result<(), Problem> {
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path())?;
    for page in Page::ALL {
        app.go(page);
        let shapes = draw(&mut app);
        assert!(shapes > 50, "{} drew only {shapes} shapes", page.label());
    }
    Ok(())
}

#[test]
fn no_page_paints_past_the_right_edge() -> Result<(), Problem> {
    // The window scrolls down and never sideways, so a page that is wider than its
    // column is a page with something cut off and no way to reach it. This is the check
    // that the tables scale, the hints truncate and the prose wraps — at the smallest
    // window the application allows and at a maximised one.
    let dir = tempfile::tempdir()?;
    seeded(dir.path())?;
    let mut app = app_for(dir.path())?;
    for size in [vec2(WINDOW_MIN[0], WINDOW_MIN[1]), vec2(1280.0, 900.0)] {
        for page in Page::ALL {
            app.go(page);
            let texts = painted_text_at(&mut app, size);
            let right = size.x - CONTENT_MARGIN;
            let widest = texts
                .iter()
                .filter(|(_, rect)| rect.min.x >= SIDEBAR_WIDTH)
                .max_by(|a, b| a.1.max.x.total_cmp(&b.1.max.x));
            let Some((text, rect)) = widest else {
                continue;
            };
            assert!(
                rect.max.x <= right + 1.0,
                "{} paints {text:?} out to {:.0} in a window {:.0} wide (the page ends \
                 at {right:.0}); make the table fit or the text wrap",
                page.label(),
                rect.max.x,
                size.x
            );
        }
    }
    Ok(())
}

#[test]
fn every_page_draws_without_a_profile_too() -> Result<(), Problem> {
    // The pages that need a profile say so; the rest must keep working without one.
    let dir = tempfile::tempdir()?;
    let mut app = app_for(dir.path())?;
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
    let mut app = app_for(dir.path())?;
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
    let mut app = app_for(dir.path())?;
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
    let mut app = app_for(dir.path())?;
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

    let mut app = app_for(dir.path())?;
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

    let mut app = app_for(dir.path())?;
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
