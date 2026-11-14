//! Dual-screen mode: the cards keep the top half, and the lower half holds the
//! widget stack — one widget on top, two in the middle, the game preview below.

use crate::model::State;
use crate::scene::{rect, Cmd, Fill, Gradient, ImgId};
use crate::text::{self, Align, Weight};
use crate::theme::white;
use crate::ui::{self, glass, glyph_at, label, Glyph, Ctx};
use tiny_skia::Color;

const SIDE: f32 = 96.0;
const WIDTH: f32 = 1920.0 - SIDE * 2.0;

pub fn stack(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State, top: f32, bottom: f32) {
    let gap = 16.0;
    let total = (bottom - top).max(0.0);
    // 1 top : 2 middle : preview, with the preview keeping the most room.
    let w1 = total * 0.29;
    let w2 = total * 0.25;
    let preview_h = total - w1 - w2 - gap * 2.0;

    activity(out, ctx, rect(ctx.x(SIDE), ctx.y(top), ctx.d(WIDTH), ctx.d(w1)), state);
    let mid_y = top + w1 + gap;
    let mid_w = (WIDTH - gap) * 0.5;
    storage(out, ctx, rect(ctx.x(SIDE), ctx.y(mid_y), ctx.d(mid_w), ctx.d(w2)), state);
    online(
        out,
        ctx,
        rect(ctx.x(SIDE + mid_w + gap), ctx.y(mid_y), ctx.d(mid_w), ctx.d(w2)),
        state,
    );
    preview(
        out,
        ctx,
        rect(ctx.x(SIDE), ctx.y(mid_y + w2 + gap), ctx.d(WIDTH), ctx.d(preview_h)),
        state,
    );
}

fn activity(out: &mut Vec<Cmd>, ctx: &Ctx, r: crate::scene::Rect, state: &State) {
    glass(out, ctx, r, ctx.d(crate::theme::m::WIDGET_RADIUS), 1.0);
    let pad = ctx.d(32.0);
    let top = r.top() + pad;
    out.push(Cmd::Text {
        text: "Play Activity".into(),
        x: r.left() + pad,
        baseline: top + ctx.d(30.0),
        size: ctx.d(36.0),
        color: white(245),
        weight: Weight::Medium,
        align: Align::Left,
    });
    let played = state.apps.iter().map(|a| a.play_ms).max().unwrap_or(0);
    out.push(Cmd::Text {
        text: format!("{} this week", crate::home::format_play_time(played)),
        x: r.left() + pad,
        baseline: top + ctx.d(70.0),
        size: ctx.d(24.0),
        color: white(160),
        weight: Weight::Regular,
        align: Align::Left,
    });

    // Weekly bars, anchored to the right of the row.
    let bars = [0.35f32, 0.55, 0.42, 0.78, 0.62, 0.9, 0.48];
    let bar_w = ctx.d(22.0);
    let gap = ctx.d(12.0);
    let mut x = r.right() - pad - (bar_w + gap) * bars.len() as f32;
    let base = r.bottom() - pad;
    let max_h = (r.height() - pad * 2.0).max(ctx.d(20.0));
    for (i, v) in bars.iter().enumerate() {
        let h = max_h * v;
        let today = i == 6;
        out.push(Cmd::RoundRect {
            rect: rect(x, base - h, bar_w, h),
            radius: ctx.d(8.0),
            fill: Some(Fill::Vertical(Gradient {
                top: if today { white(240) } else { white(120) },
                bottom: if today { white(150) } else { white(40) },
            })),
            stroke: None,
        });
        x += bar_w + gap;
    }
}

fn storage(out: &mut Vec<Cmd>, ctx: &Ctx, r: crate::scene::Rect, _state: &State) {
    glass(out, ctx, r, ctx.d(crate::theme::m::WIDGET_RADIUS), 1.0);
    let pad = ctx.d(30.0);
    out.push(Cmd::Text {
        text: "Storage".into(),
        x: r.left() + pad,
        baseline: r.top() + pad + ctx.d(28.0),
        size: ctx.d(32.0),
        color: white(240),
        weight: Weight::Medium,
        align: Align::Left,
    });
    out.push(Cmd::Text {
        text: "24.3 GB free".into(),
        x: r.right() - pad,
        baseline: r.top() + pad + ctx.d(28.0),
        size: ctx.d(24.0),
        color: white(160),
        weight: Weight::Regular,
        align: Align::Right,
    });
    // Used / free bar along the bottom of the widget.
    let bar = rect(
        r.left() + pad,
        r.bottom() - pad - ctx.d(18.0),
        r.width() - pad * 2.0,
        ctx.d(18.0),
    );
    out.push(Cmd::RoundRect {
        rect: bar,
        radius: bar.height() * 0.5,
        fill: Some(Fill::Solid(white(30))),
        stroke: None,
    });
    out.push(Cmd::RoundRect {
        rect: rect(bar.left(), bar.top(), bar.width() * 0.62, bar.height()),
        radius: bar.height() * 0.5,
        fill: Some(Fill::Vertical(Gradient {
            top: white(232),
            bottom: white(150),
        })),
        stroke: None,
    });
}

fn online(out: &mut Vec<Cmd>, ctx: &Ctx, r: crate::scene::Rect, state: &State) {
    glass(out, ctx, r, ctx.d(crate::theme::m::WIDGET_RADIUS), 1.0);
    let pad = ctx.d(30.0);
    out.push(Cmd::Text {
        text: "Online".into(),
        x: r.left() + pad,
        baseline: r.top() + pad + ctx.d(28.0),
        size: ctx.d(32.0),
        color: white(240),
        weight: Weight::Medium,
        align: Align::Left,
    });
    glyph_at(
        out,
        ctx,
        Glyph::Wifi,
        (r.right() - pad - ctx.d(16.0) - ctx.ox) / ctx.s,
        (r.top() + pad + ctx.d(16.0) - ctx.oy) / ctx.s,
        30.0,
        if state.status.online { white(235) } else { white(110) },
    );
    out.push(Cmd::Text {
        text: if state.status.online { "Wi-Fi  ·  4 friends online" } else { "Offline" }.into(),
        x: r.left() + pad,
        baseline: r.bottom() - pad,
        size: ctx.d(24.0),
        color: white(165),
        weight: Weight::Regular,
        align: Align::Left,
    });
}

fn preview(out: &mut Vec<Cmd>, ctx: &Ctx, r: crate::scene::Rect, state: &State) {
    let radius = ctx.d(crate::theme::m::WIDGET_RADIUS);
    let Some(app) = state.selected_app() else { return };
    let art = if app.hero != ImgId::None {
        app.hero
    } else if app.boxart != ImgId::None {
        app.boxart
    } else {
        ImgId::None
    };
    if art != ImgId::None {
        out.push(Cmd::Image {
            rect: r,
            radius,
            img: art,
            uv: rect(0.0, 0.0, 1.0, 1.0),
            alpha: 1.0,
            tint: Some(Color::from_rgba8(8, 10, 20, 40)),
        });
    } else {
        ui::card_placeholder(out, ctx, r, radius, app.seed);
    }
    // Scrim so the caption reads over any artwork.
    out.push(Cmd::RoundRect {
        rect: rect(r.left(), r.top() + r.height() * 0.45, r.width(), r.height() * 0.55),
        radius: 0.0,
        fill: Some(Fill::Vertical(Gradient {
            top: Color::from_rgba8(0, 0, 0, 0),
            bottom: Color::from_rgba8(0, 0, 0, 190),
        })),
        stroke: None,
    });
    out.push(Cmd::RoundRect {
        rect: r,
        radius,
        fill: None,
        stroke: Some((1.6 * ctx.s, white(46))),
    });
    let pad = ctx.d(36.0);
    out.push(Cmd::Text {
        text: text::ellipsize(&app.label, r.width() * 0.55, ctx.d(40.0), Weight::Medium),
        x: r.left() + pad,
        baseline: r.bottom() - pad - ctx.d(4.0),
        size: ctx.d(40.0),
        color: white(245),
        weight: Weight::Medium,
        align: Align::Left,
    });
    // Play pill, bottom right.
    let play = rect(
        r.right() - pad - ctx.d(240.0),
        r.bottom() - pad - ctx.d(64.0),
        ctx.d(240.0),
        ctx.d(64.0),
    );
    out.push(Cmd::RoundRect {
        rect: play,
        radius: play.height() * 0.5,
        fill: Some(Fill::Solid(white(240))),
        stroke: None,
    });
    glyph_at(
        out,
        ctx,
        Glyph::Play,
        (play.left() + ctx.d(46.0) - ctx.ox) / ctx.s,
        (play.top() + play.height() * 0.5 - ctx.oy) / ctx.s,
        30.0,
        Color::from_rgba8(18, 14, 28, 255),
    );
    out.push(Cmd::Text {
        text: "Play".into(),
        x: play.left() + ctx.d(74.0),
        baseline: play.top() + play.height() * 0.5 + ctx.d(11.0),
        size: ctx.d(28.0),
        color: Color::from_rgba8(18, 14, 28, 255),
        weight: Weight::Medium,
        align: Align::Left,
    });
    let _ = label;
}
