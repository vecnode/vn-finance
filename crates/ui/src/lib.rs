//! vn-finance's interface: the theme, the widgets, the icons and the ten pages.
//!
//! This crate draws and returns what it wants done; it never touches the vault. The
//! loop is the browser panel's best property turned into a type: every write produces
//! a new model and the window redraws from it whole, rather than patching what it
//! hopes changed.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![allow(unreachable_pub)]

pub mod app;
pub mod icons;
pub mod pages;
pub mod session;
pub mod theme;
pub mod widgets;

pub use app::App;
pub use session::Session;
