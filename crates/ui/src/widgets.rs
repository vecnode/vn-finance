//! The small vocabulary every page is built from.
//!
//! The browser panel expressed these in CSS: a card was a `border: 1px` and a
//! `border-radius: 6px`, a pill was a coloured span, a KPI was a grid track. egui has
//! no cascade, so each one is a function here — built once, with the stylesheet's own
//! measurements, so a page reads as a page of this product rather than as a form.
//!
//! Nothing here decides anything. A widget formats and lays out what the model
//! already computed.

use egui::{
    Align, Color32, FontId, Label, Layout, RichText, Sense, Stroke, TextStyle, Ui, Vec2, text::LayoutJob,
    text::TextFormat,
};

use crate::theme::{self, Palette, palette};

/// What a status colour means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Good news.
    Ok,
    /// Something to look at.
    Warn,
    /// Something is wrong.
    Danger,
    /// Unremarkable, in the neutral grey.
    Slate,
    /// Not applicable, in the faintest grey.
    Na,
    /// A link, a selection, a primary action.
    Accent,
}

impl Tone {
    /// The foreground, background and border this tone uses.
    #[must_use]
    pub fn colours(self, palette: &Palette) -> (Color32, Color32, Color32) {
        match self {
            Self::Ok => (palette.ok, palette.ok_bg, palette.ok_border),
            Self::Warn => (palette.warn, palette.warn_bg, palette.warn_border),
            Self::Danger => (palette.danger, palette.danger_bg, palette.danger_border),
            Self::Slate => (palette.slate, palette.slate_bg, palette.slate_border),
            Self::Na => (palette.text_4, Color32::TRANSPARENT, palette.border_2),
            Self::Accent => (palette.accent, palette.accent_bg, palette.accent_border),
        }
    }
}

/// A bordered surface.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let palette = palette(ui.ctx());
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(theme::radius())
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A card with a coloured edge on the left, for something that needs attention.
pub fn accented_card<R>(ui: &mut Ui, tone: Tone, add: impl FnOnce(&mut Ui) -> R) -> R {
    let palette = palette(ui.ctx());
    let (foreground, background, _) = tone.colours(&palette);
    let response = egui::Frame::new()
        .fill(background)
        .stroke(Stroke::new(1.0, foreground.gamma_multiply(0.35)))
        .corner_radius(theme::radius())
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    // A frame stroke is one width on every edge; the design's alert has a 4 px bar on
    // the left only, so it is painted over the edge rather than drawn by the frame.
    let rect = response.response.rect;
    let bar = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + 4.0, rect.bottom()));
    ui.painter().rect_filled(bar, theme::radius_small(), foreground);
    response.inner
}

/// A section heading, its hint on the right, and the sentence saying why it exists.
///
/// The lead is not decoration: the person this is for has never had to think about a
/// `declaração periódica`, and a heading alone does not teach.
pub fn section(ui: &mut Ui, title: &str, hint: Option<&str>, lead: Option<&str>) {
    let palette = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .size(theme::SECTION_TITLE)
                .strong()
                .color(palette.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(hint) = hint {
                ui.label(RichText::new(hint).size(theme::CAPTION).color(palette.text_3));
            }
        });
    });
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, palette.border));
    if let Some(lead) = lead {
        ui.add_space(8.0);
        ui.scope(|ui| {
            ui.set_max_width(760.0);
            ui.label(RichText::new(lead).size(theme::BODY).color(palette.text_2));
        });
    }
    ui.add_space(12.0);
}

/// An uppercase micro-label with the letter-spacing the stylesheet uses.
///
/// egui has no tracking on a plain label, so this lays the text out by hand. Without
/// it an uppercase label at 11 px reads as a shout rather than as a label.
pub fn micro_label(ui: &mut Ui, text: &str, colour: Color32) {
    let font = FontId::proportional(theme::MICRO);
    let mut job = LayoutJob::single_section(
        text.to_uppercase(),
        TextFormat {
            font_id: font.clone(),
            color: colour,
            extra_letter_spacing: 0.6,
            ..Default::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    ui.label(job);
}

/// A caption under something, or a fact about the last refresh.
pub fn caption(ui: &mut Ui, text: &str) {
    let palette = palette(ui.ctx());
    ui.label(RichText::new(text).size(theme::CAPTION).color(palette.text_3));
}

/// A footnote: where a number came from, or what it does not include.
pub fn footnote(ui: &mut Ui, text: &str) {
    let palette = palette(ui.ctx());
    ui.scope(|ui| {
        ui.set_max_width(880.0);
        ui.label(RichText::new(text).size(theme::BODY_SMALL).color(palette.text_4));
    });
}

/// A status pill: a bordered label with a leading mark.
pub fn pill(ui: &mut Ui, text: &str, tone: Tone) {
    let palette = palette(ui.ctx());
    let (foreground, background, border) = tone.colours(&palette);
    let fill = if background == Color32::TRANSPARENT {
        palette.surface
    } else {
        background
    };
    let stroke = Stroke::new(1.0, border);
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(theme::radius_small())
        .inner_margin(egui::Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(theme::CAPTION)
                    .strong()
                    .color(foreground),
            );
        });
}

/// A metadata badge: smaller than a pill, uppercase, and quieter.
pub fn badge(ui: &mut Ui, text: &str, tone: Tone) {
    let palette = palette(ui.ctx());
    let (foreground, background, border) = tone.colours(&palette);
    let fill = if background == Color32::TRANSPARENT {
        palette.surface_2
    } else {
        background
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(theme::radius_small())
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            let font = FontId::proportional(theme::MICRO);
            let mut job = LayoutJob::single_section(
                text.to_uppercase(),
                TextFormat {
                    font_id: font,
                    color: foreground,
                    extra_letter_spacing: 0.4,
                    ..Default::default()
                },
            );
            job.wrap.max_width = f32::INFINITY;
            ui.label(job);
        });
}

/// A severity tag: the leading element of an alert row.
pub fn severity(ui: &mut Ui, text: &str, tone: Tone) {
    let palette = palette(ui.ctx());
    let (foreground, background, border) = tone.colours(&palette);
    egui::Frame::new()
        .fill(background)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(theme::radius_small())
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            let font = FontId::proportional(theme::MICRO);
            let mut job = LayoutJob::single_section(
                text.to_uppercase(),
                TextFormat {
                    font_id: font,
                    color: foreground,
                    extra_letter_spacing: 0.55,
                    ..Default::default()
                },
            );
            job.wrap.max_width = f32::INFINITY;
            ui.label(job);
        });
}

/// A counter in the rail: a small rounded number, coloured by how urgent it is.
pub fn count(ui: &mut Ui, text: &str, tone: Tone) {
    let palette = palette(ui.ctx());
    let (foreground, _, _) = tone.colours(&palette);
    egui::Frame::new()
        .fill(palette.surface_3)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(7, 0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(theme::MICRO).color(foreground));
        });
}

/// One figure in the strip at the top of the dashboard.
pub struct Kpi<'a> {
    /// What it is.
    pub label: &'a str,
    /// The number, already formatted.
    pub value: &'a str,
    /// The sentence under it saying what it counts.
    pub support: Option<&'a str>,
    /// The quiet line at the bottom: how it was arrived at.
    pub foot: Option<&'a str>,
}

/// The strip of figures, divided by hairlines rather than separated by gaps.
pub fn kpi_row(ui: &mut Ui, items: &[Kpi<'_>]) {
    let palette = palette(ui.ctx());
    if items.is_empty() {
        return;
    }
    let count = items.len();
    let margin = 16.0_f32;
    let response = egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(theme::radius())
        .inner_margin(egui::Margin::symmetric(margin as i8, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = (ui.available_width() - margin * 2.0) / count as f32;
            ui.horizontal_top(|ui| {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        ui.add_space(margin);
                    }
                    ui.allocate_ui_with_layout(
                        Vec2::new(width - margin, 0.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            micro_label(ui, item.label, palette.text_3);
                            ui.add_space(6.0);
                            ui.label(RichText::new(item.value).size(theme::FIGURE).color(palette.text));
                            if let Some(support) = item.support {
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(support)
                                        .size(theme::BODY_SMALL)
                                        .color(palette.text_3),
                                );
                            }
                            if let Some(foot) = item.foot {
                                ui.add_space(8.0);
                                let (rule, _) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 1.0),
                                    Sense::hover(),
                                );
                                dotted(ui, rule, palette.border_2);
                                ui.add_space(4.0);
                                ui.label(RichText::new(foot).size(theme::MICRO).color(palette.text_4));
                            }
                        },
                    );
                }
            });
        });
    // The dividers, painted after the fact because the frame is as tall as its
    // tallest column and only the frame knows that height.
    let rect = response.response.rect;
    let top = rect.top() + 1.0;
    let bottom = rect.bottom() - 1.0;
    for index in 1..count {
        let x = rect.left() + margin + index as f32 * ((rect.width() - margin * 2.0) / count as f32)
            - margin / 2.0;
        ui.painter()
            .vline(x, top..=bottom, Stroke::new(1.0, palette.border));
    }
}

/// A dotted rule, which the design uses under a definition and inside a KPI's foot.
fn dotted(ui: &Ui, rect: egui::Rect, colour: Color32) {
    let stroke = Stroke::new(1.0, colour);
    ui.painter().add(egui::Shape::dashed_line(
        &[rect.left_center(), rect.right_center()],
        stroke,
        2.0,
        3.0,
    ));
}

/// A horizontal meter: a fraction, drawn as a track and a fill.
pub fn meter(ui: &mut Ui, fraction: f32, label: &str, value: &str) {
    let palette = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(theme::MICRO).color(palette.text_3));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .size(theme::CAPTION)
                    .strong()
                    .color(palette.text),
            );
        });
    });
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(2), palette.surface_3);
    let filled = egui::Rect::from_min_size(
        rect.min,
        Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    ui.painter()
        .rect_filled(filled, egui::CornerRadius::same(2), palette.accent);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(2),
        Stroke::new(1.0, palette.border),
        egui::StrokeKind::Inside,
    );
}

/// One definition: a label, its value, and a quiet line under it.
pub struct Def<'a> {
    /// What the row is about.
    pub label: &'a str,
    /// The read-only value.
    pub value: &'a str,
    /// A sentence under the value.
    pub small: Option<&'a str>,
}

/// The definition list the enquadramento uses: a fixed label column and dotted rules.
pub fn defs(ui: &mut Ui, rows: &[Def<'_>]) {
    let palette = palette(ui.ctx());
    for row in rows {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(172.0, 0.0), Layout::top_down(Align::LEFT), |ui| {
                ui.label(
                    RichText::new(row.label)
                        .size(theme::BODY_SMALL)
                        .color(palette.text_3),
                );
            });
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.label(RichText::new(row.value).size(theme::BODY).color(palette.text));
                    if let Some(small) = row.small {
                        ui.label(RichText::new(small).size(theme::CAPTION).color(palette.text_4));
                    }
                },
            );
        });
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
        dotted(ui, rect, palette.border);
        ui.add_space(6.0);
    }
}

/// A list of one-line notes, each with a coloured mark.
pub fn notes(ui: &mut Ui, tone: Tone, lines: &[String]) {
    let palette = palette(ui.ctx());
    let (foreground, _, _) = tone.colours(&palette);
    for line in lines {
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 3.0, foreground);
            ui.scope(|ui| {
                ui.set_max_width(ui.available_width());
                ui.label(RichText::new(line).size(theme::BODY_SMALL).color(palette.text_2));
            });
        });
    }
}

/// The empty state: a dashed box that says what is missing, not that something failed.
pub fn empty(ui: &mut Ui, title: &str, body: &str) {
    let palette = palette(ui.ctx());
    let response = egui::Frame::new()
        .fill(palette.surface_2)
        .stroke(Stroke::new(1.0, palette.border_2))
        .corner_radius(theme::radius())
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .size(theme::BODY)
                    .strong()
                    .color(palette.text_2),
            );
            ui.label(RichText::new(body).size(theme::CAPTION).color(palette.text_3));
        });
    // A dashed border is one more thing a frame cannot express.
    let rect = response.response.rect;
    ui.painter().add(egui::Shape::dashed_line(
        &[
            rect.left_bottom(),
            rect.right_bottom(),
            rect.right_top(),
            rect.left_top(),
            rect.left_bottom(),
        ],
        Stroke::new(1.0, palette.border_2),
        4.0,
        3.0,
    ));
}

/// A key and a value, with the value selectable and monospaced.
pub fn key_value(ui: &mut Ui, key: &str, value: &str) {
    let palette = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(Vec2::new(150.0, 0.0), Layout::top_down(Align::LEFT), |ui| {
            ui.label(RichText::new(key).size(theme::MICRO).color(palette.text_3));
        });
        ui.add(
            Label::new(
                RichText::new(value)
                    .size(theme::MONO)
                    .monospace()
                    .color(palette.text_2),
            )
            .truncate()
            .selectable(true),
        );
    });
}

/// A monospaced, selectable path or hash.
pub fn path(ui: &mut Ui, value: &str) {
    let palette = palette(ui.ctx());
    if value.is_empty() {
        ui.label(RichText::new("—").size(theme::BODY_SMALL).color(palette.text_4));
        return;
    }
    ui.add(
        Label::new(
            RichText::new(value)
                .size(theme::MONO)
                .monospace()
                .color(palette.text_2),
        )
        .wrap()
        .selectable(true),
    );
}

/// A chip, for the row of choices above a list.
pub fn chip(ui: &mut Ui, selected: bool, text: &str) -> egui::Response {
    let palette = palette(ui.ctx());
    let (colour, fill, stroke) = if selected {
        (
            palette.accent,
            palette.accent_bg,
            Stroke::new(1.0, palette.accent_border),
        )
    } else {
        (
            palette.text_2,
            Color32::TRANSPARENT,
            Stroke::new(1.0, palette.border),
        )
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(theme::MICRO).color(colour))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(theme::radius_small())
            .min_size(Vec2::new(0.0, 24.0)),
    )
}

/// A secondary button, sized like the ones in the design.
pub fn button(ui: &mut Ui, text: &str) -> egui::Response {
    let palette = palette(ui.ctx());
    ui.add(
        egui::Button::new(RichText::new(text).size(theme::BODY_SMALL).color(palette.text_2))
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border_2))
            .corner_radius(theme::radius())
            .min_size(Vec2::new(0.0, 28.0)),
    )
}

/// A primary button.
pub fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    let palette = palette(ui.ctx());
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .size(theme::BODY_SMALL)
                .strong()
                .color(palette.accent_fg),
        )
        .fill(palette.accent)
        .stroke(Stroke::new(1.0, palette.accent))
        .corner_radius(theme::radius())
        .min_size(Vec2::new(0.0, 28.0)),
    )
}

/// A labelled text field, for the forms.
pub fn field(ui: &mut Ui, label: &str, hint: &str, value: &mut String) {
    let palette = palette(ui.ctx());
    ui.label(
        RichText::new(label)
            .size(theme::BODY_SMALL)
            .strong()
            .color(palette.text_2),
    );
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .font(TextStyle::Body)
            .margin(egui::Margin::symmetric(10, 7)),
    );
    ui.add_space(4.0);
}

/// A number, big, in the place a page states its total.
pub fn figure(ui: &mut Ui, value: &str) {
    let palette = palette(ui.ctx());
    ui.label(
        RichText::new(value)
            .size(theme::FIGURE_SMALL)
            .strong()
            .color(palette.text),
    );
}

/// One column of a table.
#[derive(Debug, Clone, Copy)]
pub struct Column {
    /// The heading.
    pub label: &'static str,
    /// A fixed width, or `0.0` to share whatever the fixed columns leave.
    pub width: f32,
    /// Where the content sits inside its column.
    pub align: Align,
}

impl Column {
    /// A flexible column.
    #[must_use]
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            width: 0.0,
            align: Align::LEFT,
        }
    }

    /// A column of a fixed width.
    #[must_use]
    pub fn at(label: &'static str, width: f32) -> Self {
        Self {
            label,
            width,
            align: Align::LEFT,
        }
    }

    /// Right-align it, which is what a number wants.
    #[must_use]
    pub fn right(mut self) -> Self {
        self.align = Align::RIGHT;
        self
    }
}

/// Work out each column's width: the fixed ones keep theirs, the rest share.
fn widths_for(available: f32, columns: &[Column]) -> Vec<f32> {
    let fixed: f32 = columns
        .iter()
        .filter(|column| column.width > 0.0)
        .map(|column| column.width)
        .sum();
    let flexible = columns.iter().filter(|column| column.width <= 0.0).count().max(1) as f32;
    // A floor, so a narrow window scrolls the table rather than collapsing a column
    // to nothing and clipping a figure mid-digit.
    let share = ((available - fixed) / flexible).max(64.0);
    columns
        .iter()
        .map(|column| if column.width > 0.0 { column.width } else { share })
        .collect()
}

/// A table: a heading row and one row per item, with hairlines between.
///
/// The design's tables are `width: 100%` with auto columns; this distributes the same
/// way, fixed widths first and the rest sharing what is left. Cells are filled by
/// calling the accessors on [`Cells`] in column order, which is what keeps a row from
/// drifting out of step with its heading.
pub fn table<T>(
    ui: &mut Ui,
    columns: &[Column],
    items: &[T],
    highlight: impl Fn(&T) -> bool,
    mut row: impl FnMut(&mut Cells<'_, '_>, &T),
) {
    let palette = palette(ui.ctx());
    let widths = widths_for((ui.available_width() - 20.0).max(320.0), columns);

    egui::Frame::new()
        .fill(palette.surface_2)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                for (index, column) in columns.iter().enumerate() {
                    let width = widths.get(index).copied().unwrap_or(80.0);
                    ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(column.align), |ui| {
                        micro_label(ui, column.label, palette.text_3);
                    });
                }
            });
        });

    for item in items {
        let fill = if highlight(item) {
            palette.accent_bg
        } else {
            Color32::TRANSPARENT
        };
        let response = egui::Frame::new()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let mut cells = Cells::new(ui, &widths, columns);
                    row(&mut cells, item);
                });
            });
        let rect = response.response.rect;
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, palette.border));
    }
}

/// The pen a table row writes with: one call per column, in order.
pub struct Cells<'u, 'c> {
    ui: &'u mut Ui,
    widths: &'c [f32],
    columns: &'c [Column],
    index: usize,
}

impl<'u, 'c> Cells<'u, 'c> {
    fn new(ui: &'u mut Ui, widths: &'c [f32], columns: &'c [Column]) -> Self {
        Self {
            ui,
            widths,
            columns,
            index: 0,
        }
    }

    /// Move to the next column and draw in it.
    fn advance(&mut self, add: impl FnOnce(&mut Ui)) {
        let Some(column) = self.columns.get(self.index) else {
            return;
        };
        let width = self.widths.get(self.index).copied().unwrap_or(80.0);
        let align = column.align;
        self.index += 1;
        self.ui
            .allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(align), |ui| {
                ui.set_max_width(width);
                add(ui);
            });
    }

    /// Plain text.
    pub fn text(&mut self, value: &str) {
        let palette = palette(self.ui.ctx());
        self.advance(|ui| {
            ui.label(RichText::new(value).size(theme::BODY).color(palette.text));
        });
    }

    /// The row's subject, in the emphasis the design gives it.
    pub fn strong(&mut self, value: &str) {
        let palette = palette(self.ui.ctx());
        self.advance(|ui| {
            ui.label(
                RichText::new(value)
                    .size(theme::BODY)
                    .strong()
                    .color(palette.text),
            );
        });
    }

    /// A quiet cell: a caption, a legal basis.
    pub fn muted(&mut self, value: &str) {
        let palette = palette(self.ui.ctx());
        self.advance(|ui| {
            ui.label(RichText::new(value).size(theme::CAPTION).color(palette.text_3));
        });
    }

    /// A date or a hash, in the monospace face.
    pub fn mono(&mut self, value: &str) {
        let palette = palette(self.ui.ctx());
        self.advance(|ui| {
            ui.label(
                RichText::new(value)
                    .size(theme::MONO)
                    .monospace()
                    .color(palette.text_2),
            );
        });
    }

    /// A dash, for a cell that has nothing to say.
    pub fn nothing(&mut self) {
        let palette = palette(self.ui.ctx());
        self.advance(|ui| {
            ui.label(RichText::new("—").size(theme::BODY_SMALL).color(palette.text_4));
        });
    }

    /// A status pill.
    pub fn pill(&mut self, text: &str, tone: Tone) {
        self.advance(|ui| pill(ui, text, tone));
    }

    /// A metadata badge.
    pub fn badge(&mut self, text: &str, tone: Tone) {
        self.advance(|ui| badge(ui, text, tone));
    }

    /// Anything else, drawn by the caller.
    pub fn custom(&mut self, add: impl FnOnce(&mut Ui)) {
        self.advance(add);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_has_a_readable_pair() {
        let light = Palette::light();
        for tone in [
            Tone::Ok,
            Tone::Warn,
            Tone::Danger,
            Tone::Slate,
            Tone::Na,
            Tone::Accent,
        ] {
            let (foreground, background, border) = tone.colours(&light);
            assert_ne!(foreground, Color32::TRANSPARENT, "{tone:?} must have a colour");
            assert_ne!(border, Color32::TRANSPARENT, "{tone:?} must have a border");
            // `Na` is the one tone whose fill is the surface behind it.
            if tone != Tone::Na {
                assert_ne!(background, Color32::TRANSPARENT, "{tone:?} must have a fill");
            }
        }
    }
}
