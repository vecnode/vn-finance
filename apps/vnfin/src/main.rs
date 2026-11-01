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
use vnf_ui::{App, Session};

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
            .with_min_inner_size(WINDOW_MIN),
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
