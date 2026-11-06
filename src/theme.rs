//! Look & feel: a dark graphite theme in the spirit of macOS pro apps, plus small custom widgets.

use egui::epaint::{Mesh, Shadow, Vertex, WHITE_UV};
use egui::{
    Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Response, RichText, Rounding, Sense, Stroke,
    TextStyle, Ui, Vec2,
};
use std::sync::Arc;

pub const WINDOW: Color32 = Color32::from_rgb(28, 28, 30);
pub const TOOLBAR: Color32 = Color32::from_rgb(42, 42, 45);
pub const SIDEBAR: Color32 = Color32::from_rgb(36, 36, 39);
pub const VIEWER: Color32 = Color32::from_rgb(20, 20, 22);
pub const CARD: Color32 = Color32::from_rgb(48, 48, 52);
pub const SHEET: Color32 = Color32::from_rgb(44, 44, 48);
pub const SEPARATOR: Color32 = Color32::from_rgb(58, 58, 62);
pub const CONTROL: Color32 = Color32::from_rgb(64, 64, 68);
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(78, 78, 83);
pub const TRACK: Color32 = Color32::from_rgb(38, 38, 41);
pub const TEXT: Color32 = Color32::from_rgb(236, 236, 240);
pub const TEXT_2: Color32 = Color32::from_rgb(160, 160, 166);
pub const TEXT_3: Color32 = Color32::from_rgb(112, 112, 118);
pub const ACCENT: Color32 = Color32::from_rgb(10, 132, 255);
pub const RED: Color32 = Color32::from_rgb(255, 69, 58);
pub const YELLOW: Color32 = Color32::from_rgb(255, 214, 10);
pub const GREEN: Color32 = Color32::from_rgb(48, 209, 88);
pub const PURPLE: Color32 = Color32::from_rgb(158, 92, 230);
pub const CLIP: Color32 = Color32::from_rgb(56, 96, 178);

pub const MEDIUM: &str = "medium";

/// The app's name as people see it; the dot is red, like a camera's recording light.
pub const APP_NAME: &str = "rec·ording";

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

fn read_first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let base = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    let regular = read_first(&[
        "/usr/share/fonts/inter/Inter-Regular.otf",
        "/usr/share/fonts/TTF/Inter-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/cantarell/Cantarell-VF.otf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
    ]);
    let medium_font = read_first(&[
        "/usr/share/fonts/inter/Inter-Medium.otf",
        "/usr/share/fonts/TTF/Inter-Medium.ttf",
        "/usr/share/fonts/noto/NotoSans-Medium.ttf",
        "/usr/share/fonts/TTF/NotoSans-Medium.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Medium.ttf",
        "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
        "C:\\Windows\\Fonts\\seguisb.ttf",
    ]);

    let mut medium_family = base.clone();
    if let Some(bytes) = regular {
        fonts.font_data.insert("ui".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
        medium_family.insert(0, "ui".into());
    }
    if let Some(bytes) = medium_font {
        fonts.font_data.insert("ui-medium".into(), Arc::new(FontData::from_owned(bytes)));
        medium_family.insert(0, "ui-medium".into());
    }
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), medium_family);
    ctx.set_fonts(fonts);

    // Always dark: following a light system theme (the Windows default) would swap in egui's
    // plain light style.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, regular_id(11.0)),
            (TextStyle::Body, regular_id(13.0)),
            (TextStyle::Button, regular_id(13.0)),
            (TextStyle::Heading, medium(17.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 8.0);
        s.button_padding = Vec2::new(12.0, 5.0);
        s.interact_size = Vec2::new(36.0, 26.0);
        s.slider_width = 150.0;
        s.slider_rail_height = 4.0;
        s.window_margin = egui::Margin::same(20.0);
        s.combo_height = 300.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = WINDOW;
        v.window_fill = SHEET;
        v.extreme_bg_color = Color32::from_rgb(30, 30, 33);
        v.faint_bg_color = CARD;
        v.window_rounding = Rounding::same(14.0);
        v.menu_rounding = Rounding::same(10.0);
        v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(72, 72, 76));
        v.window_shadow = Shadow { offset: Vec2::new(0.0, 16.0), blur: 48.0, spread: 0.0, color: Color32::from_black_alpha(150) };
        v.popup_shadow = Shadow { offset: Vec2::new(0.0, 6.0), blur: 18.0, spread: 0.0, color: Color32::from_black_alpha(110) };
        v.selection.bg_fill = ACCENT.linear_multiply(0.6);
        v.selection.stroke = Stroke::new(1.0_f32, TEXT);
        v.hyperlink_color = ACCENT;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;

        let r = Rounding::same(7.0);
        let w = &mut v.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, SEPARATOR);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_2);
        w.noninteractive.rounding = r;
        for (ws, fill) in [
            (&mut w.inactive, CONTROL),
            (&mut w.hovered, CONTROL_HOVER),
            (&mut w.active, Color32::from_rgb(92, 92, 98)),
            (&mut w.open, CONTROL_HOVER),
        ] {
            ws.bg_fill = fill;
            ws.weak_bg_fill = fill;
            ws.bg_stroke = Stroke::NONE;
            ws.fg_stroke = Stroke::new(1.5_f32, TEXT);
            ws.rounding = r;
            ws.expansion = 0.0;
        }
    });
}

fn regular_id(size: f32) -> FontId {
    FontId::proportional(size)
}

// ---------- Widgets ----------

/// Blue call-to-action button.
pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    tinted_button(ui, text, ACCENT, enabled)
}

pub fn tinted_button(ui: &mut Ui, text: &str, fill: Color32, enabled: bool) -> Response {
    let btn = egui::Button::new(RichText::new(text).font(medium(13.0)).color(Color32::WHITE))
        .fill(if enabled { fill } else { fill.linear_multiply(0.35) })
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(0.0, 28.0));
    ui.add_enabled(enabled, btn)
}

pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).color(TEXT)).min_size(Vec2::new(0.0, 28.0)))
}

/// Rounded pill button for quick presets.
pub fn chip(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let btn = egui::Button::new(RichText::new(text).font(regular(12.5)).color(if selected { Color32::WHITE } else { TEXT }))
        .fill(if selected { ACCENT } else { Color32::from_rgb(62, 62, 67) })
        .rounding(Rounding::same(12.0))
        .min_size(Vec2::new(0.0, 24.0));
    ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let bg = lerp_color(Color32::from_rgb(84, 84, 90), ACCENT, t);
    ui.painter().rect_filled(rect, Rounding::same(10.0), bg);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    ui.painter().circle_filled(Pos2::new(x, rect.center().y + 0.5), 8.5, Color32::from_black_alpha(40));
    ui.painter().circle_filled(Pos2::new(x, rect.center().y), 8.0, Color32::WHITE);
    resp
}

/// macOS-style segmented control. Returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)], width: f32) -> bool {
    let h = 26.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, h), Sense::hover());
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, Rounding::same(7.0), Color32::from_rgb(54, 54, 58));
    // Equal segments when every label fits; otherwise each gets its label's width plus an
    // equal share of what is left, so long labels never run into their neighbours.
    let n = options.len().max(1) as f32;
    let label_w: Vec<f32> = options.iter().map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), medium(12.5), TEXT).size().x + 12.0).collect();
    let equal = rect.width() / n;
    let widths: Vec<f32> = if label_w.iter().all(|w| *w <= equal) {
        vec![equal; options.len()]
    } else {
        let spare = (rect.width() - label_w.iter().sum::<f32>()) / n;
        label_w.iter().map(|w| w + spare).collect()
    };
    let mut changed = false;
    let mut x = rect.left();
    for (i, (opt, label)) in options.iter().enumerate() {
        let seg_w = widths[i];
        let r = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(seg_w, h));
        x += seg_w;
        let resp = ui.interact(r, ui.id().with(("seg", i, label)), Sense::click());
        let selected = *value == *opt;
        if selected {
            let pill = r.shrink(2.0);
            painter.rect_filled(pill.translate(Vec2::new(0.0, 0.5)), Rounding::same(5.5), Color32::from_black_alpha(50));
            painter.rect_filled(pill, Rounding::same(5.5), Color32::from_rgb(104, 104, 110));
        } else if resp.hovered() {
            painter.rect_filled(r.shrink(2.0), Rounding::same(5.5), Color32::from_rgb(66, 66, 71));
        } else if i > 0 && options.get(i - 1).map(|(o, _)| *o != *value).unwrap_or(true) {
            painter.line_segment(
                [Pos2::new(r.left(), r.top() + 7.0), Pos2::new(r.left(), r.bottom() - 7.0)],
                Stroke::new(1.0_f32, Color32::from_rgb(78, 78, 84)),
            );
        }
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            if selected { medium(12.5) } else { regular(12.5) },
            if selected { Color32::WHITE } else { TEXT_2 },
        );
        if resp.clicked() && !selected {
            *value = *opt;
            changed = true;
        }
    }
    changed
}

/// A titled group box, like the panes of System Settings.
pub fn section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(4.0);
    ui.label(RichText::new(title).font(medium(12.5)).color(TEXT_2));
    ui.add_space(-2.0);
    egui::Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A labelled slider row filling the available width.
pub fn slider_row(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, fmt: impl Fn(f32) -> String) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        fixed_label(ui, 78.0, RichText::new(label).color(TEXT), egui::Align::Min);
        ui.spacing_mut().interact_size.y = 18.0;
        let value_w = 40.0;
        ui.spacing_mut().slider_width = (ui.available_width() - value_w - 8.0).max(40.0);
        changed = ui.add(egui::Slider::new(value, range).show_value(false)).changed();
        ui.label(RichText::new(fmt(*value)).color(TEXT_2).font(regular(12.0)));
    });
    changed
}

/// A label in a fixed-width column, aligned left (`Min`) or right (`Max`).
pub fn fixed_label(ui: &mut Ui, width: f32, text: RichText, align: egui::Align) {
    let layout = if align == egui::Align::Max {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.allocate_ui_with_layout(Vec2::new(width, 24.0), layout, |ui| {
        ui.set_min_width(width);
        ui.add(egui::Label::new(text).truncate());
    });
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Points of a rounded rectangle outline (convex, clockwise).
pub fn rounded_rect_points(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let corners = [
        (Pos2::new(rect.right() - r, rect.top() + r), -90.0f32),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 90.0),
        (Pos2::new(rect.left() + r, rect.top() + r), 180.0),
    ];
    let mut pts = Vec::new();
    for (c, start) in corners {
        for i in 0..=6 {
            let a = (start + 15.0 * i as f32).to_radians();
            pts.push(Pos2::new(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

/// Fills a convex outline with a linear gradient from `a` to `b` along `angle_deg`.
pub fn fill_gradient(painter: &egui::Painter, pts: &[Pos2], a: Color32, b: Color32, angle_deg: f32) {
    if pts.len() < 3 {
        return;
    }
    let (dx, dy) = (angle_deg.to_radians().cos(), angle_deg.to_radians().sin());
    let proj: Vec<f32> = pts.iter().map(|p| p.x * dx + p.y * dy).collect();
    let (lo, hi) = proj.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    let span = (hi - lo).max(1e-3);
    let center = pts.iter().fold(Vec2::ZERO, |acc, p| acc + p.to_vec2()) / pts.len() as f32;
    let cproj = center.x * dx + center.y * dy;

    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex { pos: center.to_pos2(), uv: WHITE_UV, color: lerp_color(a, b, (cproj - lo) / span) });
    for (p, v) in pts.iter().zip(&proj) {
        mesh.vertices.push(Vertex { pos: *p, uv: WHITE_UV, color: lerp_color(a, b, (v - lo) / span) });
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(egui::Shape::mesh(mesh));
}

// ---------- Icons (painted, so they are crisp at any scale) ----------

pub fn icon_play(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let pts = vec![
        Pos2::new(c.x - s * 0.35, c.y - s * 0.5),
        Pos2::new(c.x + s * 0.55, c.y),
        Pos2::new(c.x - s * 0.35, c.y + s * 0.5),
    ];
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
}

pub fn icon_pause(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    for dx in [-0.25f32, 0.25] {
        let r = Rect::from_center_size(Pos2::new(c.x + dx * s, c.y), Vec2::new(s * 0.26, s));
        p.rect_filled(r, Rounding::same(1.5), col);
    }
}

pub fn icon_to_start(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    p.rect_filled(
        Rect::from_center_size(Pos2::new(c.x - s * 0.42, c.y), Vec2::new(s * 0.14, s * 0.8)),
        Rounding::same(1.0),
        col,
    );
    let pts = vec![
        Pos2::new(c.x + s * 0.45, c.y - s * 0.4),
        Pos2::new(c.x + s * 0.45, c.y + s * 0.4),
        Pos2::new(c.x - s * 0.3, c.y),
    ];
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
}

pub fn icon_phone(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let r = Rect::from_center_size(c, Vec2::new(s * 0.56, s));
    p.rect_stroke(r, Rounding::same(s * 0.12), Stroke::new(s * 0.07, col));
    p.circle_filled(Pos2::new(c.x, r.top() + s * 0.1), s * 0.035, col);
}

pub fn icon_import(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new(s * 0.08, col);
    p.line_segment([Pos2::new(c.x, c.y - s * 0.45), Pos2::new(c.x, c.y + s * 0.12)], st);
    p.line_segment([Pos2::new(c.x - s * 0.22, c.y - s * 0.1), Pos2::new(c.x, c.y + s * 0.13)], st);
    p.line_segment([Pos2::new(c.x + s * 0.22, c.y - s * 0.1), Pos2::new(c.x, c.y + s * 0.13)], st);
    let base = c.y + s * 0.42;
    p.line_segment([Pos2::new(c.x - s * 0.42, c.y + s * 0.15), Pos2::new(c.x - s * 0.42, base)], st);
    p.line_segment([Pos2::new(c.x - s * 0.42, base), Pos2::new(c.x + s * 0.42, base)], st);
    p.line_segment([Pos2::new(c.x + s * 0.42, base), Pos2::new(c.x + s * 0.42, c.y + s * 0.15)], st);
}

pub fn icon_check(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new(s * 0.1, col);
    let a = Pos2::new(c.x - s * 0.28, c.y + s * 0.02);
    let b = Pos2::new(c.x - s * 0.08, c.y + s * 0.22);
    let d = Pos2::new(c.x + s * 0.3, c.y - s * 0.2);
    p.line_segment([a, b], st);
    p.line_segment([b, d], st);
}

/// "rec·ording" with its red dot drawn as a recording light, for the toolbar.
pub fn wordmark(ui: &mut Ui, size: f32) -> Response {
    let font = medium(size);
    let rec = ui.painter().layout_no_wrap("rec".into(), font.clone(), TEXT);
    let ording = ui.painter().layout_no_wrap("ording".into(), font, TEXT);
    let dot_r = (size * 0.16).max(2.0);
    let space = size * 0.22;
    let (rec_w, h) = (rec.size().x, rec.size().y.max(ording.size().y));
    let w = rec_w + space + 2.0 * dot_r + space + ording.size().x;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter();
    p.galley(rect.left_top(), rec, TEXT);
    // The dot sits at the middle of the lowercase letters, with a faint glow.
    let c = Pos2::new(rect.left() + rec_w + space + dot_r, rect.top() + h * 0.58);
    p.circle_filled(c, dot_r + 1.5, RED.linear_multiply(0.25));
    p.circle_filled(c, dot_r, RED);
    p.galley(Pos2::new(c.x + dot_r + space, rect.top()), ording, TEXT);
    resp
}

/// `text` with every "·" of the app's name in red, for sentences that mention it.
pub fn with_name(text: &str, font: FontId, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let format = |c: Color32| egui::TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    let mut rest = text;
    while let Some(i) = rest.find(APP_NAME) {
        let dot = i + "rec".len();
        job.append(&rest[..dot], 0.0, format(color));
        job.append("·", 0.0, format(RED));
        rest = &rest[dot + "·".len()..];
    }
    job.append(rest, 0.0, format(color));
    job
}

// ---------- Tool icons ----------

/// The icons of the tool buttons (timeline, inspector, toolbar).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Icon {
    Zoom,
    Camera,
    Text,
    PhoneMove,
    Director,
    Split,
    Freeze,
    AddVideo,
    Swap,
    Undo,
    Redo,
    Trash,
    Pencil,
    Sidebar,
    Plus,
    Folder,
    Home,
    Image,
    Effect,
    Mic,
    Music,
}

fn polyline(p: &egui::Painter, pts: Vec<Pos2>, st: Stroke) {
    p.add(egui::Shape::line(pts, st));
}

/// Points of an arc (angles in degrees, y down).
fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = 14;
    (0..=n)
        .map(|i| {
            let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
            Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// An open arrow head at `tip`, pointing along `dir`.
fn arrow_head(p: &egui::Painter, tip: Pos2, dir: Vec2, size: f32, st: Stroke) {
    let d = dir.normalized();
    let n = Vec2::new(-d.y, d.x);
    polyline(p, vec![tip - d * size + n * size * 0.8, tip, tip - d * size - n * size * 0.8], st);
}

/// Paints `icon` centred on `c`, about `s` points across.
pub fn paint_icon(p: &egui::Painter, icon: Icon, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new((s * 0.09).max(1.4), col);
    let v = |x: f32, y: f32| c + Vec2::new(x * s, y * s);
    match icon {
        Icon::Zoom => {
            let o = v(-0.1, -0.1);
            p.circle_stroke(o, s * 0.32, st);
            p.line_segment([v(0.13, 0.13), v(0.44, 0.44)], Stroke::new(st.width * 1.4, col));
            p.line_segment([o - Vec2::new(s * 0.14, 0.0), o + Vec2::new(s * 0.14, 0.0)], st);
            p.line_segment([o - Vec2::new(0.0, s * 0.14), o + Vec2::new(0.0, s * 0.14)], st);
        }
        Icon::Camera => {
            let body = Rect::from_min_max(v(-0.46, -0.24), v(0.16, 0.24));
            p.rect_stroke(body, Rounding::same(s * 0.08), st);
            p.add(egui::Shape::closed_line(vec![v(0.2, -0.06), v(0.46, -0.22), v(0.46, 0.22), v(0.2, 0.06)], st));
            p.circle_filled(v(-0.3, -0.36), s * 0.08, col);
        }
        Icon::Text => {
            p.line_segment([v(-0.34, -0.36), v(0.34, -0.36)], Stroke::new(st.width * 1.2, col));
            p.line_segment([v(0.0, -0.36), v(0.0, 0.4)], Stroke::new(st.width * 1.2, col));
            p.line_segment([v(-0.14, 0.4), v(0.14, 0.4)], st);
        }
        Icon::PhoneMove => {
            let body = Rect::from_min_max(v(-0.4, -0.38), v(0.0, 0.38));
            p.rect_stroke(body, Rounding::same(s * 0.09), st);
            p.line_segment([v(-0.26, -0.27), v(-0.14, -0.27)], st);
            let pts = arc(v(-0.06, 0.0), s * 0.42, -50.0, 50.0);
            let (end, before) = (pts[pts.len() - 1], pts[pts.len() - 3]);
            polyline(p, pts, st);
            arrow_head(p, end, end - before, s * 0.14, st);
        }
        Icon::Director => {
            let body = Rect::from_min_max(v(-0.42, -0.1), v(0.42, 0.4));
            p.rect_stroke(body, Rounding::same(s * 0.05), st);
            p.add(egui::Shape::closed_line(vec![v(-0.42, -0.18), v(0.36, -0.42), v(0.42, -0.26), v(-0.36, -0.02)], st));
            for x in [-0.14, 0.12] {
                p.line_segment([v(x, -0.27), v(x + 0.1, -0.12)], st);
            }
        }
        Icon::Split => {
            p.line_segment([v(0.0, -0.48), v(0.0, 0.48)], st);
            p.rect_stroke(Rect::from_min_max(v(-0.46, -0.22), v(-0.12, 0.22)), Rounding::same(s * 0.05), st);
            p.rect_stroke(Rect::from_min_max(v(0.12, -0.22), v(0.46, 0.22)), Rounding::same(s * 0.05), st);
        }
        Icon::Freeze => {
            for k in 0..3 {
                let a = (k as f32 * 60.0 + 90.0).to_radians();
                let d = Vec2::new(a.cos(), a.sin()) * s * 0.44;
                p.line_segment([c - d, c + d], st);
                for e in [d, -d] {
                    let base = c + e * 0.6;
                    let n = Vec2::new(-e.y, e.x).normalized() * s * 0.13;
                    let out = e.normalized() * s * 0.13;
                    p.line_segment([base, base + out + n], st);
                    p.line_segment([base, base + out - n], st);
                }
            }
        }
        Icon::AddVideo => {
            let film = Rect::from_min_max(v(-0.46, -0.3), v(0.22, 0.3));
            p.rect_stroke(film, Rounding::same(s * 0.06), st);
            for y in [-0.16, 0.0, 0.16] {
                p.circle_filled(v(-0.36, y), s * 0.035, col);
                p.circle_filled(v(0.12, y), s * 0.035, col);
            }
            p.line_segment([v(0.3, 0.26), v(0.5, 0.26)], st);
            p.line_segment([v(0.4, 0.16), v(0.4, 0.36)], st);
        }
        Icon::Swap => {
            p.line_segment([v(-0.42, -0.16), v(0.4, -0.16)], st);
            arrow_head(p, v(0.42, -0.16), Vec2::X, s * 0.16, st);
            p.line_segment([v(0.42, 0.18), v(-0.4, 0.18)], st);
            arrow_head(p, v(-0.42, 0.18), -Vec2::X, s * 0.16, st);
        }
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Redo { -1.0 } else { 1.0 };
            let mut pts = arc(v(0.04 * flip, 0.08), s * 0.34, 200.0, 400.0);
            for q in &mut pts {
                q.x = c.x + (q.x - c.x) * flip;
            }
            let (tip, next) = (pts[0], pts[2]);
            polyline(p, pts, st);
            arrow_head(p, tip, tip - next, s * 0.16, st);
        }
        Icon::Trash => {
            p.line_segment([v(-0.4, -0.3), v(0.4, -0.3)], st);
            p.add(egui::Shape::line(vec![v(-0.13, -0.3), v(-0.13, -0.42), v(0.13, -0.42), v(0.13, -0.3)], st));
            p.add(egui::Shape::line(vec![v(-0.3, -0.2), v(-0.24, 0.44), v(0.24, 0.44), v(0.3, -0.2)], st));
            for x in [-0.08, 0.08] {
                p.line_segment([v(x, -0.1), v(x, 0.32)], Stroke::new(st.width * 0.8, col));
            }
        }
        Icon::Pencil => {
            let d = Vec2::new(1.0, -1.0).normalized();
            let n = Vec2::new(1.0, 1.0).normalized() * s * 0.12;
            let (tail, tip) = (v(0.42, -0.42), v(-0.42, 0.42));
            let neck = tip + d * s * 0.3;
            p.add(egui::Shape::closed_line(vec![tail + n, neck + n, tip, neck - n, tail - n], st));
            p.line_segment([tail - d * s * 0.2 + n, tail - d * s * 0.2 - n], st);
        }
        Icon::Sidebar => {
            let r = Rect::from_center_size(c, Vec2::new(s * 0.92, s * 0.72));
            p.rect_stroke(r, Rounding::same(s * 0.1), st);
            p.line_segment([Pos2::new(r.right() - s * 0.3, r.top()), Pos2::new(r.right() - s * 0.3, r.bottom())], st);
        }
        Icon::Plus => {
            p.line_segment([v(-0.4, 0.0), v(0.4, 0.0)], st);
            p.line_segment([v(0.0, -0.4), v(0.0, 0.4)], st);
        }
        Icon::Folder => {
            p.add(egui::Shape::closed_line(vec![v(-0.46, -0.32), v(-0.12, -0.32), v(0.0, -0.2), v(0.46, -0.2), v(0.46, 0.36), v(-0.46, 0.36)], st));
            p.line_segment([v(-0.46, -0.08), v(0.46, -0.08)], st);
        }
        Icon::Image => {
            let r = Rect::from_center_size(c, Vec2::new(s * 0.94, s * 0.74));
            p.rect_stroke(r, Rounding::same(s * 0.1), st);
            p.circle_filled(v(0.2, -0.14), s * 0.08, col);
            polyline(p, vec![v(-0.38, 0.28), v(-0.12, 0.0), v(0.06, 0.16), v(0.18, 0.06), v(0.4, 0.28)], st);
        }
        Icon::Effect => {
            // A sparkle: a four-pointed star and a small one.
            let star = |cx: f32, cy: f32, r: f32| -> Vec<Pos2> {
                (0..8)
                    .map(|i| {
                        let a = (i as f32 * 45.0 - 90.0).to_radians();
                        let k = if i % 2 == 0 { r } else { r * 0.3 };
                        v(cx + k * a.cos(), cy + k * a.sin())
                    })
                    .collect()
            };
            p.add(egui::Shape::closed_line(star(-0.08, 0.06, 0.4), st));
            p.add(egui::Shape::closed_line(star(0.32, -0.3, 0.16), Stroke::new(st.width * 0.8, col)));
        }
        Icon::Mic => {
            let capsule = Rect::from_min_max(v(-0.15, -0.46), v(0.15, 0.1));
            p.rect_stroke(capsule, Rounding::same(s * 0.15), st);
            polyline(p, arc(v(0.0, -0.06), s * 0.3, 0.0, 180.0), st);
            p.line_segment([v(0.0, 0.24), v(0.0, 0.42)], st);
            p.line_segment([v(-0.18, 0.44), v(0.18, 0.44)], st);
        }
        Icon::Music => {
            p.circle_filled(v(-0.26, 0.3), s * 0.13, col);
            p.circle_filled(v(0.26, 0.2), s * 0.13, col);
            p.line_segment([v(-0.14, 0.3), v(-0.14, -0.32)], st);
            p.line_segment([v(0.38, 0.2), v(0.38, -0.42)], st);
            p.line_segment([v(-0.14, -0.32), v(0.38, -0.42)], Stroke::new(st.width * 1.8, col));
        }
        Icon::Home => {
            p.add(egui::Shape::line(vec![v(-0.46, -0.02), v(0.0, -0.42), v(0.46, -0.02)], st));
            p.add(egui::Shape::line(vec![v(-0.32, -0.12), v(-0.32, 0.4), v(0.32, 0.4), v(0.32, -0.12)], st));
        }
    }
}

/// The tooltip of a tool button: its name, and its shortcut in grey.
fn tool_tip(resp: Response, name: &str, shortcut: &str) -> Response {
    resp.on_hover_ui(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).color(TEXT));
            if !shortcut.is_empty() {
                ui.label(RichText::new(shortcut).color(TEXT_3));
            }
        });
    })
}

/// A square tool button showing only an icon (its name appears on hover), in the spirit of
/// the tool bars of video editors. `on` marks a mode that is active.
pub fn icon_button(ui: &mut Ui, icon: Icon, name: &str, shortcut: &str, on: bool, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let h = ui.ctx().animate_bool(resp.id, resp.hovered() && enabled);
    let fill = if on { ACCENT } else { Color32::from_rgb(70, 70, 76).gamma_multiply(h) };
    ui.painter().rect_filled(rect, Rounding::same(6.0), fill);
    let col = match (enabled, on) {
        (false, _) => TEXT_3.linear_multiply(0.6),
        (true, true) => Color32::WHITE,
        (true, false) => lerp_color(TEXT_2, TEXT, h),
    };
    paint_icon(ui.painter(), icon, rect.center(), 17.0, col);
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    tool_tip(resp, name, shortcut)
}

/// Several tool buttons on one rounded strip.
pub fn tool_group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::none()
        .fill(Color32::from_rgb(44, 44, 48))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(54, 54, 59)))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::same(2.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(add).inner
        })
        .inner
}

/// A small icon button for the corner of a section (rename, delete…).
pub fn small_icon_button(ui: &mut Ui, icon: Icon, name: &str, enabled: bool, danger: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(22.0, 20.0), if enabled { Sense::click() } else { Sense::hover() });
    let h = ui.ctx().animate_bool(resp.id, resp.hovered() && enabled);
    ui.painter().rect_filled(rect, Rounding::same(5.0), Color32::from_rgb(64, 64, 70).gamma_multiply(h));
    let col = match (enabled, danger) {
        (false, _) => Color32::from_rgb(82, 82, 88),
        (true, true) => lerp_color(TEXT_2, RED, h),
        (true, false) => lerp_color(TEXT_2, TEXT, h),
    };
    paint_icon(ui.painter(), icon, rect.center(), 13.0, col);
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    tool_tip(resp, name, "")
}

/// A titled group box with small buttons on the right of its title.
pub fn section_with<R>(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui), add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.set_height(20.0);
        ui.label(RichText::new(title).font(medium(12.5)).color(TEXT_2));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            actions(ui);
        });
    });
    ui.add_space(-4.0);
    egui::Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A label and a segmented control on one row, or the control below the label when its
/// options do not fit beside it.
pub fn segmented_row<T: PartialEq + Copy>(ui: &mut Ui, label: &str, value: &mut T, options: &[(T, &str)]) -> bool {
    let need = segmented_width(ui, options);
    if need <= ui.available_width() - 82.0 {
        ui.horizontal(|ui| {
            fixed_label(ui, 78.0, RichText::new(label).color(TEXT), egui::Align::Min);
            segmented(ui, value, options, ui.available_width())
        })
        .inner
    } else {
        ui.label(RichText::new(label).color(TEXT));
        ui.add_space(-2.0);
        segmented(ui, value, options, ui.available_width())
    }
}

/// The width a segmented control needs to show all its labels.
pub fn segmented_width<T>(ui: &Ui, options: &[(T, &str)]) -> f32 {
    options.iter().map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), medium(12.5), TEXT).size().x + 16.0).sum::<f32>() + 4.0
}
