//! Custom widgets for the viewer.

use std::time::Instant;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

use crate::style::*;

/// The logo (assets/logo.svg: a trace from a pad to a via) and the name.
pub fn wordmark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(86.0, 24.0), Sense::hover());
    let p = ui.painter();
    let x = rect.left();
    let cy = rect.center().y;
    // Logo units are the 64-unit grid of the SVG. The mark goes from y 7 to y 56.5.
    let k = 0.4;
    let at = |ux: f32, uy: f32| Pos2::new(x + (ux - 15.0) * k, cy + (uy - 31.75) * k);
    let trace = [
        at(22.0, 18.0),
        at(22.0, 40.0),
        at(32.0, 50.0),
        at(42.0, 50.0),
    ];
    p.add(egui::Shape::line(
        trace.to_vec(),
        Stroke::new(7.0 * k, ACCENT),
    ));
    for c in trace {
        p.circle_filled(c, 3.5 * k, ACCENT);
    }
    for (c, r, hole) in [(at(22.0, 14.0), 7.0, 2.6), (at(46.0, 50.0), 6.0, 2.4)] {
        p.circle_filled(c, r * k, ACCENT);
        p.circle_filled(c, hole * k, PANEL);
    }
    p.text(
        Pos2::new(x + 22.0, cy),
        Align2::LEFT_CENTER,
        "lamina",
        semibold(16.0),
        TEXT,
    );
}

/// A switch with two options and a thumb that slides. Returns true if the value changed.
pub fn segmented(ui: &mut egui::Ui, rect: Rect, labels: &[&str], sel: &mut usize) -> bool {
    let id = ui.scope_id().with("segmented");
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(7), CANVAS_EDGE);
    p.rect_stroke(
        rect,
        CornerRadius::same(7),
        Stroke::new(1.0, HAIRLINE),
        StrokeKind::Inside,
    );
    let n = labels.len() as f32;
    let w = (rect.width() - 4.0) / n;
    let x = ui.ctx().animate_value_with_time(id, *sel as f32, 0.18);
    let thumb = Rect::from_min_size(
        Pos2::new(rect.left() + 2.0 + x * w, rect.top() + 2.0),
        Vec2::new(w, rect.height() - 4.0),
    );
    p.rect_filled(thumb, CornerRadius::same(5), RAISED);
    p.rect_stroke(
        thumb,
        CornerRadius::same(5),
        Stroke::new(1.0, Color32::from_rgb(0x2E, 0x35, 0x40)),
        StrokeKind::Inside,
    );
    let mut changed = false;
    for (i, l) in labels.iter().enumerate() {
        let c = Pos2::new(rect.left() + 2.0 + w * (i as f32 + 0.5), rect.center().y);
        let active = i == *sel;
        p.text(
            c,
            Align2::CENTER_CENTER,
            *l,
            semibold(12.5),
            if active { TEXT } else { TEXT_3 },
        );
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let i =
                (((pos.x - rect.left() - 2.0) / w).floor().max(0.0) as usize).min(labels.len() - 1);
            if i != *sel {
                *sel = i;
                changed = true;
            }
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    changed
}

/// Status chip: coloured dot and a short label.
pub fn chip(ui: &mut egui::Ui, color: Color32, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), body(12.5), TEXT);
    let size = Vec2::new(galley.size().x + 30.0, 26.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(13), RAISED);
    p.rect_stroke(
        rect,
        CornerRadius::same(13),
        Stroke::new(1.0, HAIRLINE),
        StrokeKind::Inside,
    );
    let dot = Pos2::new(rect.left() + 13.0, rect.center().y);
    p.circle_filled(dot, 3.5, color);
    p.galley(
        Pos2::new(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
}

/// The "live" indicator. It becomes brighter just after a reload.
pub fn live_dot(ui: &mut egui::Ui, last: Option<Instant>) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(64.0, 26.0), Sense::hover());
    let p = ui.painter();
    let age = last.map(|t| t.elapsed().as_secs_f32()).unwrap_or(99.0);
    let pulse = (1.0 - (age / 0.9)).clamp(0.0, 1.0);
    let c = Pos2::new(rect.left() + 8.0, rect.center().y);
    if pulse > 0.0 {
        p.circle_filled(c, 3.5 + 5.0 * pulse, ACCENT.gamma_multiply(0.25 * pulse));
        ui.ctx().request_repaint();
    }
    p.circle_filled(c, 3.5, if last.is_some() { ACCENT } else { TEXT_3 });
    p.text(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        "live",
        body(12.5),
        TEXT_2,
    );
    let tip = match last {
        Some(t) => format!(
            "The viewer watches the project folder. The last update was {:.1} s ago.",
            t.elapsed().as_secs_f32()
        ),
        None => "The viewer waits for the first load.".to_string(),
    };
    resp.on_hover_text(tip);
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(semibold(13.0)).color(TEXT));
}

pub fn hairline(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, HAIRLINE);
}

/// Small borderless text button.
pub fn text_button(ui: &mut egui::Ui, text: &str) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), body(12.0), TEXT_2);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + 12.0, 22.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(5), HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if resp.hovered() { TEXT } else { TEXT_2 };
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        body(12.0),
        color,
    );
    resp.clicked()
}

/// Accent button. Returns true when clicked and enabled.
pub fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let p = ui.painter();
    let fill = if !enabled {
        RAISED
    } else if resp.is_pointer_button_down_on() {
        Color32::from_rgb(0xC7, 0x77, 0x30)
    } else if resp.hovered() {
        Color32::from_rgb(0xEA, 0x98, 0x4E)
    } else {
        ACCENT
    };
    p.rect_filled(rect, CornerRadius::same(7), fill);
    if resp.has_focus() {
        p.rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(9),
            Stroke::new(1.5, ACCENT),
            StrokeKind::Outside,
        );
    }
    let fg = if enabled {
        Color32::from_rgb(0x16, 0x10, 0x0A)
    } else {
        TEXT_3
    };
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        semibold(13.0),
        fg,
    );
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
}

/// Layer row: swatch, name, item count, eye. Returns true if the user toggled it.
pub fn layer_row(
    ui: &mut egui::Ui,
    name: &str,
    color: Color32,
    count: usize,
    on: &mut bool,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(6), HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let sw = Rect::from_center_size(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Vec2::splat(12.0),
    );
    let solid = Color32::from_rgb(color.r(), color.g(), color.b());
    if *on {
        p.rect_filled(sw, CornerRadius::same(3), solid);
    } else {
        p.rect_stroke(
            sw,
            CornerRadius::same(3),
            Stroke::new(1.5, solid.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
    }
    // The name must not run into the count.
    let count_w = if count > 0 {
        p.layout_no_wrap(count.to_string(), mono(11.0), TEXT_3)
            .size()
            .x
            + 8.0
    } else {
        0.0
    };
    let room = rect.width() - 30.0 - 32.0 - count_w;
    p.text(
        Pos2::new(rect.left() + 30.0, rect.center().y),
        Align2::LEFT_CENTER,
        fit_text(p, name, body(13.0), room.max(20.0)),
        body(13.0),
        if *on { TEXT } else { TEXT_3 },
    );
    if count > 0 {
        p.text(
            Pos2::new(rect.right() - 32.0, rect.center().y),
            Align2::RIGHT_CENTER,
            count.to_string(),
            mono(11.0),
            TEXT_3,
        );
    }
    eye(p, Pos2::new(rect.right() - 14.0, rect.center().y), *on);
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
}

fn eye(p: &egui::Painter, c: Pos2, open: bool) {
    let color = if open { TEXT_2 } else { TEXT_3 };
    let w = 6.5;
    let h = if open { 3.6 } else { 0.8 };
    let pts: Vec<Pos2> = (0..=16)
        .map(|i| {
            let t = std::f32::consts::PI * i as f32 / 16.0;
            Pos2::new(c.x - w * t.cos(), c.y - h * t.sin())
        })
        .chain((0..=16).map(|i| {
            let t = std::f32::consts::PI * i as f32 / 16.0;
            Pos2::new(c.x + w * t.cos(), c.y + h * t.sin())
        }))
        .collect();
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.2, color)));
    if open {
        p.circle_filled(c, 1.8, color);
    }
}

/// One rule violation. Returns true if the user clicked it (to zoom to it).
pub fn violation_row(
    ui: &mut egui::Ui,
    color: Color32,
    rule: &str,
    message: &str,
    has_location: bool,
) -> bool {
    let width = ui.available_width();
    let galley = ui
        .painter()
        .layout(message.to_string(), body(12.0), TEXT_2, width - 22.0);
    let h = galley.size().y + 26.0;
    let sense = if has_location {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, h), sense);
    let p = ui.painter();
    if resp.hovered() && has_location {
        p.rect_filled(rect, CornerRadius::same(6), HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    p.circle_filled(Pos2::new(rect.left() + 8.0, rect.top() + 12.0), 3.5, color);
    p.text(
        Pos2::new(rect.left() + 20.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        rule.replace('_', " "),
        semibold(12.5),
        TEXT,
    );
    p.galley(
        Pos2::new(rect.left() + 20.0, rect.top() + 21.0),
        galley,
        TEXT_2,
    );
    if has_location {
        resp.clone().on_hover_text("Show on the board");
    }
    resp.clicked()
}

pub fn note_row(ui: &mut egui::Ui, n: &lamina_model::Note) {
    let width = ui.available_width();
    let galley = ui.painter().layout(
        n.text.clone(),
        body(12.5),
        if n.resolved { TEXT_3 } else { TEXT },
        width - 16.0,
    );
    let h = galley.size().y + 30.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect.shrink2(Vec2::new(0.0, 2.0)),
        CornerRadius::same(7),
        RAISED,
    );
    let who_color = if n.author == "agent" { OK } else { ACCENT };
    let meta = match (&n.refdes, n.at) {
        (Some(r), _) => format!(
            "{} · {}{}",
            n.author,
            r,
            if n.resolved { " · resolved" } else { "" }
        ),
        (None, Some(at)) => format!(
            "{} · {:.1}, {:.1}{}",
            n.author,
            at.x,
            at.y,
            if n.resolved { " · resolved" } else { "" }
        ),
        _ => format!(
            "{}{}",
            n.author,
            if n.resolved { " · resolved" } else { "" }
        ),
    };
    p.circle_filled(
        Pos2::new(rect.left() + 12.0, rect.top() + 14.0),
        3.0,
        who_color,
    );
    p.text(
        Pos2::new(rect.left() + 21.0, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        meta,
        body(11.5),
        TEXT_3,
    );
    p.text(
        Pos2::new(rect.right() - 10.0, rect.top() + 14.0),
        Align2::RIGHT_CENTER,
        &n.id,
        mono(11.0),
        TEXT_3,
    );
    p.galley(
        Pos2::new(rect.left() + 10.0, rect.top() + 23.0),
        galley,
        TEXT,
    );
}

/// The hint line at the bottom of the canvas. If the line is too long, items
/// (split by ` · `) are removed from the end until it fits.
pub fn hint(p: &egui::Painter, rect: Rect, text: &str) {
    let room = rect.width() - 160.0;
    let items: Vec<&str> = text.split(" · ").collect();
    let mut n = items.len();
    let mut line = text.to_string();
    while n > 1 && p.layout_no_wrap(line.clone(), body(12.0), TEXT_3).size().x > room {
        n -= 1;
        line = format!("{} · F1 for more", items[..n].join(" · "));
    }
    // Put it in the centre of the space to the right of the scale bar. A dark
    // background makes it easy to read over copper.
    let at = Pos2::new(
        rect.left() + 140.0 + (rect.width() - 140.0) / 2.0,
        rect.bottom() - 16.0,
    );
    let g = p.layout_no_wrap(line, body(12.0), TEXT_2);
    let r = Rect::from_center_size(at, g.size()).expand2(Vec2::new(8.0, 3.0));
    p.rect_filled(r, CornerRadius::same(5), Color32::from_black_alpha(215));
    p.galley(r.min + Vec2::new(8.0, 3.0), g, TEXT_2);
}

/// The text, cut at the end with "…" so that it is not wider than `max`.
pub fn fit_text(p: &egui::Painter, text: &str, font: egui::FontId, max: f32) -> String {
    let width = |t: &str| p.layout_no_wrap(t.to_string(), font.clone(), TEXT).size().x;
    if width(text) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if width(&t) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// A short state message at the bottom of the canvas, above the hint line.
/// Row 0 is just above the hint line; row 1 is above row 0.
pub fn banner(p: &egui::Painter, rect: Rect, text: &str, row: u8) {
    let text = fit_text(p, text, body(12.5), (rect.width() - 60.0).max(40.0));
    let g = p.layout_no_wrap(text, body(12.5), TEXT);
    let at = Pos2::new(rect.center().x, rect.bottom() - 48.0 - 32.0 * row as f32);
    let r = Rect::from_center_size(at, g.size()).expand2(Vec2::new(10.0, 5.0));
    p.rect_filled(r, CornerRadius::same(6), RAISED);
    p.rect_stroke(
        r,
        CornerRadius::same(6),
        Stroke::new(1.0, ACCENT),
        egui::StrokeKind::Inside,
    );
    p.galley(r.min + Vec2::new(10.0, 5.0), g, TEXT);
}

/// Toolbar button: label with its shortcut key, accent when active.
pub fn tool_button(ui: &mut egui::Ui, label: &str, key: &str, active: bool) -> egui::Response {
    // Compact form: only the key letter.
    if label.is_empty() {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::click());
        let p = ui.painter();
        if active {
            p.rect_filled(rect, CornerRadius::same(6), ACCENT_DIM);
        } else if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(6), HOVER);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            key,
            mono(12.5),
            if active { ACCENT } else { TEXT },
        );
        return resp;
    }
    let lg = ui
        .painter()
        .layout_no_wrap(label.to_string(), body(12.5), TEXT);
    let kg = ui
        .painter()
        .layout_no_wrap(key.to_string(), mono(10.5), TEXT_3);
    let w = lg.size().x + kg.size().x + 24.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, CornerRadius::same(6), ACCENT_DIM);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(6), HOVER);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let x = rect.left() + 10.0;
    p.text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        body(12.5),
        if active { ACCENT } else { TEXT },
    );
    p.text(
        Pos2::new(x + lg.size().x + 6.0, rect.center().y + 0.5),
        Align2::LEFT_CENTER,
        key,
        mono(10.5),
        if active {
            ACCENT.gamma_multiply(0.7)
        } else {
            TEXT_3
        },
    );
    resp
}

/// Thin vertical divider inside a toolbar.
pub fn vdivider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 18.0), Sense::hover());
    ui.painter().rect_filled(
        Rect::from_center_size(rect.center(), Vec2::new(1.0, 18.0)),
        0.0,
        HAIRLINE,
    );
}

/// A small pill that shows a value. Returns the response for clicks.
pub fn pill(ui: &mut egui::Ui, swatch: Option<Color32>, text: &str) -> egui::Response {
    let g = ui
        .painter()
        .layout_no_wrap(text.to_string(), mono(11.5), TEXT_2);
    let sw = if swatch.is_some() { 16.0 } else { 0.0 };
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(g.size().x + 18.0 + sw, 28.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(6), HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut x = rect.left() + 9.0;
    if let Some(c) = swatch {
        p.rect_filled(
            Rect::from_center_size(Pos2::new(x + 5.0, rect.center().y), Vec2::splat(10.0)),
            CornerRadius::same(3),
            Color32::from_rgb(c.r(), c.g(), c.b()),
        );
        x += sw;
    }
    p.text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        mono(11.5),
        if resp.hovered() { TEXT } else { TEXT_2 },
    );
    resp
}

/// Floating message over the canvas.
pub fn toast(p: &egui::Painter, at: Pos2, text: &str, color: Color32, alpha: f32) {
    let g = p.layout_no_wrap(text.to_string(), body(12.5), TEXT.gamma_multiply(alpha));
    let rect = Rect::from_center_size(at, g.size() + Vec2::new(28.0, 14.0));
    p.rect_filled(
        rect,
        CornerRadius::same(8),
        RAISED.gamma_multiply(0.96 * alpha),
    );
    p.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, HAIRLINE.gamma_multiply(alpha)),
        StrokeKind::Inside,
    );
    p.circle_filled(
        Pos2::new(rect.left() + 12.0, rect.center().y),
        3.0,
        color.gamma_multiply(alpha),
    );
    p.galley(
        Pos2::new(rect.left() + 21.0, rect.top() + 7.0),
        g,
        TEXT.gamma_multiply(alpha),
    );
}

/// A row of a right-click menu: the label at the left, the shortcut dim at
/// the right. The whole row is the button.
pub fn menu_row<'a>(label: &'a str, key: &'a str) -> impl egui::Widget + 'a {
    move |ui: &mut egui::Ui| {
        let w = ui.available_width().clamp(180.0, 270.0);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), egui::Sense::click());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, 5.0, ACCENT_DIM);
        }
        let color = if resp.hovered() { TEXT } else { TEXT_2 };
        let key_w = if key.is_empty() {
            0.0
        } else {
            p.layout_no_wrap(key.to_string(), mono(11.0), TEXT_3)
                .size()
                .x
                + 16.0
        };
        // A long net name must not run into the key or out of the menu.
        let label = fit_text(p, label, body(12.5), w - 20.0 - key_w);
        p.text(
            Pos2::new(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            body(12.5),
            color,
        );
        if !key.is_empty() {
            p.text(
                Pos2::new(rect.right() - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                key,
                mono(11.0),
                TEXT_3,
            );
        }
        resp
    }
}
