//! The visual language of MacAmp: an original Graphite skin in the spirit of
//! late-90s desktop audio players, plus two built-in variations.

use egui::{Color32, FontFamily, FontId, Pos2, Rect, Stroke};
use serde::{Deserialize, Serialize};

pub const MAIN_W: f32 = 550.0;
pub const MAIN_H: f32 = 232.0;
pub const SHADE_H: f32 = 30.0;
pub const TITLE_H: f32 = 28.0;
pub const PLAYLIST_W: f32 = 550.0;
pub const PLAYLIST_H: f32 = 464.0;
pub const PLAYLIST_MIN_W: f32 = 330.0;
pub const PLAYLIST_MIN_H: f32 = 200.0;
pub const PLAYLIST_MAX_W: f32 = 1400.0;
pub const PLAYLIST_MAX_H: f32 = 1200.0;
pub const EQ_W: f32 = 550.0;
pub const EQ_H: f32 = 232.0;

pub const PIXEL_FAMILY: &str = "macamp_pixel";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skin {
    #[default]
    Graphite,
    Midnight,
    Brass,
}

impl Skin {
    pub const ALL: [Self; 3] = [Self::Graphite, Self::Midnight, Self::Brass];

    pub fn name(self) -> &'static str {
        match self {
            Self::Graphite => "Graphite",
            Self::Midnight => "Midnight",
            Self::Brass => "Brass",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Graphite => Palette {
                frame: rgb(0x3c, 0x43, 0x3b),
                title_top: rgb(0x67, 0x71, 0x5f),
                title_bottom: rgb(0x37, 0x3f, 0x36),
                edge_hi: rgb(0x8e, 0x98, 0x8a),
                edge_lo: rgb(0x15, 0x1a, 0x15),
                face: rgb(0x4a, 0x53, 0x48),
                face_dark: rgb(0x39, 0x41, 0x39),
                inset: rgb(0x11, 0x16, 0x12),
                screen: rgb(0x07, 0x11, 0x0a),
                text: rgb(0xe8, 0xee, 0xe3),
                title_text: rgb(0xf3, 0xf7, 0xef),
                dim: rgb(0xaa, 0xb5, 0xa7),
                led: rgb(0xa8, 0xf0, 0x78),
                amber: rgb(0xff, 0xbc, 0x5b),
                red: rgb(0xff, 0x6f, 0x5e),
                select: rgb(0x20, 0x43, 0x2a),
                select_text: rgb(0xd9, 0xff, 0xbe),
                slider_track: rgb(0x0e, 0x13, 0x0f),
                thumb: rgb(0xc3, 0xcd, 0xbe),
            },
            Self::Midnight => Palette {
                frame: rgb(0x28, 0x32, 0x46),
                title_top: rgb(0x40, 0x55, 0x78),
                title_bottom: rgb(0x1a, 0x25, 0x37),
                edge_hi: rgb(0x7b, 0x90, 0xb2),
                edge_lo: rgb(0x0d, 0x14, 0x1f),
                face: rgb(0x33, 0x42, 0x5c),
                face_dark: rgb(0x26, 0x33, 0x4a),
                inset: rgb(0x0b, 0x11, 0x1c),
                screen: rgb(0x05, 0x0c, 0x17),
                text: rgb(0xe2, 0xec, 0xfb),
                title_text: rgb(0xf0, 0xf6, 0xff),
                dim: rgb(0xa3, 0xb6, 0xd0),
                led: rgb(0x7e, 0xe8, 0xff),
                amber: rgb(0xff, 0x9b, 0x5c),
                red: rgb(0xff, 0x7a, 0x8a),
                select: rgb(0x1d, 0x3f, 0x5e),
                select_text: rgb(0xd6, 0xf5, 0xff),
                slider_track: rgb(0x0a, 0x12, 0x20),
                thumb: rgb(0xd0, 0xdc, 0xef),
            },
            Self::Brass => Palette {
                frame: rgb(0x4a, 0x42, 0x37),
                title_top: rgb(0x8b, 0x7b, 0x63),
                title_bottom: rgb(0x43, 0x3a, 0x2e),
                edge_hi: rgb(0xc1, 0xae, 0x90),
                edge_lo: rgb(0x1d, 0x19, 0x14),
                face: rgb(0x5b, 0x51, 0x43),
                face_dark: rgb(0x44, 0x3b, 0x30),
                inset: rgb(0x1a, 0x16, 0x11),
                screen: rgb(0x17, 0x12, 0x07),
                text: rgb(0xf2, 0xea, 0xd9),
                title_text: rgb(0xfb, 0xf4, 0xe7),
                dim: rgb(0xc1, 0xb4, 0x9c),
                led: rgb(0xff, 0xc4, 0x6b),
                amber: rgb(0xff, 0xd6, 0x97),
                red: rgb(0xff, 0x7d, 0x5c),
                select: rgb(0x5a, 0x45, 0x24),
                select_text: rgb(0xff, 0xe9, 0xbf),
                slider_track: rgb(0x17, 0x12, 0x0b),
                thumb: rgb(0xd9, 0xca, 0xaf),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub frame: Color32,
    pub title_top: Color32,
    pub title_bottom: Color32,
    pub edge_hi: Color32,
    pub edge_lo: Color32,
    pub face: Color32,
    pub face_dark: Color32,
    pub inset: Color32,
    pub screen: Color32,
    pub text: Color32,
    pub title_text: Color32,
    pub dim: Color32,
    pub led: Color32,
    pub amber: Color32,
    pub red: Color32,
    pub select: Color32,
    pub select_text: Color32,
    pub slider_track: Color32,
    pub thumb: Color32,
}

impl Palette {
    pub fn text_font(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(PIXEL_FAMILY.into()))
    }

    pub fn mono(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn bevel(painter: &egui::Painter, rect: Rect, fill: Color32, hi: Color32, lo: Color32) {
    painter.rect_filled(rect, 0.0, fill);
    painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, hi));
    painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, hi));
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, lo),
    );
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, lo),
    );
}

pub fn inset(painter: &egui::Painter, rect: Rect, fill: Color32, hi: Color32, lo: Color32) {
    painter.rect_filled(rect, 0.0, fill);
    painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, lo));
    painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, lo));
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, hi),
    );
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, hi),
    );
}

pub fn gradient_rect(
    painter: &egui::Painter,
    rect: Rect,
    top: Color32,
    bottom: Color32,
    steps: usize,
) {
    let steps = steps.max(1);
    let h = rect.height() / steps as f32;
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let color = lerp_color(top, bottom, t);
        let y = rect.top() + i as f32 * h;
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(rect.left(), y),
                Pos2::new(rect.right(), y + h + 0.5),
            ),
            0.0,
            color,
        );
    }
}

pub fn child_rect(parent: Rect, x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(parent.min + egui::vec2(x, y), egui::vec2(w, h))
}
