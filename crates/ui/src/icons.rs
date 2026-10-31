//! The line icons the sidebar and the status lines use.
//!
//! The panel drew these with inline SVG at 15 px and a 1.3 stroke. egui has no SVG,
//! and a glyph font would be a megabyte for eleven shapes, so each one is a short
//! polyline drawn into the rectangle the caller reserved — the same geometry, half a
//! page of code and no dependency.

use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

/// Which shape to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// The dashboard: four small squares.
    Dashboard,
    /// The agenda: a calendar.
    Calendar,
    /// The ledger: a document with a folded corner.
    Invoice,
    /// Segurança Social: a shield.
    Shield,
    /// IVA: a percentage sign.
    Percent,
    /// IRS: a form.
    Form,
    /// The vault: a padlock.
    Lock,
    /// The law: an open book.
    Book,
    /// About: an information mark.
    Info,
    /// Show or hide: an eye.
    Eye,
}

/// Draw an icon centred in `rect`.
///
/// The shapes are authored on a 16 × 16 grid and scaled to whatever rectangle the
/// caller reserved, so the same call works at 15 px in the rail and at 20 px in a
/// header.
pub fn draw(painter: &Painter, rect: Rect, colour: Color32, icon: Icon) {
    let scale = rect.width().min(rect.height()) / 16.0;
    let origin = rect.min;
    let point = |x: f32, y: f32| Pos2::new(origin.x + x * scale, origin.y + y * scale);
    let stroke = Stroke::new((1.3 * scale).max(1.0), colour);
    let line = |points: &[(f32, f32)]| {
        let positions: Vec<Pos2> = points.iter().map(|(x, y)| point(*x, *y)).collect();
        painter.add(egui::Shape::line(positions, stroke));
    };
    let square = |x: f32, y: f32, size: f32| {
        painter.rect_stroke(
            Rect::from_min_size(point(x, y), Vec2::splat(size * scale)),
            egui::CornerRadius::same(1),
            stroke,
            egui::StrokeKind::Middle,
        );
    };

    match icon {
        Icon::Dashboard => {
            for (x, y) in [(2.0, 2.0), (9.0, 2.0), (2.0, 9.0), (9.0, 9.0)] {
                square(x, y, 5.0);
            }
        }
        Icon::Calendar => {
            painter.rect_stroke(
                Rect::from_min_max(point(2.0, 3.0), point(14.0, 14.0)),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Middle,
            );
            line(&[(2.0, 6.6), (14.0, 6.6)]);
            line(&[(5.6, 1.6), (5.6, 4.4)]);
            line(&[(10.4, 1.6), (10.4, 4.4)]);
        }
        Icon::Invoice => {
            line(&[
                (3.6, 1.6),
                (9.0, 1.6),
                (12.4, 5.0),
                (12.4, 14.4),
                (3.6, 14.4),
                (3.6, 1.6),
            ]);
            line(&[(8.8, 1.6), (8.8, 5.2), (12.4, 5.2)]);
            line(&[(5.6, 8.6), (10.4, 8.6)]);
            line(&[(5.6, 11.0), (8.6, 11.0)]);
        }
        Icon::Shield => {
            line(&[
                (8.0, 1.6),
                (13.4, 3.6),
                (13.4, 8.6),
                (8.0, 14.7),
                (2.6, 8.6),
                (2.6, 3.6),
                (8.0, 1.6),
            ]);
        }
        Icon::Percent => {
            painter.circle_stroke(point(5.0, 5.0), 2.0 * scale, stroke);
            painter.circle_stroke(point(11.0, 11.0), 2.0 * scale, stroke);
            line(&[(12.6, 3.4), (3.4, 12.6)]);
        }
        Icon::Form => {
            painter.rect_stroke(
                Rect::from_min_max(point(3.0, 1.6), point(13.0, 14.4)),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Middle,
            );
            line(&[(5.4, 4.6), (10.6, 4.6)]);
            line(&[(5.4, 8.0), (7.0, 8.0)]);
            line(&[(9.0, 8.0), (10.6, 8.0)]);
            line(&[(5.4, 10.8), (7.0, 10.8)]);
            line(&[(9.0, 10.8), (10.6, 10.8)]);
        }
        Icon::Lock => {
            painter.rect_stroke(
                Rect::from_min_max(point(3.0, 7.0), point(13.0, 14.0)),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Middle,
            );
            line(&[(5.6, 7.0), (5.6, 5.2)]);
            painter.circle_stroke(point(8.0, 4.4), 1.2 * scale, stroke);
            line(&[(10.4, 7.0), (10.4, 5.2)]);
        }
        Icon::Book => {
            line(&[
                (8.0, 3.6),
                (6.5, 2.0),
                (3.0, 2.0),
                (3.0, 13.0),
                (6.5, 13.0),
                (8.0, 14.6),
                (9.5, 13.0),
                (13.0, 13.0),
                (13.0, 2.0),
                (9.5, 2.0),
                (8.0, 3.6),
            ]);
            line(&[(8.0, 3.6), (8.0, 14.6)]);
        }
        Icon::Info => {
            painter.circle_stroke(point(8.0, 8.0), 6.2 * scale, stroke);
            line(&[(8.0, 7.2), (8.0, 11.4)]);
            painter.circle_filled(point(8.0, 5.0), 0.8 * scale, colour);
        }
        Icon::Eye => {
            line(&[
                (1.4, 8.0),
                (3.8, 4.4),
                (8.0, 3.2),
                (12.2, 4.4),
                (14.6, 8.0),
                (12.2, 11.6),
                (8.0, 12.8),
                (3.8, 11.6),
                (1.4, 8.0),
            ]);
            painter.circle_stroke(point(8.0, 8.0), 1.9 * scale, stroke);
        }
    }
}

/// Reserve a square of `size` beside the text that follows, and draw an icon in it.
pub fn inline(ui: &mut egui::Ui, icon: Icon, size: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    draw(ui.painter(), rect, colour, icon);
}
