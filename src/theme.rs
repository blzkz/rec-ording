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
    let n = options.len().max(1) as f32;
    let seg_w = rect.width() / n;
    let mut changed = false;
    for (i, (opt, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(rect.left() + seg_w * i as f32, rect.top()), Vec2::new(seg_w, h));
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
