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

use vnf_ui::app::Failure;
use vnf_ui::{App, Session};

/// The window's starting size.
const STARTING_SIZE: [f32; 2] = [1180.0, 780.0];

/// The smallest the window may be made before the layout stops reflowing.
const MINIMUM_SIZE: [f32; 2] = [920.0, 600.0];

fn main() -> eframe::Result<()> {
    let today = clock::today_in_lisbon();
    let year = today.get(..4).and_then(|year| year.parse().ok()).unwrap_or(2026);
    let version = env!("CARGO_PKG_VERSION").to_owned();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("vn-finance {version}"))
            .with_app_id("vn-finance")
            .with_inner_size(STARTING_SIZE)
            .with_min_inner_size(MINIMUM_SIZE),
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
