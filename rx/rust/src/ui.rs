//! Shared drawing primitives: the design-space context, the frosted glass
//! surfaces, circular icon buttons and the hand-drawn glyph set from v1.0.1.

use crate::scene::{rect, Cmd, Fill, Gradient, ImgId, Rect};
use crate::text::{self, Align, Weight};
use crate::theme::{m, white, Palette};
use tiny_skia::Color;

/// Design-space -> pixel mapping.
#[derive(Clone, Copy)]
pub struct Ctx {
    pub s: f32,
    pub ox: f32,
    pub oy: f32,
    pub w: f32,
    pub h: f32,
}

impl Ctx {
    pub fn new(width: f32, height: f32) -> Self {
        let sx = width / crate::theme::DESIGN_W;
        let sy = height / crate::theme::DESIGN_H;
        let s = sx.max(sy);
        Ctx {
            s,
            ox: (width - crate::theme::DESIGN_W * s) * 0.5,
            oy: (height - crate::theme::DESIGN_H * s) * 0.5,
            w: width,
            h: height,
        }
    }

    #[inline]
    pub fn x(&self, v: f32) -> f32 {
        self.ox + v * self.s
    }
    #[inline]
    pub fn y(&self, v: f32) -> f32 {
        self.oy + v * self.s
    }
    #[inline]
    pub fn d(&self, v: f32) -> f32 {
        v * self.s
    }
    #[inline]
    pub fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        rect(self.x(x), self.y(y), self.d(w), self.d(h))
    }
}

pub fn glass(out: &mut Vec<Cmd>, ctx: &Ctx, r: Rect, radius: f32, alpha: f32) {
    out.push(Cmd::RoundRect {
        rect: r,
        radius,
        fill: Some(Fill::Vertical(Gradient {
            top: white((64.0 * alpha) as u8),
            bottom: white((14.0 * alpha) as u8),
        })),
        stroke: Some((1.5 * ctx.s, white((46.0 * alpha) as u8))),
    });
    // Top sheen: a thin brighter band along the upper edge.
    let sheen = rect(r.left(), r.top(), r.width(), r.height() * 0.42);
    out.push(Cmd::RoundRect {
        rect: sheen,
        radius,
        fill: Some(Fill::Vertical(Gradient {
            top: white((26.0 * alpha) as u8),
            bottom: white(0),
        })),
        stroke: None,
    });
}

pub fn circle_button(out: &mut Vec<Cmd>, ctx: &Ctx, cx: f32, cy: f32, d: f32, glyph: Glyph, active: bool) {
    let r = ctx.d(d) * 0.5;
    out.push(Cmd::Circle {
        cx: ctx.x(cx),
        cy: ctx.y(cy),
        radius: r,
        fill: Some(Fill::Solid(if active {
            Color::from_rgba8(255, 255, 255, 46)
        } else {
            Color::from_rgba8(255, 255, 255, 22)
        })),
        stroke: Some((1.2 * ctx.s, white(40))),
    });
    glyph_at(out, ctx, glyph, cx, cy, d * 0.46, if active { white(255) } else { white(226) });
}

pub fn glyph_at(out: &mut Vec<Cmd>, ctx: &Ctx, glyph: Glyph, cx: f32, cy: f32, size: f32, color: Color) {
    let s = ctx.d(size);
    let cx = ctx.x(cx);
    let cy = ctx.y(cy);
    let w = (size * 0.11).max(1.5) * ctx.s;
    let line = |out: &mut Vec<Cmd>, x0: f32, y0: f32, x1: f32, y1: f32| {
        out.push(Cmd::Line { x0, y0, x1, y1, width: w, color });
    };
    match glyph {
        Glyph::Menu => {
            for i in -1..=1 {
                line(out, cx - s * 0.36, cy + i as f32 * s * 0.24, cx + s * 0.36, cy + i as f32 * s * 0.24);
            }
        }
        Glyph::Grid => {
            let gap = s * 0.08;
            let cell = (s * 0.62 - gap) * 0.5;
            for row in 0..2 {
                for col in 0..2 {
                    let l = cx - s * 0.31 + col as f32 * (cell + gap);
                    let t = cy - s * 0.31 + row as f32 * (cell + gap);
                    out.push(Cmd::RoundRect {
                        rect: rect(l, t, cell, cell),
                        radius: cell * 0.3,
                        fill: None,
                        stroke: Some((w, color)),
                    });
                }
            }
        }
        Glyph::Sort => {
            line(out, cx - s * 0.34, cy - s * 0.2, cx + s * 0.34, cy - s * 0.2);
            line(out, cx - s * 0.34, cy + s * 0.02, cx + s * 0.06, cy + s * 0.02);
            line(out, cx - s * 0.34, cy + s * 0.24, cx - s * 0.14, cy + s * 0.24);
        }
        Glyph::Sliders => {
            for i in -1..=1 {
                let y = cy + i as f32 * s * 0.26;
                line(out, cx - s * 0.36, y, cx + s * 0.36, y);
                out.push(Cmd::Circle {
                    cx: cx + i as f32 * s * 0.2,
                    cy: y,
                    radius: s * 0.085,
                    fill: Some(Fill::Solid(color)),
                    stroke: None,
                });
            }
        }
        Glyph::List => {
            for i in -1..=1 {
                let y = cy + i as f32 * s * 0.26;
                out.push(Cmd::Circle {
                    cx: cx - s * 0.26,
                    cy: y,
                    radius: s * 0.06,
                    fill: Some(Fill::Solid(color)),
                    stroke: None,
                });
                line(out, cx - s * 0.1, y, cx + s * 0.3, y);
            }
        }
        Glyph::Power => {
            out.push(Cmd::Arc {
                cx,
                cy,
                radius: s * 0.3,
                start_deg: -60.0,
                sweep_deg: 300.0,
                width: w,
                color,
            });
            line(out, cx, cy - s * 0.38, cx, cy - s * 0.02);
        }
        Glyph::Gear => {
            let teeth = s * 0.12;
            for i in 0..8 {
                let angle = (i as f32 * 45.0).to_radians();
                let tx = cx + s * 0.29 * angle.cos();
                let ty = cy + s * 0.29 * angle.sin();
                out.push(Cmd::RoundRect {
                    rect: rect(tx - teeth * 0.5, ty - teeth * 0.5, teeth, teeth),
                    radius: teeth * 0.35,
                    fill: Some(Fill::Solid(color)),
                    stroke: None,
                });
            }
            out.push(Cmd::Circle {
                cx,
                cy,
                radius: s * 0.23,
                fill: None,
                stroke: Some((w, color)),
            });
        }
        Glyph::Play => {
            out.push(Cmd::Poly {
                points: vec![
                    (cx - s * 0.16, cy - s * 0.28),
                    (cx + s * 0.3, cy),
                    (cx - s * 0.16, cy + s * 0.28),
                ],
                color,
            });
        }
        Glyph::Dots => {
            for i in -1..=1 {
                out.push(Cmd::Circle {
                    cx: cx + i as f32 * s * 0.28,
                    cy,
                    radius: s * 0.085,
                    fill: Some(Fill::Solid(color)),
                    stroke: None,
                });
            }
        }
        Glyph::User => {
            out.push(Cmd::Circle {
                cx,
                cy: cy - s * 0.14,
                radius: s * 0.17,
                fill: None,
                stroke: Some((w, color)),
            });
            out.push(Cmd::Arc {
                cx,
                cy: cy + s * 0.26,
                radius: s * 0.3,
                start_deg: 200.0,
                sweep_deg: 140.0,
                width: w,
                color,
            });
        }
        Glyph::Bell => {
            out.push(Cmd::Arc {
                cx,
                cy: cy - s * 0.04,
                radius: s * 0.28,
                start_deg: 180.0,
                sweep_deg: 180.0,
                width: w,
                color,
            });
            line(out, cx - s * 0.34, cy + s * 0.24, cx + s * 0.34, cy + s * 0.24);
            out.push(Cmd::Circle {
                cx,
                cy: cy + s * 0.34,
                radius: s * 0.07,
                fill: Some(Fill::Solid(color)),
                stroke: None,
            });
        }
        Glyph::Music => {
            line(out, cx - s * 0.12, cy + s * 0.1, cx - s * 0.12, cy - s * 0.32);
            line(out, cx + s * 0.28, cy + s * 0.2, cx + s * 0.28, cy - s * 0.22);
            line(out, cx - s * 0.12, cy - s * 0.32, cx + s * 0.28, cy - s * 0.22);
            out.push(Cmd::Circle {
                cx: cx - s * 0.2,
                cy: cy + s * 0.2,
                radius: s * 0.115,
                fill: Some(Fill::Solid(color)),
                stroke: None,
            });
            out.push(Cmd::Circle {
                cx: cx + s * 0.2,
                cy: cy + s * 0.3,
                radius: s * 0.115,
                fill: Some(Fill::Solid(color)),
                stroke: None,
            });
        }
        Glyph::Help => {
            out.push(Cmd::Text {
                text: "?".into(),
                x: cx,
                baseline: cy + s * 0.36,
                size: s * 1.1,
                color,
                weight: Weight::Bold,
                align: Align::Center,
            });
        }
        Glyph::Wifi => {
            for ring in 1..=2 {
                out.push(Cmd::Arc {
                    cx,
                    cy: cy - s * 0.18,
                    radius: s * 0.14 * ring as f32,
                    start_deg: 225.0,
                    sweep_deg: 90.0,
                    width: w * 0.9,
                    color,
                });
            }
            out.push(Cmd::Circle {
                cx,
                cy: cy + s * 0.06,
                radius: s * 0.07,
                fill: Some(Fill::Solid(color)),
                stroke: None,
            });
        }
        Glyph::Check => {
            line(out, cx - s * 0.28, cy, cx - s * 0.06, cy + s * 0.22);
            line(out, cx - s * 0.06, cy + s * 0.22, cx + s * 0.3, cy - s * 0.24);
        }
        Glyph::Diamond => {
            out.push(Cmd::Poly {
                points: vec![
                    (cx, cy - s * 0.26),
                    (cx + s * 0.26, cy),
                    (cx, cy + s * 0.26),
                    (cx - s * 0.26, cy),
                ],
                color,
            });
        }
        Glyph::Home => {
            line(out, cx - s * 0.32, cy + s * 0.02, cx, cy - s * 0.28);
            line(out, cx, cy - s * 0.28, cx + s * 0.32, cy + s * 0.02);
            line(out, cx - s * 0.24, cy - s * 0.04, cx - s * 0.24, cy + s * 0.28);
            line(out, cx + s * 0.24, cy - s * 0.04, cx + s * 0.24, cy + s * 0.28);
            line(out, cx - s * 0.32, cy + s * 0.28, cx + s * 0.32, cy + s * 0.28);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Menu,
    Grid,
    Sort,
    Sliders,
    List,
    Power,
    Gear,
    Play,
    Dots,
    User,
    Bell,
    Music,
    Help,
    Wifi,
    Check,
    Diamond,
    Home,
}

/// A pill with a label, used for Play / page chips.
#[allow(clippy::too_many_arguments)]
pub fn pill(
    out: &mut Vec<Cmd>,
    ctx: &Ctx,
    r: Rect,
    radius: f32,
    fill: Fill,
    label: &str,
    size: f32,
    color: Color,
    weight: Weight,
    glyph: Option<Glyph>,
    glyph_color: Color,
) {
    out.push(Cmd::RoundRect {
        rect: r,
        radius,
        fill: Some(fill),
        stroke: None,
    });
    let cy = r.top() + r.height() * 0.5;
    let text_w = text::measure(label, ctx.d(size), weight);
    let glyph_w = if glyph.is_some() { ctx.d(size) * 0.9 } else { 0.0 };
    let gap = if glyph.is_some() { ctx.d(size) * 0.34 } else { 0.0 };
    let total = text_w + glyph_w + gap;
    let mut x = r.left() + (r.width() - total) * 0.5;
    if let Some(g) = glyph {
        glyph_at(out, ctx, g, (x + glyph_w * 0.5 - ctx.ox) / ctx.s, (cy - ctx.oy) / ctx.s, size * 0.92, glyph_color);
        x += glyph_w + gap;
    }
    out.push(Cmd::Text {
        text: label.into(),
        x,
        baseline: cy + ctx.d(size) * 0.34,
        size: ctx.d(size),
        color,
        weight,
        align: Align::Left,
    });
}

pub fn label(out: &mut Vec<Cmd>, ctx: &Ctx, text_str: &str, x: f32, y: f32, size: f32, color: Color, weight: Weight, align: Align) {
    out.push(Cmd::Text {
        text: text_str.into(),
        x: ctx.x(x),
        baseline: ctx.y(y),
        size: ctx.d(size),
        color,
        weight,
        align,
    });
}

pub fn card_placeholder(out: &mut Vec<Cmd>, _ctx: &Ctx, r: Rect, radius: f32, seed: u32) {
    let hue_a = (seed % 360) as f32;
    let hue_b = (hue_a + 46.0) % 360.0;
    let top = hsv(hue_a, 0.62, 0.42);
    let bottom = hsv(hue_b, 0.70, 0.18);
    out.push(Cmd::RoundRect {
        rect: r,
        radius,
        fill: Some(Fill::Vertical(Gradient { top, bottom })),
        stroke: None,
    });
}

pub fn hsv(h: f32, s: f32, v: f32) -> Color {
    let c = v * s;
    let hp = (h % 360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    Color::from_rgba((r + m).clamp(0.0, 1.0), (g + m).clamp(0.0, 1.0), (b + m).clamp(0.0, 1.0), 1.0)
        .unwrap_or(Palette::PANEL())
}

pub fn dots(out: &mut Vec<Cmd>, ctx: &Ctx, cx: f32, cy: f32, count: usize, active: usize) {
    let gap = 22.0;
    let total = (count.saturating_sub(1)) as f32 * gap;
    let mut x = cx - total * 0.5;
    for i in 0..count {
        let is_active = i == active;
        out.push(Cmd::Circle {
            cx: ctx.x(x),
            cy: ctx.y(cy),
            radius: ctx.d(if is_active { 6.0 } else { 5.0 }),
            fill: Some(Fill::Solid(if is_active { white(240) } else { white(90) })),
            stroke: None,
        });
        x += gap;
    }
}

pub fn spacer(out: &mut Vec<Cmd>, _ctx: &Ctx, _r: Rect) {
    let _ = out;
}

/// Empty image id helper.
pub const NO_IMAGE: ImgId = ImgId::None;

/// Standard size for the frosted chrome.
pub const TOP_BAR_CY: f32 = 96.0;
pub const TOP_BTN: f32 = m::BTN_D;
