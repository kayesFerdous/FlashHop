//! The desktop look: palette, Inter weights, and the few custom widgets (pills, round icon
//! buttons, the ticket, tick progress, the top glow, vector icons, the window icon).
//! android/src/app/akhripasta/Ui.kt mirrors the same tokens and shapes; change both together.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Mesh, Painter, Rect,
    Response, RichText, Role, Sense, Shape, Stroke, StrokeKind, TextFormat, Ui, Vec2, WidgetInfo,
    epaint::text::{FontTweak, LayoutJob, VariationCoords},
    pos2, vec2,
};
use std::{f32::consts::TAU, sync::Arc};

/// Lavender-grey ground.
pub const BG: Color32 = Color32::from_rgb(0xE9, 0xEB, 0xF5);
pub const CARD: Color32 = Color32::WHITE;
/// Controls sitting on a white card.
pub const RAISED: Color32 = Color32::from_rgb(0xEE, 0xF0, 0xF7);
const RAISED_HOVER: Color32 = Color32::from_rgb(0xE2, 0xE5, 0xF0);
pub const LINE: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xF0);
pub const INK: Color32 = Color32::from_rgb(0x0E, 0x0F, 0x14);
pub const MUTED: Color32 = Color32::from_rgb(0x58, 0x5D, 0x70);
/// The sending block, progress, focus, selection.
pub const COBALT: Color32 = Color32::from_rgb(0x2F, 0x55, 0xE4);
/// Go: send, the "+" on the cobalt block.
pub const LIME: Color32 = Color32::from_rgb(0xD6, 0xF5, 0x5B);
pub const WHITE: Color32 = Color32::WHITE;
/// Secondary text on cobalt.
pub const ON_COBALT_MUTED: Color32 = Color32::from_rgb(0xDC, 0xE3, 0xFD);
pub const GREEN: Color32 = Color32::from_rgb(0x15, 0x8A, 0x55);
pub const AMBER: Color32 = Color32::from_rgb(0xA8, 0x6A, 0x0A);
pub const RED: Color32 = Color32::from_rgb(0xD6, 0x33, 0x3A);
const PERFORATION: Color32 = Color32::from_rgb(0xB9, 0xC7, 0xF6);

const INTER: &[u8] = include_bytes!("../assets/Inter.ttf");

/// Inter is variable: each weight is the same bytes with different axis values.
#[derive(Clone, Copy)]
pub enum W {
    Regular,
    Medium,
    Semibold,
    /// Headline: optical size 32, weight 640.
    Display,
    /// Large numerals.
    Light,
}

impl W {
    const ALL: [W; 5] = [W::Regular, W::Medium, W::Semibold, W::Display, W::Light];
    fn key(self) -> &'static str {
        match self {
            W::Regular => "regular",
            W::Medium => "medium",
            W::Semibold => "semibold",
            W::Display => "display",
            W::Light => "light",
        }
    }
    fn axes(self) -> [(&'static [u8; 4], f32); 2] {
        let (wght, opsz) = match self {
            W::Regular => (400.0, 14.0),
            W::Medium => (500.0, 14.0),
            W::Semibold => (600.0, 14.0),
            W::Display => (640.0, 32.0),
            W::Light => (330.0, 32.0),
        };
        [(b"wght", wght), (b"opsz", opsz)]
    }
}

pub fn font(size: f32, w: W) -> FontId {
    FontId::new(size, FontFamily::Name(w.key().into()))
}

pub fn text(s: impl Into<String>, size: f32, w: W, color: Color32) -> RichText {
    RichText::new(s).font(font(size, w)).color(color)
}

/// One-line path that drops characters from the start ("…/AkhriPasta"), so the folder name stays
/// visible. The full path shows on hover.
pub fn path_label(ui: &mut Ui, path: &str, size: f32, w: W, color: Color32) -> Response {
    let fits = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font(size, w), color).size().x <= ui.available_width();
    let mut shown = path.to_owned();
    let mut chars = path.char_indices().skip(1);
    while !fits(&shown) {
        let Some((i, _)) = chars.next() else { break };
        shown = format!("…{}", &path[i..]);
    }
    ui.add(egui::Label::new(text(shown, size, w, color)).truncate()).on_hover_text(path)
}

pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::empty();
    for w in W::ALL {
        let tweak = FontTweak { coords: VariationCoords::new(w.axes()), ..Default::default() };
        fonts.font_data.insert(w.key().into(), Arc::new(FontData::from_static(INTER).tweak(tweak)));
        fonts.families.insert(FontFamily::Name(w.key().into()), vec![w.key().into()]);
    }
    fonts.families.insert(FontFamily::Proportional, vec![W::Regular.key().into()]);
    fonts.families.insert(FontFamily::Monospace, vec![W::Regular.key().into()]);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::ThemePreference::Light);
    ctx.style_mut_of(egui::Theme::Light, |s| {
        use egui::TextStyle::*;
        s.text_styles = [
            (Small, font(12.0, W::Regular)),
            (Body, font(14.0, W::Regular)),
            (Button, font(14.0, W::Medium)),
            (Heading, font(20.0, W::Semibold)),
            (Monospace, font(13.0, W::Regular)),
        ]
        .into();
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(14.0, 8.0);
        s.spacing.interact_size.y = 36.0;
        s.spacing.scroll = egui::style::ScrollStyle::floating();

        let v = &mut s.visuals;
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(12);
        v.extreme_bg_color = RAISED;
        v.text_edit_bg_color = Some(RAISED);
        v.faint_bg_color = RAISED;
        v.hyperlink_color = COBALT;
        v.error_fg_color = RED;
        v.warn_fg_color = AMBER;
        v.selection.bg_fill = COBALT.gamma_multiply(0.25);
        v.selection.stroke = Stroke::new(1.5, COBALT);
        v.text_cursor.stroke = Stroke::new(2.0, COBALT);
        let w = &mut v.widgets;
        w.noninteractive.fg_stroke.color = INK;
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        for (wv, fill) in [(&mut w.inactive, RAISED), (&mut w.hovered, RAISED_HOVER), (&mut w.active, RAISED_HOVER), (&mut w.open, RAISED)] {
            wv.bg_fill = fill;
            wv.weak_bg_fill = fill;
            wv.bg_stroke = Stroke::NONE;
            wv.fg_stroke.color = INK;
            wv.corner_radius = CornerRadius::same(12);
            wv.expansion = 0.0;
        }
    });
}

/// White section card.
pub fn card() -> Frame {
    Frame::new().fill(CARD).corner_radius(CornerRadius::same(24)).inner_margin(Margin::symmetric(16, 14))
}

/// The cobalt block that holds everything about sending.
pub fn dock(hot: bool) -> Frame {
    let fill = if hot { Color32::from_rgb(0x41, 0x66, 0xF5) } else { COBALT };
    Frame::new().fill(fill).corner_radius(CornerRadius::same(28)).inner_margin(Margin::same(14))
}

pub fn hairline(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, LINE);
}

/// Two-tone headline: the state in ink, the next step in grey.
pub fn headline(ui: &mut Ui, strong: &str, rest: &str) {
    let mut job = LayoutJob::default();
    for (s, c) in [(strong, INK), (rest, MUTED)] {
        let mut f = TextFormat::simple(font(28.0, W::Display), c);
        f.extra_letter_spacing = -0.7;
        job.append(s, 0.0, f);
    }
    job.wrap.max_width = ui.available_width();
    ui.label(job);
}

/// Large light numeral followed by a small unit, e.g. "28" "%".
pub fn numeral(ui: &mut Ui, value: &str, unit: &str) {
    let mut job = LayoutJob::default();
    job.append(value, 0.0, TextFormat::simple(font(32.0, W::Light), INK));
    job.append(unit, 2.0, TextFormat::simple(font(15.0, W::Regular), MUTED));
    ui.label(job);
}

fn focus_ring(p: &Painter, r: &Response, rect: Rect, radius: f32) {
    if r.has_focus() {
        p.rect_stroke(rect.expand(3.0), CornerRadius::same((radius + 3.0) as u8), Stroke::new(2.0, COBALT), StrokeKind::Outside);
    }
}

/// Button surfaces. Each names the surface it sits on or the job it does.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    /// Black: the main action on white or lavender.
    Ink,
    /// Light grey on a white card.
    Card,
    /// White on the lavender ground.
    Ground,
    /// Translucent white on cobalt.
    Glass,
    /// Lime: send / add.
    Lime,
}

impl Tone {
    /// (fill, hover fill, foreground)
    fn colors(self) -> (Color32, Color32, Color32) {
        match self {
            Tone::Ink => (INK, Color32::from_rgb(0x2A, 0x2C, 0x36), WHITE),
            Tone::Card => (RAISED, RAISED_HOVER, INK),
            Tone::Ground => (WHITE, Color32::from_rgb(0xF6, 0xF7, 0xFC), INK),
            Tone::Glass => (Color32::from_white_alpha(32), Color32::from_white_alpha(56), WHITE),
            Tone::Lime => (LIME, Color32::from_rgb(0xC8, 0xEA, 0x45), INK),
        }
    }
}

const PILL_H: f32 = 38.0;

fn pill_galley(ui: &Ui, s: &str) -> Arc<egui::Galley> {
    ui.painter().layout_no_wrap(s.to_owned(), font(13.5, W::Semibold), Color32::PLACEHOLDER)
}

pub fn pill_width(ui: &Ui, s: &str, icon: bool) -> f32 {
    pill_galley(ui, s).size().x + 34.0 + if icon { 22.0 } else { 0.0 }
}

pub fn pill(ui: &mut Ui, s: &str, icon: Option<Icon>, tone: Tone, enabled: bool) -> Response {
    let galley = pill_galley(ui, s);
    let w = pill_width(ui, s, icon.is_some());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, PILL_H), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, s));
    if ui.is_rect_visible(rect) {
        let (fill, hover, mut fg) = tone.colors();
        let t = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.12);
        let mut fill = fill.lerp_to_gamma(hover, t);
        if !enabled {
            fill = fill.gamma_multiply(0.35);
            fg = fg.gamma_multiply(0.45);
        } else if resp.is_pointer_button_down_on() {
            fill = fill.gamma_multiply(0.88);
        }
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(19), fill);
        let mut x = rect.left() + 17.0;
        if let Some(ic) = icon {
            paint(p, ic, Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(16.0)), fg, fill);
            x += 22.0;
        }
        p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
        focus_ring(p, &resp, rect, 19.0);
    }
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Round icon button.
pub fn round(ui: &mut Ui, icon: Icon, tip: &str, size: f32, tone: Tone, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, tip));
    if ui.is_rect_visible(rect) {
        let (base, hover, mut fg) = tone.colors();
        let t = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.12);
        let mut fill = base.lerp_to_gamma(hover, t);
        if !enabled {
            (fill, fg) = (RAISED, MUTED.gamma_multiply(0.6));
        } else if resp.is_pointer_button_down_on() {
            fill = fill.gamma_multiply(0.88);
        }
        let p = ui.painter();
        p.circle_filled(rect.center(), size / 2.0, fill);
        paint(p, icon, Rect::from_center_size(rect.center(), Vec2::splat((size * 0.45).min(20.0))), fg, fill);
        focus_ring(p, &resp, rect, size / 2.0);
    }
    let resp = resp.on_hover_text(tip);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Non-interactive icon in a filled circle.
pub fn badge(ui: &mut Ui, icon: Icon, fg: Color32, bg: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.circle_filled(rect.center(), size / 2.0, bg);
    paint(p, icon, Rect::from_center_size(rect.center(), Vec2::splat(size * 0.44)), fg, bg);
}

/// Bare icon, for list rows.
pub fn glyph(ui: &mut Ui, icon: Icon, size: f32, color: Color32, bg: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint(ui.painter(), icon, rect, color, bg);
}

/// Initial of a device name in a circle.
pub fn avatar(ui: &mut Ui, name: &str, size: f32, bg: Color32, fg: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.circle_filled(rect.center(), size / 2.0, bg);
    let ch = name.chars().find(|c| c.is_alphanumeric()).map_or("?".into(), |c| c.to_uppercase().to_string());
    p.text(rect.center(), Align2::CENTER_CENTER, ch, font(size * 0.42, W::Semibold), fg);
}

/// Text that acts as a quiet button ("Clear finished").
pub fn link(ui: &mut Ui, s: &str) -> Response {
    let resp = ui.add(egui::Label::new(text(s, 13.0, W::Medium, MUTED)).sense(Sense::click()));
    if resp.hovered() {
        ui.painter().text(resp.rect.left_top(), Align2::LEFT_TOP, s, font(13.0, W::Medium), INK);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// White ticket with side notches and a dashed perforation between the `top` and `stub` parts.
/// The notches are painted in `under`, the colour it sits on; `None` means the glowing ground.
pub fn ticket<R>(ui: &mut Ui, under: Option<Color32>, top: impl FnOnce(&mut Ui), stub: impl FnOnce(&mut Ui) -> R) -> R {
    let mut split = 0.0;
    let inner = Frame::new().fill(WHITE).corner_radius(CornerRadius::same(22)).inner_margin(Margin::symmetric(18, 16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        top(ui);
        ui.add_space(10.0);
        split = ui.cursor().top();
        ui.add_space(14.0);
        stub(ui)
    });
    let r = inner.response.rect;
    let p = ui.painter();
    for x in [r.left(), r.right()] {
        let at = pos2(x, split);
        p.circle_filled(at, 11.0, under.unwrap_or_else(|| ground(ui.ctx(), at)));
    }
    p.extend(Shape::dashed_line(&[pos2(r.left() + 18.0, split), pos2(r.right() - 18.0, split)], Stroke::new(1.5, PERFORATION), 6.0, 5.0));
    inner.inner
}

/// Progress as a row of thin ticks: lit in cobalt, the leading tick in ink.
pub fn ticks(ui: &mut Ui, fraction: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let pitch = 6.0;
    let n = ((rect.width() + 3.0) / pitch).floor().max(1.0) as usize;
    let lit = (fraction.clamp(0.0, 1.0) * n as f32).round() as usize;
    let p = ui.painter();
    for i in 0..n {
        let x = rect.left() + i as f32 * pitch;
        let c = if i + 1 == lit { INK } else if i < lit { COBALT } else { RAISED_HOVER };
        p.rect_filled(Rect::from_min_max(pos2(x, rect.top()), pos2(x + 3.0, rect.bottom())), CornerRadius::same(2), c);
    }
}

fn glow_shape(screen: Rect) -> (egui::Pos2, Vec2) {
    (pos2(screen.left() + screen.width() * 0.2, screen.top() - 60.0), vec2(screen.width() * 1.1, (screen.height() * 0.5).max(300.0)))
}

/// The ground colour at `at`: lavender plus the glow, so cut-outs can match what is behind them.
fn ground(ctx: &egui::Context, at: egui::Pos2) -> Color32 {
    let (screen, strength) = ctx.data(|d| d.get_temp::<(Rect, f32)>(egui::Id::unique("glow-params"))).unwrap_or((Rect::ZERO, 0.0));
    let (c, r) = glow_shape(screen);
    let t = (((at - c) / r).length()).min(1.0);
    BG.lerp_to_gamma(COBALT, strength * (1.0 - t).powi(2))
}

/// Soft cobalt wash from the top-left of the window; `strength` 0..1 follows activity.
pub fn glow(ctx: &egui::Context, screen: Rect, strength: f32) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("glow-params"), (screen, strength)));
    let p = ctx.layer_painter(egui::LayerId::background());
    let (c, r) = glow_shape(screen);
    let (rings, seg) = (20u32, 72u32);
    let col = |t: f32| COBALT.gamma_multiply(strength * (1.0 - t).powi(2));
    let mut m = Mesh::default();
    m.colored_vertex(c, col(0.0));
    for i in 1..=rings {
        let t = i as f32 / rings as f32;
        for k in 0..seg {
            let a = TAU * k as f32 / seg as f32;
            m.colored_vertex(c + vec2(a.cos() * r.x, a.sin() * r.y) * t, col(t));
        }
    }
    for k in 0..seg {
        m.add_triangle(0, 1 + k, 1 + (k + 1) % seg);
    }
    for i in 1..rings {
        let (a, b) = (1 + (i - 1) * seg, 1 + i * seg);
        for k in 0..seg {
            let k1 = (k + 1) % seg;
            m.add_triangle(a + k, b + k, b + k1);
            m.add_triangle(a + k, b + k1, a + k1);
        }
    }
    p.add(Shape::mesh(m));
}

#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    Close,
    Check,
    Up,
    Down,
    UpRight,
    Left,
    Folder,
    File,
    Text,
    Paste,
    Sliders,
}

/// Icons on a 24-unit grid, one stroke weight. `bg` fills knobs and cut-outs.
pub fn paint(p: &Painter, icon: Icon, rect: Rect, color: Color32, bg: Color32) {
    let s = rect.width() / 24.0;
    let at = |x: f32, y: f32| rect.min + vec2(x, y) * s;
    let st = Stroke::new((1.9 * s).max(1.3), color);
    let pts = |v: &[(f32, f32)]| v.iter().map(|&(x, y)| at(x, y)).collect::<Vec<_>>();
    let line = |v: &[(f32, f32)]| {
        p.add(Shape::line(pts(v), st));
    };
    let closed = |v: &[(f32, f32)]| {
        p.add(Shape::closed_line(pts(v), st));
    };
    match icon {
        Icon::Plus => {
            line(&[(12., 5.), (12., 19.)]);
            line(&[(5., 12.), (19., 12.)]);
        }
        Icon::Close => {
            line(&[(6.5, 6.5), (17.5, 17.5)]);
            line(&[(17.5, 6.5), (6.5, 17.5)]);
        }
        Icon::Check => line(&[(5., 12.5), (10., 17.5), (19., 7.)]),
        Icon::Up => {
            line(&[(12., 19.), (12., 5.)]);
            line(&[(6., 11.), (12., 5.), (18., 11.)]);
        }
        Icon::Down => {
            line(&[(12., 5.), (12., 19.)]);
            line(&[(6., 13.), (12., 19.), (18., 13.)]);
        }
        Icon::UpRight => {
            line(&[(7., 17.), (17., 7.)]);
            line(&[(8., 7.), (17., 7.), (17., 16.)]);
        }
        Icon::Left => {
            line(&[(19., 12.), (5., 12.)]);
            line(&[(11., 6.), (5., 12.), (11., 18.)]);
        }
        Icon::Folder => closed(&[(3., 6.), (9.5, 6.), (11.5, 8.5), (21., 8.5), (21., 19.), (3., 19.)]),
        Icon::File => {
            closed(&[(6., 3.), (14., 3.), (19., 8.), (19., 21.), (6., 21.)]);
            line(&[(14., 3.), (14., 8.), (19., 8.)]);
        }
        Icon::Text => {
            line(&[(5., 7.), (19., 7.)]);
            line(&[(5., 12.), (19., 12.)]);
            line(&[(5., 17.), (13., 17.)]);
        }
        Icon::Paste => {
            let r = CornerRadius::same((2.0 * s) as u8);
            p.rect_stroke(Rect::from_min_max(at(5., 5.), at(19., 21.)), r, st, StrokeKind::Middle);
            let clip = Rect::from_min_max(at(8.5, 3.), at(15.5, 7.));
            p.rect_filled(clip, r, bg);
            p.rect_stroke(clip, r, st, StrokeKind::Middle);
        }
        Icon::Sliders => {
            line(&[(4., 8.), (20., 8.)]);
            line(&[(4., 16.), (20., 16.)]);
            for (x, y) in [(15., 8.), (9., 16.)] {
                p.circle_filled(at(x, y), 2.8 * s, bg);
                p.circle_stroke(at(x, y), 2.8 * s, st);
            }
        }
    }
}

/// Window icon, rasterized from the same geometry as android/res/drawable/ic_launcher_fg.xml
/// (108-unit adaptive-icon canvas): a white ticket with an up-right arrow on a cobalt tile.
/// macOS shows icons unmasked, so there the tile gets Apple's grid margin (824 of 1024 px);
/// eframe also puts this icon in the Dock, so it must match the .app bundle icon.
pub fn app_icon(n: u32) -> egui::IconData {
    let canvas: f32 = if cfg!(target_os = "macos") { 100.0 * 1024.0 / 824.0 } else { 108.0 }; // tile is 100 units
    let (unit, pad) = (canvas / n as f32, (canvas - 108.0) / 2.0); // canvas units per pixel, margin
    let cover = |sd: f32| (0.5 - sd / unit).clamp(0.0, 1.0);
    let rrect = |x: f32, y: f32, (x0, y0, x1, y1): (f32, f32, f32, f32), r: f32| {
        let (cx, cy, hx, hy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0 - r, (y1 - y0) / 2.0 - r);
        let (dx, dy) = ((x - cx).abs() - hx, (y - cy).abs() - hy);
        dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r
    };
    let seg = |x: f32, y: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32), w: f32| {
        let (px, py, ex, ey) = (x - ax, y - ay, bx - ax, by - ay);
        let h = ((px * ex + py * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
        (px - ex * h).hypot(py - ey * h) - w / 2.0
    };
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for j in 0..n {
        for i in 0..n {
            let (x, y) = ((i as f32 + 0.5) * unit - pad, (j as f32 + 0.5) * unit - pad);
            let tile_c = Color32::from_rgb(0x4A, 0x6E, 0xF5).lerp_to_gamma(Color32::from_rgb(0x24, 0x44, 0xCC), y / 108.0);
            let tile = cover(rrect(x, y, (4.0, 4.0, 104.0, 104.0), 24.0));
            let notch = ((x - 64.0).hypot(y - 36.0) - 5.5).min((x - 64.0).hypot(y - 72.0) - 5.5);
            let ticket = cover(rrect(x, y, (25.0, 36.0, 83.0, 72.0), 7.0).max(-notch));
            let dash = (0..3).map(|k| rrect(x, y, (63.0, 44.0 + k as f32 * 8.0, 65.0, 48.0 + k as f32 * 8.0), 0.5)).fold(f32::MAX, f32::min);
            let arrow = seg(x, y, (37.0, 61.0), (51.0, 47.0), 4.5).min(seg(x, y, (43.0, 47.0), (51.0, 47.0), 4.5)).min(seg(x, y, (51.0, 47.0), (51.0, 55.0), 4.5));
            let cut = cover(dash.min(arrow));
            let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let px = |c: u8| mix(mix(c as f32, 255.0, ticket), c as f32, ticket * cut) as u8;
            rgba.extend([px(tile_c.r()), px(tile_c.g()), px(tile_c.b()), (tile * 255.0) as u8]);
        }
    }
    egui::IconData { rgba, width: n, height: n }
}
