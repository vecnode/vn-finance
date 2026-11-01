//! The application mark: the crystal on the suite's ink tile.
//!
//! One picture, embedded rather than loaded. [`TILE`] is the 64 × 64 unmultiplied RGBA
//! that `assets/logo/vn-finance-app.rgba` ships, and `apps/vnfin` builds the window icon
//! from the same constant, so the taskbar and the header cannot drift apart. A PNG would
//! have cost an image decoder for one 16 KiB picture; these bytes are exactly the three
//! fields `egui::IconData` takes.
//!
//! The tile is ink, not cobalt. An icon has to hold its own against whatever is behind
//! it — a bare cobalt glyph sinks into a dark taskbar — and an ink tile reads on a white
//! header as well as on a dark title bar, so nothing here swaps artwork when the theme
//! changes. The colour inside is the application's identity and never a status
//! (`assets/logo/BRAND.md`).

use egui::{Color32, ColorImage, Context, Painter, Pos2, Rect, TextureHandle, TextureOptions, Ui, Vec2};

/// The application tile: 64 × 64 unmultiplied RGBA, on the suite's ink.
pub const TILE: &[u8] = include_bytes!("../../../assets/logo/vn-finance-app.rgba");

/// The side of [`TILE`], in pixels.
const SIDE: usize = 64;

/// Upload [`TILE`] as a texture.
///
/// Call this once per context and keep the handle: a texture is freed when its handle
/// drops, so a caller that uploaded per frame would upload every frame.
pub fn texture(ctx: &Context) -> TextureHandle {
    // A mark that is not the size it claims costs the picture, not the window: an asset
    // edited by hand is worth seeing missing rather than worth a panic on startup.
    let image = if TILE.len() == SIDE * SIDE * 4 {
        ColorImage::from_rgba_unmultiplied([SIDE, SIDE], TILE)
    } else {
        ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 0])
    };
    ctx.load_texture("vn-finance.mark", image, TextureOptions::LINEAR)
}

/// Paint the mark into `rect`.
///
/// The tile carries its own rounded corners in its alpha channel, so the picture needs
/// no clipping and the header needs no rounded-frame special case.
pub fn paint(painter: &Painter, rect: Rect, texture: &TextureHandle) {
    let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(texture.id(), rect, whole, Color32::WHITE);
}

/// Reserve a square of `size` for the mark, and paint it there.
///
/// Across the window the mark is only ever this: a square beside the name it belongs
/// to. It is never a button, so it takes no sense beyond a hover.
pub fn inline(ui: &mut Ui, size: f32, texture: &TextureHandle) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint(ui.painter(), rect, texture);
    response
}
