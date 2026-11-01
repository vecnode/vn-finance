//! vn-finance — one window, one folder, no network.
//!
//! The binary does three things the libraries deliberately do not: it reads the clock,
//! it opens the vault the machine resolves, and it hands both to the interface. A
//! refusal — the folder is inside a repository, another window holds it — is shown in
//! the window rather than written to a console nobody sees, because this build has no
//! console.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![allow(unreachable_pub)]
// A release build is a window, not a terminal: without this Windows attaches a
// console that flashes behind the window on every launch.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clock;

use vnf_ui::app::{Failure, WINDOW_MIN, WINDOW_START};
use vnf_ui::{App, Session, brand};

/// The icon is square and this many pixels on a side.
///
/// The bytes are the application tile from `assets/logo/`, and they are the same bytes
/// the header paints: one constant, so the title bar and the window cannot disagree.
/// The tile's ink fill is what keeps a white crystal readable on a light title bar as
/// well as a dark one, and it costs the application no image decoder
/// (`assets/logo/BRAND.md`).
const ICON_SIZE: u32 = 64;

fn main() -> eframe::Result<()> {
    let today = clock::today_in_lisbon();
    let year = today.get(..4).and_then(|year| year.parse().ok()).unwrap_or(2026);
    let version = env!("CARGO_PKG_VERSION").to_owned();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("vn-finance {version}"))
            .with_app_id("vn-finance")
            .with_inner_size(WINDOW_START)
            // Both sizes live in the interface crate: the smallest allowed height is
            // what keeps the rail's ten pages and the profile on screen, so the number
            // belongs beside the rail rather than beside the window's title bar.
            .with_min_inner_size(WINDOW_MIN)
            .with_icon(egui::IconData {
                rgba: brand::TILE.to_vec(),
                width: ICON_SIZE,
                height: ICON_SIZE,
            }),
        ..Default::default()
    };

    eframe::run_native(
        "vn-finance",
        options,
        Box::new(move |cc| match Session::open(&version) {
            Ok(session) => Ok(Box::new(App::new(cc, session, &today, year))),
            Err(error) => Ok(Box::new(Failure::new(cc, error.to_string(), &today))),
        }),
    )
}
