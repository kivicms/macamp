//! Hand-painted widgets for the classic skin. Everything is drawn from
//! primitives so the interface stays crisp at any DPI.

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Id, Painter, Pos2, Rect, Response, Sense,
    Shape, Stroke, Ui, Vec2,
};

use crate::theme::{bevel, inset, lerp_color, Palette};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Previous,
    Play,
    Pause,
    Stop,
    Next,
    Add,
    Remove,
    Menu,
    Minimize,
    Close,
    Folder,
    Shade,
}

pub fn draw_icon(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let c = rect.center();
    let w = rect.width();
    let h = rect.height();
    let s = w.min(h);
    let tri = |cx: f32, dir: f32| {
        let half = s * 0.24;
        let points = vec![
            pos2(cx - half * dir, c.y - half),
            pos2(cx - half * dir, c.y + half),
            pos2(cx + half * dir, c.y),
        ];
        painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
    };
    let bar = |x: f32, w: f32| {
        painter.rect_filled(
            Rect::from_center_size(pos2(x, c.y), vec2(w, s * 0.48)),
            0.0,
            color,
        );
    };
    match icon {
        Icon::Previous => {
            bar(c.x - s * 0.26, s * 0.09);
            tri(c.x + s * 0.06, -1.0);
        }
        Icon::Play => {
            tri(c.x + s * 0.05, 1.0);
        }
        Icon::Pause => {
            bar(c.x - s * 0.13, s * 0.12);
            bar(c.x + s * 0.13, s * 0.12);
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.44, s * 0.44)),
                0.0,
                color,
            );
        }
        Icon::Next => {
            tri(c.x - s * 0.06, 1.0);
            bar(c.x + s * 0.26, s * 0.09);
        }
        Icon::Add => {
            bar(c.x, s * 0.48);
            painter.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.48, s * 0.09)),
                0.0,
                color,
            );
        }
        Icon::Remove => {
            painter.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.48, s * 0.09)),
                0.0,
                color,
            );
        }
        Icon::Menu => {
            for i in [-1.0, 0.0, 1.0] {
                painter.rect_filled(
                    Rect::from_center_size(pos2(c.x, c.y + i * s * 0.2), vec2(s * 0.44, s * 0.08)),
                    0.0,
                    color,
                );
            }
        }
        Icon::Minimize => {
            painter.rect_filled(
                Rect::from_center_size(pos2(c.x, c.y + s * 0.15), vec2(s * 0.42, s * 0.09)),
                0.0,
                color,
            );
        }
        Icon::Close => {
            let d = s * 0.28;
            let stroke = Stroke::new((s * 0.09).max(1.0), color);
            painter.line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], stroke);
            painter.line_segment([pos2(c.x - d, c.y + d), pos2(c.x + d, c.y - d)], stroke);
        }
        Icon::Folder => {
            let r = Rect::from_center_size(c, vec2(s * 0.5, s * 0.36));
            painter.rect_filled(r, 0.0, color);
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(r.left() + 1.0, r.top() - s * 0.08),
                    vec2(s * 0.22, s * 0.12),
                ),
                0.0,
                color,
            );
        }
        Icon::Shade => {
            let bar_h = (s * 0.09).max(1.0);
            painter.rect_filled(
                Rect::from_center_size(pos2(c.x, c.y - s * 0.12), vec2(s * 0.48, bar_h)),
                0.0,
                color,
            );
            painter.rect_filled(
                Rect::from_center_size(pos2(c.x - s * 0.16, c.y + s * 0.14), vec2(s * 0.16, bar_h)),
                0.0,
                color,
            );
            painter.rect_filled(
                Rect::from_center_size(pos2(c.x + s * 0.16, c.y + s * 0.14), vec2(s * 0.16, bar_h)),
                0.0,
                color,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn classic_button(
    ui: &mut Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    palette: &Palette,
    icon: Option<Icon>,
    label: Option<&str>,
    active: bool,
    enabled: bool,
) -> Response {
    let id = Id::unique(("classic_button", id_salt));
    let response = ui.interact(rect, id, Sense::click());
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let face = if active {
        lerp_color(palette.face, palette.led, 0.7)
    } else if hovered {
        lerp_color(palette.face, palette.edge_hi, 0.22)
    } else {
        palette.face
    };
    let face = if pressed {
        lerp_color(face, palette.edge_lo, 0.25)
    } else {
        face
    };
    if pressed {
        inset(ui.painter(), rect, face, palette.edge_hi, palette.edge_lo);
    } else {
        bevel(ui.painter(), rect, face, palette.edge_hi, palette.edge_lo);
    }
    if active {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            0.0,
            Stroke::new(1.0, palette.led),
            egui::StrokeKind::Inside,
        );
    }
    let fg = if !enabled {
        palette.dim.gamma_multiply(0.45)
    } else if active {
        palette.inset
    } else {
        palette.text
    };
    let offset = vec2(0.0, if pressed { 1.0 } else { 0.0 });
    let both = icon.is_some() && label.is_some();
    if let Some(icon) = icon {
        let icon_rect = if both {
            Rect::from_center_size(
                pos2(rect.left() + 13.0, rect.center().y) + offset,
                vec2(11.0, 11.0),
            )
        } else {
            Rect::from_center_size(rect.center() + offset, rect.size() * 0.5)
        };
        draw_icon(ui.painter(), icon, icon_rect, fg);
    }
    if let Some(label) = label {
        ui.painter().text(
            rect.center() + offset + if both { vec2(7.0, 0.0) } else { Vec2::ZERO },
            Align2::CENTER_CENTER,
            label,
            palette.text_font(8.0),
            fg,
        );
    }
    response
}

pub fn tiny_title_button(
    ui: &mut Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    palette: &Palette,
    icon: Icon,
) -> Response {
    let response = ui.interact(rect, Id::unique(("title_button", id_salt)), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let face = if hovered {
        lerp_color(palette.face, palette.edge_hi, 0.25)
    } else {
        palette.face
    };
    if response.is_pointer_button_down_on() {
        inset(ui.painter(), rect, face, palette.edge_hi, palette.edge_lo);
    } else {
        bevel(ui.painter(), rect, face, palette.edge_hi, palette.edge_lo);
    }
    draw_icon(ui.painter(), icon, rect.shrink(3.5), palette.text);
    response
}

pub fn toggle_button(
    ui: &mut Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    palette: &Palette,
    label: &str,
    on: bool,
    tooltip: &str,
) -> Response {
    let response = classic_button(ui, rect, id_salt, palette, None, Some(label), on, true);
    response.on_hover_text(tooltip)
}

/// A horizontal slider. Returns the response and the new value when the user
/// clicks or drags.
pub fn slider(
    ui: &mut Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    palette: &Palette,
    value: f32,
    centered: bool,
    tooltip: &str,
) -> (Response, Option<f32>) {
    let response = ui.interact(
        rect,
        Id::unique(("slider", id_salt)),
        Sense::click_and_drag(),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    let value = value.clamp(0.0, 1.0);
    let track = Rect::from_min_max(
        pos2(rect.left(), rect.center().y - 2.5),
        pos2(rect.right(), rect.center().y + 2.5),
    );
    inset(
        ui.painter(),
        track,
        palette.slider_track,
        palette.edge_hi,
        palette.edge_lo,
    );

    let thumb_x = rect.left() + rect.width() * value;
    if centered {
        let center = rect.center().x;
        let fill = Rect::from_min_max(
            pos2(thumb_x.min(center), track.top() + 1.0),
            pos2(thumb_x.max(center), track.bottom() - 1.0),
        );
        ui.painter()
            .rect_filled(fill, 0.0, palette.led.gamma_multiply(0.65));
        let tick = Rect::from_center_size(pos2(center, track.center().y), vec2(1.0, 9.0));
        ui.painter()
            .rect_filled(tick, 0.0, palette.dim.gamma_multiply(0.7));
    } else {
        let fill = Rect::from_min_max(
            pos2(track.left() + 1.0, track.top() + 1.0),
            pos2(thumb_x, track.bottom() - 1.0),
        );
        ui.painter()
            .rect_filled(fill, 0.0, palette.led.gamma_multiply(0.72));
    }
    let thumb = Rect::from_center_size(
        pos2(thumb_x, rect.center().y),
        vec2(8.0, rect.height().max(12.0) * 0.9),
    );
    bevel(
        ui.painter(),
        thumb,
        palette.thumb,
        palette.edge_hi,
        palette.edge_lo,
    );
    ui.painter().line_segment(
        [thumb.center_top(), thumb.center_bottom()],
        Stroke::new(1.0, palette.face_dark),
    );

    let mut new_value = None;
    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            new_value = Some(((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0));
        }
    }
    response.on_hover_text(tooltip).pipe(|r| (r, new_value))
}

pub fn draw_spectrum(painter: &Painter, rect: Rect, levels: &[f32], palette: &Palette) {
    let count = levels.len().max(1);
    let gap = 2.0;
    let bar_w = ((rect.width() - gap * (count as f32 - 1.0)) / count as f32).max(1.0);
    let segments = 21;
    let segment_h = 2.6;
    let segment_gap = 1.0;
    let bottom = rect.bottom();
    for (i, level) in levels.iter().enumerate() {
        let x = rect.left() + i as f32 * (bar_w + gap);
        let lit = (level.clamp(0.0, 1.0) * segments as f32).round() as usize;
        for s in 0..segments {
            if s >= lit {
                continue;
            }
            let y = bottom - (s as f32 + 1.0) * (segment_h + segment_gap);
            let t = s as f32 / segments as f32;
            let color = if t > 0.84 {
                palette.red
            } else if t > 0.58 {
                palette.amber
            } else {
                palette.led
            };
            painter.rect_filled(
                Rect::from_min_size(pos2(x, y), vec2(bar_w, segment_h)),
                0.0,
                color.gamma_multiply(0.92),
            );
        }
    }
}

pub fn led_text(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    size: f32,
    color: Color32,
    shadow: Color32,
) {
    let font = FontId::new(
        size,
        egui::FontFamily::Name(crate::theme::PIXEL_FAMILY.into()),
    );
    painter.text(pos + vec2(1.0, 1.0), anchor, text, font.clone(), shadow);
    painter.text(pos, anchor, text, font, color);
}

pub fn draw_scrollbar(
    ui: &mut Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    palette: &Palette,
    offset: f32,
    content: f32,
    viewport: f32,
) -> Response {
    let id = Id::unique(("scrollbar", id_salt));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    inset(
        ui.painter(),
        rect,
        palette.inset,
        palette.edge_lo,
        palette.edge_hi,
    );
    let frac = (viewport / content.max(1.0)).clamp(0.08, 1.0);
    let thumb_h = (rect.height() * frac).max(18.0);
    let travel = rect.height() - thumb_h;
    let max_offset = (content - viewport).max(0.0);
    let t = if max_offset > 0.0 {
        (offset / max_offset).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb = Rect::from_min_size(
        pos2(rect.left() + 1.0, rect.top() + t * travel),
        vec2(rect.width() - 2.0, thumb_h),
    );
    bevel(
        ui.painter(),
        thumb,
        if response.hovered() || response.dragged() {
            lerp_color(palette.face, palette.edge_hi, 0.2)
        } else {
            palette.face
        },
        palette.edge_hi,
        palette.edge_lo,
    );
    if response.dragged() || response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let t = ((pos.y - rect.top() - thumb_h * 0.5) / travel.max(1.0)).clamp(0.0, 1.0);
            // The caller interprets the response through `scroll_to`.
            ui.ctx()
                .data_mut(|d| d.insert_temp(id.with("scroll_to"), t));
        }
    }
    response
}

pub fn scroll_to_value(ui: &Ui, id_salt: impl std::hash::Hash + std::fmt::Debug) -> Option<f32> {
    let id = Id::unique(("scrollbar", id_salt)).with("scroll_to");
    ui.ctx().data_mut(|d| d.remove_temp::<f32>(id))
}

pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x
}

pub fn draw_marquee(
    ui: &Ui,
    rect: Rect,
    text: &str,
    offset: f32,
    palette: &Palette,
    size: f32,
    color: Color32,
) {
    let font = palette.text_font(size);
    let painter = ui.painter().with_clip_rect(rect);
    let width = painter
        .layout_no_wrap(text.to_owned(), font.clone(), color)
        .size()
        .x;
    let gap = 52.0;
    let x = rect.left() - offset;
    painter.text(
        pos2(x, rect.top() + 1.0),
        Align2::LEFT_TOP,
        text,
        font.clone(),
        color,
    );
    if width + gap > rect.width() {
        painter.text(
            pos2(x + width + gap, rect.top() + 1.0),
            Align2::LEFT_TOP,
            text,
            font,
            color,
        );
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}
