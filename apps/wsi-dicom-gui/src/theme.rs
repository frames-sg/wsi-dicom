//! Visual theme for the wsi-dicom GUI.
//!
//! Aesthetic: editorial laboratory — warm pastel paper, hairline rules, serif
//! display type, monospace for paths and reports. A tetradic palette of four
//! pastel neutrals (sand, sage, steel, mauve, 90° apart on the hue wheel) is
//! used informationally — one chip per section — rather than decoratively.

use std::sync::Arc;

use eframe::egui::{
    self, style::HandleShape, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Margin, Pos2, Rect, Response, RichText, Shadow, Stroke, TextStyle, Ui, Vec2, WidgetText,
};

// ============================================================================
// Coast — tetradic pastel neutrals at hue base 15° (90° offsets).
//   peach    15°   warm soft coral
//   sage    105°   fresh garden green
//   azure   195°   open sky blue
//   heather 285°   cool violet-gray
// Names kept stable across the codebase (SAND / SAGE / STEEL / MAUVE) so the
// section assignments don't churn — only the actual hex values move.
// ============================================================================

pub const SAND: Color32 = Color32::from_rgb(0xEC, 0xC9, 0xB2);
pub const SAGE: Color32 = Color32::from_rgb(0xB9, 0xD5, 0xB0);
pub const STEEL: Color32 = Color32::from_rgb(0xB0, 0xC6, 0xE0);
pub const MAUVE: Color32 = Color32::from_rgb(0xCC, 0xB0, 0xD8);

pub const SAND_INK: Color32 = Color32::from_rgb(0xA1, 0x68, 0x3D);
pub const SAGE_INK: Color32 = Color32::from_rgb(0x4E, 0x82, 0x4F);
pub const STEEL_INK: Color32 = Color32::from_rgb(0x3F, 0x6F, 0x9F);
pub const MAUVE_INK: Color32 = Color32::from_rgb(0x8E, 0x5B, 0xA5);

// Cool light neutrals — modern UI canvas, not warm paper.
pub const PAPER: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF5);
pub const PAPER_SOFT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const PAPER_RAISED: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const PAPER_SUNKEN: Color32 = Color32::from_rgb(0xEC, 0xEC, 0xEF);

pub const RULE: Color32 = Color32::from_rgb(0xE0, 0xE0, 0xE3);

pub const INK: Color32 = Color32::from_rgb(0x18, 0x18, 0x1B);
pub const INK_MUTED: Color32 = Color32::from_rgb(0x52, 0x52, 0x5B);
pub const INK_FAINT: Color32 = Color32::from_rgb(0x8F, 0x8F, 0x99);

// Steel is the action color — it carries the most "instrument" weight.
pub const PRIMARY: Color32 = STEEL_INK;
pub const PRIMARY_TEXT_ON: Color32 = Color32::from_rgb(0xFB, 0xF7, 0xEE);

// ============================================================================
// Typography
// ============================================================================

/// Named font family for the display serif (brand mark, headings, primary
/// button labels). Falls back to the proportional family if no serif loaded.
pub fn display_family() -> FontFamily {
    FontFamily::Name(Arc::from("display"))
}

/// Named font family for body labels — short, serif on macOS for editorial
/// feel, falls back to the platform proportional default elsewhere.
pub fn body_family() -> FontFamily {
    FontFamily::Proportional
}

pub fn mono_family() -> FontFamily {
    FontFamily::Monospace
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    install_visuals(ctx);
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // Monospace fallback that ships with the binary — JetBrains Mono Regular,
    // SIL OFL. Cross-platform mono base; on macOS we prepend SF Mono.
    fonts.font_data.insert(
        "jetbrains_mono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );

    // Apple SF Pro (system sans, modern, screen-optimized) and SF Mono are
    // loaded at runtime from /System/Library/Fonts. Not redistributed with
    // the binary — consumed locally on the user's machine.
    let sf_pro = try_read_font("/System/Library/Fonts/SFNS.ttf");
    let sf_mono = try_read_font("/System/Library/Fonts/SFNSMono.ttf");

    let mono_chain = fonts
        .families
        .entry(FontFamily::Monospace)
        .or_insert_with(Vec::new);
    mono_chain.clear();
    if sf_mono.is_some() {
        mono_chain.push("sf_mono".to_owned());
    }
    mono_chain.push("jetbrains_mono".to_owned());
    if let Some(bytes) = sf_mono {
        fonts
            .font_data
            .insert("sf_mono".to_owned(), Arc::new(FontData::from_owned(bytes)));
    }

    // SF Pro becomes the proportional default on macOS, with the egui-default
    // Ubuntu-Light staying as a fallback for other platforms.
    if let Some(bytes) = sf_pro {
        fonts
            .font_data
            .insert("sf_pro".to_owned(), Arc::new(FontData::from_owned(bytes)));
        let proportional = fonts
            .families
            .entry(FontFamily::Proportional)
            .or_insert_with(Vec::new);
        proportional.insert(0, "sf_pro".to_owned());
    }

    // Display family — same SF Pro chain, used for the brand mark, headings
    // and primary CTA at larger sizes.
    let mut display_chain: Vec<String> = Vec::new();
    if fonts.font_data.contains_key("sf_pro") {
        display_chain.push("sf_pro".to_owned());
    }
    display_chain.push("Ubuntu-Light".to_owned());
    fonts
        .families
        .insert(FontFamily::Name(Arc::from("display")), display_chain);

    ctx.set_fonts(fonts);

    // Text styles — generous sizing, comfortable line-height. Heading sizes
    // bumped so the serif gets room to breathe; body kept at 13.5 for density.
    ctx.global_style_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(28.0, display_family()));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(16.0, body_family()));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::new(16.0, body_family()));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(13.5, body_family()));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::new(14.0, mono_family()));
    });
}

fn try_read_font(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

// ============================================================================
// Visuals & spacing
// ============================================================================

fn install_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.dark_mode = false;

    visuals.panel_fill = PAPER;
    visuals.window_fill = PAPER_SOFT;
    visuals.window_stroke = Stroke::new(1.0, RULE);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.faint_bg_color = PAPER_RAISED;
    visuals.extreme_bg_color = PAPER_SUNKEN;
    visuals.code_bg_color = PAPER_SUNKEN;
    visuals.override_text_color = Some(INK);
    visuals.weak_text_color = Some(INK_MUTED);
    visuals.hyperlink_color = STEEL_INK;
    visuals.warn_fg_color = SAND_INK;
    visuals.error_fg_color = MAUVE_INK;

    visuals.selection.bg_fill = SAND.linear_multiply(1.2);
    visuals.selection.stroke = Stroke::new(1.0, SAND_INK);

    visuals.handle_shape = HandleShape::Rect { aspect_ratio: 0.4 };
    visuals.slider_trailing_fill = true;
    visuals.striped = false;
    visuals.button_frame = true;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.indent_has_left_vline = false;
    visuals.disabled_alpha = 0.55;

    // Soft warm shadow — almost a paper drop.
    visuals.window_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(0x2A, 0x20, 0x10, 22),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(0x2A, 0x20, 0x10, 28),
    };

    // Widget states — fully borderless. Identity comes from fill tone alone.
    let radius = CornerRadius::same(8);
    visuals.widgets.noninteractive.bg_fill = PAPER_SOFT;
    visuals.widgets.noninteractive.weak_bg_fill = PAPER_SOFT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.noninteractive.corner_radius = radius;
    visuals.widgets.noninteractive.expansion = 0.0;

    visuals.widgets.inactive.bg_fill = PAPER_SUNKEN;
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.inactive.corner_radius = radius;
    visuals.widgets.inactive.expansion = 0.0;

    visuals.widgets.hovered.bg_fill = PAPER_SUNKEN;
    visuals.widgets.hovered.weak_bg_fill = PAPER_SUNKEN;
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.hovered.corner_radius = radius;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = STEEL.linear_multiply(0.9);
    visuals.widgets.active.weak_bg_fill = STEEL.linear_multiply(0.9);
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.active.corner_radius = radius;
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = PAPER_SUNKEN;
    visuals.widgets.open.weak_bg_fill = PAPER_SUNKEN;
    visuals.widgets.open.bg_stroke = Stroke::NONE;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.open.corner_radius = radius;
    visuals.widgets.open.expansion = 0.0;

    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(12.0, 10.0);
        style.spacing.button_padding = Vec2::new(14.0, 7.0);
        style.spacing.window_margin = Margin::same(0);
        style.spacing.menu_margin = Margin::symmetric(8, 8);
        style.spacing.indent = 18.0;
        style.spacing.slider_width = 220.0;
        style.spacing.combo_width = 220.0;
        style.spacing.interact_size.y = 28.0;
        style.spacing.icon_width = 16.0;
        style.spacing.icon_spacing = 8.0;
        style.animation_time = 0.18;
    });
}

// ============================================================================
// Helpers — composable visual pieces used by main.rs
// ============================================================================

/// 2x2 tetradic glyph used as the brand mark — four flush quadrants in the
/// palette colors, contained in a single rounded square. Reads as a logo at
/// any size, also as a metaphor for DICOM whole-slide tile quadrants.
pub fn brand_mark(ui: &mut Ui, side: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let half = side / 2.0;
    let cells = [
        (Pos2::new(rect.min.x, rect.min.y), SAND),
        (Pos2::new(rect.min.x + half, rect.min.y), SAGE),
        (Pos2::new(rect.min.x, rect.min.y + half), MAUVE),
        (Pos2::new(rect.min.x + half, rect.min.y + half), STEEL),
    ];
    for (origin, color) in cells {
        let cell_rect = Rect::from_min_size(origin, Vec2::splat(half));
        painter.rect_filled(cell_rect, CornerRadius::ZERO, color);
    }
    response
}

/// A pill-shaped status badge — colored dot + tracked label.
pub fn status_pill(ui: &mut Ui, label: &str, color: Color32, deep: Color32) {
    let frame = egui::Frame::new()
        .fill(PAPER_SUNKEN)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(12, 5));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            // colored dot
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.circle_filled(rect.center(), 5.0, color);
            painter.circle_stroke(
                rect.center(),
                5.0,
                Stroke::new(0.8, deep.gamma_multiply(0.55)),
            );
            ui.label(
                RichText::new(label)
                    .color(INK_MUTED)
                    .family(body_family())
                    .size(13.0),
            );
        });
    });
}

/// Section card — pure-white surface that floats on the canvas via a very
/// soft shadow. No border, no decorative bar. Identity comes from a small
/// tetradic dot before the title; everything else is whitespace and type.
pub fn card<R>(
    ui: &mut Ui,
    accent: Color32,
    _deep: Color32,
    title: &str,
    subtitle: Option<&str>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let frame = egui::Frame::new()
        .fill(PAPER_SOFT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(14))
        .shadow(Shadow {
            offset: [0, 2],
            blur: 14,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(15, 17, 26, 12),
        })
        .inner_margin(Margin {
            left: 24,
            right: 24,
            top: 20,
            bottom: 20,
        });
    let inner = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            // Small filled circle in the section's tetradic color — quiet
            // identifier sitting next to the title.
            let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot_rect.center(), 5.0, accent);
            ui.label(
                RichText::new(title)
                    .family(display_family())
                    .size(20.0)
                    .color(INK),
            );
            if let Some(sub) = subtitle {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(sub)
                        .family(body_family())
                        .size(13.0)
                        .color(INK_FAINT),
                );
            }
        });
        ui.add_space(14.0);
        add_contents(ui)
    });
    inner.inner
}

/// Field label — readable serif body, dark enough to feel like type, not
/// chrome. Used to label adjacent inputs.
pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .family(body_family())
            .size(14.5)
            .color(INK_MUTED),
    );
}

/// Render a monospace path value inside a borderless sunken pill — the gray
/// fill alone is enough to read as "data", no outline needed.
pub fn path_value(ui: &mut Ui, text: &str, placeholder: bool) {
    let color = if placeholder { INK_FAINT } else { INK };
    egui::Frame::new()
        .fill(PAPER_SUNKEN)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(text)
                    .family(mono_family())
                    .size(14.0)
                    .color(color),
            );
        });
}

/// A loud primary action button — large hit-area, steel-ink fill when armed,
/// muted paper-ink "awaiting" treatment when disabled (filled, not hollow,
/// so it still has weight). One per screen.
pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let (fill, stroke, text_color) = if enabled {
        (PRIMARY, Stroke::new(1.0, PRIMARY), PRIMARY_TEXT_ON)
    } else {
        (PAPER_SUNKEN, Stroke::new(1.0, RULE), INK_MUTED)
    };
    let text = WidgetText::from(
        RichText::new(label)
            .family(display_family())
            .size(18.0)
            .color(text_color),
    );
    let button = egui::Button::new(text)
        .min_size(Vec2::new(180.0, 44.0))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8));
    ui.add_enabled(enabled, button)
}

/// Secondary button — borderless ghost. Transparent at rest, lights up with
/// a soft fill on hover via the global widget visuals.
pub fn secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let text = WidgetText::from(
        RichText::new(label)
            .family(body_family())
            .size(14.5)
            .color(INK_MUTED),
    );
    let button = egui::Button::new(text)
        .min_size(Vec2::new(0.0, 36.0))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8));
    ui.add_enabled(enabled, button)
}
