//! The CardsNX home screen: hero wash, top bar, coverflow card row, glass panel.

use crate::model::{Action, Page, State};
use crate::scene::{rect, Cmd, Fill, Gradient, ImgId, Rect};
use crate::text::{self, Align, Weight};
use crate::theme::{m, white, Palette};
use crate::ui::{self, dots, glyph_at, glass, label, pill, Glyph, Ctx};
use tiny_skia::Color;

pub const CARD_W: f32 = m::CARD_H * m::CARD_RATIO;

/// Draws the hero backdrop: the focused game's artwork, full bleed, plus a
/// bottom-up wash so the chrome stays legible.
pub fn hero(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    let full = rect(0.0, 0.0, ctx.w, ctx.h);
    if let Some(app) = state.selected_app() {
        let hero_img = if app.hero != ImgId::None { app.hero } else { app.boxart };
        if hero_img != ImgId::None {
            out.push(Cmd::Image {
                rect: full,
                radius: 0.0,
                img: hero_img,
                uv: rect(0.0, 0.0, 1.0, 1.0),
                alpha: 1.0,
                tint: Some(Color::from_rgba8(18, 26, 46, 46)),
            });
        } else {
            // Procedural fallback so the screen never looks unfinished.
            out.push(Cmd::RoundRect {
                rect: full,
                radius: 0.0,
                fill: Some(Fill::Vertical(Gradient {
                    top: ui::hsv(app.seed as f32, 0.55, 0.38),
                    bottom: ui::hsv(app.seed as f32 + 40.0, 0.62, 0.12),
                })),
                stroke: None,
            });
        }
    }
    // Legibility wash: dark at the top bar, clear in the middle, darker at the panel.
    out.push(Cmd::RoundRect {
        rect: rect(0.0, 0.0, ctx.w, ctx.y(210.0)),
        radius: 0.0,
        fill: Some(Fill::Vertical(Gradient {
            top: Color::from_rgba8(4, 6, 14, 132),
            bottom: Color::from_rgba8(4, 6, 14, 0),
        })),
        stroke: None,
    });
    out.push(Cmd::RoundRect {
        rect: rect(0.0, ctx.y(560.0), ctx.w, ctx.h - ctx.y(560.0)),
        radius: 0.0,
        fill: Some(Fill::Vertical(Gradient {
            top: Color::from_rgba8(4, 6, 14, 0),
            bottom: Color::from_rgba8(4, 6, 14, 150),
        })),
        stroke: None,
    });
}

pub fn top_bar(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    let cy = ui::TOP_BAR_CY;
    let d = 112.0;
    // Left cluster: menu, grid, sort.
    ui::circle_button(out, ctx, 104.0, cy, d, Glyph::Menu, false);
    ui::circle_button(out, ctx, 236.0, cy, d, Glyph::Grid, false);
    ui::circle_button(out, ctx, 368.0, cy, d, Glyph::Sort, false);

    // Page title pill with the profile bubble, like the reference's "Title 119".
    let pill_r = ctx.r(440.0, cy - 56.0, 322.0, 112.0);
    glass(out, ctx, pill_r, ctx.d(56.0), 0.9);
    label(out, ctx, state.page.title(), 472.0, cy + 12.0, 40.0, Palette::TEXT(), Weight::Medium, Align::Left);
    out.push(Cmd::Circle {
        cx: ctx.x(724.0),
        cy: ctx.y(cy),
        radius: ctx.d(34.0),
        fill: Some(Fill::Vertical(Gradient {
            top: Color::from_rgba8(0xFF, 0x6B, 0x8A, 255),
            bottom: Color::from_rgba8(0xC0, 0x2E, 0x50, 255),
        })),
        stroke: Some((1.5 * ctx.s, white(60))),
    });
    glyph_at(out, ctx, Glyph::User, 724.0, cy, 40.0, white(240));

    // Right cluster, right-aligned: bell, music, power, gear as circles...
    ui::circle_button(out, ctx, 1800.0, cy, d, Glyph::Gear, false);
    ui::circle_button(out, ctx, 1680.0, cy, d, Glyph::Power, false);
    ui::circle_button(out, ctx, 1560.0, cy, d, Glyph::Music, false);
    ui::circle_button(out, ctx, 1440.0, cy, d, Glyph::Bell, false);
    ui::circle_button(out, ctx, 1320.0, cy, 96.0, Glyph::Help, false);

    // ...and the status text before them.
    status_readout(out, ctx, state);
}

fn status_readout(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    let cy = ui::TOP_BAR_CY;
    // Clock.
    label(out, ctx, &state.status.clock, 990.0, cy + 12.0, 36.0, white(240), Weight::Medium, Align::Right);
    // Battery: pill outline with a level bar and the percentage.
    let bx = 1006.0;
    let by = cy - 18.0;
    out.push(Cmd::RoundRect {
        rect: ctx.r(bx, by, 52.0, 36.0),
        radius: ctx.d(10.0),
        fill: None,
        stroke: Some((2.2 * ctx.s, white(200))),
    });
    let level = (state.status.battery.min(100) as f32) / 100.0;
    let bar_color = if state.status.battery <= 15 {
        Palette::DANGER()
    } else if state.status.battery <= 30 {
        Palette::WARN()
    } else {
        Palette::GOOD()
    };
    out.push(Cmd::RoundRect {
        rect: ctx.r(bx + 5.0, by + 5.0, 42.0 * level, 26.0),
        radius: ctx.d(6.0),
        fill: Some(Fill::Solid(bar_color)),
        stroke: None,
    });
    out.push(Cmd::RoundRect {
        rect: ctx.r(bx + 54.0, by + 11.0, 5.0, 14.0),
        radius: ctx.d(2.0),
        fill: Some(Fill::Solid(white(200))),
        stroke: None,
    });
    label(
        out,
        ctx,
        &format!("{}%", state.status.battery),
        1074.0,
        cy + 12.0,
        32.0,
        white(232),
        Weight::Medium,
        Align::Left,
    );
    // Free storage, right-aligned so it never reaches the icon row.
    label(
        out,
        ctx,
        &format_bytes(state.status.free_bytes),
        1258.0,
        cy + 12.0,
        32.0,
        white(220),
        Weight::Regular,
        Align::Right,
    );
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    // Decimal units, the way Android's storage screen reports them.
    let gb = bytes as f64 / 1_000_000_000.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        let mb = bytes as f64 / 1_000_000.0;
        format!("{mb:.0} MB")
    }
}

pub fn format_play_time(ms: u64) -> String {
    if ms < 60_000 {
        return "0m".into();
    }
    let minutes = ms / 60_000;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}h {}m", minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

/// The coverflow row. Returns the focused card rect so the caller can wire hit boxes.
pub fn cards(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State, top: f32, height: f32) -> Rect {
    let visible = state.visible();
    let cx = 960.0;
    let h = height;
    let w = h * m::CARD_RATIO;
    let radius = m::CARD_RADIUS * (h / m::CARD_H);

    let pos = visible.iter().position(|i| *i == state.selected).unwrap_or(0) as f32;
    let focus = state.scroll;

    // Everything that can be on screen, far neighbours included.
    for offset in -3i32..=3 {
        let idx = pos as i32 + offset;
        if idx < 0 || idx as usize >= visible.len() {
            continue;
        }
        let app_index = visible[idx as usize];
        let app = &state.apps[app_index];
        let dist = (idx as f32 - focus) as f32;
        let abs = dist.abs();
        if abs > 3.2 {
            continue;
        }
        // Focus falls off with distance: the selected card is full size and lifted.
        let t = (1.0 - abs.min(1.0)) as f32;
        let scale = m::CARD_BACK_SCALE + (1.0 - m::CARD_BACK_SCALE) * smooth(t);
        let card_h = h * scale;
        let card_w = w * scale;
        let drop = m::CARD_BACK_DROP * (1.0 - smooth(t));
        let center_x = cx + dist * (w + m::CARD_GAP) * 0.98;
        let card = rect(
            ctx.x(center_x - card_w * 0.5),
            ctx.y(top + drop + (h - card_h) * 0.0),
            ctx.d(card_w),
            ctx.d(card_h),
        );
        if card.right() < 0.0 || card.left() > ctx.w {
            continue;
        }
        let faded = abs > 0.55;
        let art = if app.boxart != ImgId::None { app.boxart } else { app.icon };
        if art != ImgId::None {
            out.push(Cmd::Image {
                rect: card,
                radius: ctx.d(radius * scale),
                img: art,
                uv: rect(0.0, 0.0, 1.0, 1.0),
                alpha: if faded { 0.92 } else { 1.0 },
                tint: None,
            });
        } else {
            ui::card_placeholder(out, ctx, card, ctx.d(radius * scale), app.seed);
            // A soft read of the name so empty cards still read as artwork.
            let seed_hue = app.seed as f32;
            out.push(Cmd::RoundRect {
                rect: rect(card.left(), card.top() + card.height() * 0.55, card.width(), card.height() * 0.45),
                radius: 0.0,
                fill: Some(Fill::Vertical(Gradient {
                    top: Color::from_rgba8(0, 0, 0, 0),
                    bottom: Color::from_rgba8(0, 0, 0, 120),
                })),
                stroke: None,
            });
            let _ = seed_hue;
            out.push(Cmd::Text {
                text: text::ellipsize(&app.label, card.width() * 0.8, ctx.d(34.0 * scale), Weight::Medium),
                x: card.left() + card.width() * 0.5,
                baseline: card.top() + card.height() * 0.9,
                size: ctx.d(34.0 * scale),
                color: white(236),
                weight: Weight::Medium,
                align: Align::Center,
            });
        }
        // The focused card gets the thin white rim and a lift shadow.
        if abs < 0.5 {
            let focus_alpha = (1.0 - abs * 2.0).clamp(0.0, 1.0);
            out.push(Cmd::RoundRect {
                rect: card,
                radius: ctx.d(radius * scale),
                fill: None,
                stroke: Some((2.5 * ctx.s, white((190.0 * focus_alpha) as u8))),
            });
            out.push(Cmd::RoundRect {
                rect: rect(card.left() - 2.0, card.top() + 6.0, card.width() + 4.0, card.height()),
                radius: ctx.d(radius * scale + 2.0),
                fill: None,
                stroke: Some((2.0 * ctx.s, white((40.0 * focus_alpha) as u8))),
            });
        }
    }

    // Focused card rect for hit testing.
    let card = rect(ctx.x(cx - w * 0.5), ctx.y(top), ctx.d(w), ctx.d(h));
    card
}

#[inline]
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn panel(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State, top: f32) -> Rect {
    let r = ctx.r((1920.0 - m::PANEL_W) * 0.5, top, m::PANEL_W, m::PANEL_H);
    glass(out, ctx, r, ctx.d(m::PANEL_RADIUS), 1.0);

    let Some(app) = state.selected_app() else { return r };

    // Artwork thumb.
    let thumb = rect(r.left() + ctx.d(36.0), r.top() + ctx.d(34.0), ctx.d(96.0), ctx.d(96.0));
    let art = if app.boxart != ImgId::None { app.boxart } else { app.icon };
    if art != ImgId::None {
        out.push(Cmd::Image {
            rect: thumb,
            radius: ctx.d(18.0),
            img: art,
            uv: rect(0.0, 0.0, 1.0, 1.0),
            alpha: 1.0,
            tint: None,
        });
    } else {
        ui::card_placeholder(out, ctx, thumb, ctx.d(18.0), app.seed);
    }
    out.push(Cmd::RoundRect {
        rect: thumb,
        radius: ctx.d(18.0),
        fill: None,
        stroke: Some((1.5 * ctx.s, white(70))),
    });

    let text_left = 1920.0 / 2.0 - m::PANEL_W / 2.0 + 150.0;
    let title_w = ctx.d(m::PANEL_W - 300.0);
    let title = text::ellipsize(&app.label, title_w, ctx.d(44.0), Weight::Medium);
    label(out, ctx, &title, text_left, top + 88.0, 44.0, Palette::TEXT(), Weight::Medium, Align::Left);

    let played = format_play_time(app.play_ms);
    let last = if app.last_played_min == 0 {
        "Never played".to_string()
    } else if app.last_played_min < 60 {
        format!("{}m ago", app.last_played_min)
    } else if app.last_played_min < 1440 {
        format!("{}h ago", app.last_played_min / 60)
    } else {
        format!("{}d ago", app.last_played_min / 1440)
    };
    label(
        out,
        ctx,
        &format!("{played}    {last}"),
        text_left,
        top + 138.0,
        28.0,
        Palette::TEXT_DIM(),
        Weight::Regular,
        Align::Left,
    );

    // Version chip.
    let chip_w = 150.0;
    out.push(Cmd::RoundRect {
        rect: ctx.r(text_left, top + 158.0, chip_w, 44.0),
        radius: ctx.d(22.0),
        fill: Some(Fill::Solid(Color::from_rgba8(0, 0, 0, 70))),
        stroke: Some((1.2 * ctx.s, white(36))),
    });
    glyph_at(out, ctx, Glyph::Diamond, text_left + 26.0, top + 180.0, 24.0, Palette::AMBER());
    label(
        out,
        ctx,
        &format!("v{}", app.version),
        text_left + 48.0,
        top + 189.0,
        26.0,
        Palette::AMBER(),
        Weight::Medium,
        Align::Left,
    );

    // Play pill.
    let play = ctx.r(1920.0 / 2.0 - m::PANEL_W / 2.0 + 36.0, top + 196.0, 300.0, 76.0);
    pill(
        out,
        ctx,
        play,
        ctx.d(38.0),
        Fill::Solid(white(242)),
        "Play",
        32.0,
        Color::from_rgba8(18, 14, 28, 255),
        Weight::Medium,
        Some(Glyph::Play),
        Color::from_rgba8(18, 14, 28, 255),
    );

    // Card options bubble.
    let more_c = (1920.0 / 2.0 - m::PANEL_W / 2.0 + 392.0, top + 234.0);
    out.push(Cmd::Circle {
        cx: ctx.x(more_c.0),
        cy: ctx.y(more_c.1),
        radius: ctx.d(38.0),
        fill: Some(Fill::Solid(Color::from_rgba8(255, 255, 255, 26))),
        stroke: Some((1.4 * ctx.s, white(46))),
    });
    glyph_at(out, ctx, Glyph::Dots, more_c.0, more_c.1, 30.0, white(232));

    // View toggles on the right edge of the panel.
    let right = 1920.0 / 2.0 + m::PANEL_W / 2.0;
    for (i, glyph) in [Glyph::Grid, Glyph::List].iter().enumerate() {
        let cx = right - 72.0 - i as f32 * 86.0;
        out.push(Cmd::Circle {
            cx: ctx.x(cx),
            cy: ctx.y(top + 234.0),
            radius: ctx.d(38.0),
            fill: Some(Fill::Solid(Color::from_rgba8(255, 255, 255, 26))),
            stroke: Some((1.4 * ctx.s, white(46))),
        });
        glyph_at(out, ctx, *glyph, cx, top + 234.0, 28.0, white(220));
    }

    let visible = state.visible();
    let active = visible.iter().position(|i| *i == state.selected).unwrap_or(0);
    if visible.len() > 1 {
        dots(out, ctx, right - 240.0, top + 252.0, visible.len().min(6), active.min(5));
    }
    r
}

/// Whole home screen. `dual` compresses the card row into the top half.
pub fn build(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    hero(out, ctx, state);
    top_bar(out, ctx, state);
    if state.dual {
        let card_top = 148.0;
        let card_h = 372.0;
        cards(out, ctx, state, card_top, card_h);
        crate::dual::stack(out, ctx, state, 528.0, 1046.0);
    } else {
        cards(out, ctx, state, m::CARD_ROW_TOP, m::CARD_H);
        panel(out, ctx, state, m::PANEL_TOP);
    }
}

/// Pages as a faint tab strip under the top bar, only used in settings.
pub fn page_tabs(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    let mut x = 96.0;
    for page in Page::ALL {
        let active = page == state.page;
        let w = 148.0;
        out.push(Cmd::RoundRect {
            rect: ctx.r(x, 196.0, w, 56.0),
            radius: ctx.d(28.0),
            fill: Some(Fill::Solid(if active { white(46) } else { white(16) })),
            stroke: None,
        });
        label(
            out,
            ctx,
            page.title(),
            x + w * 0.5,
            234.0,
            26.0,
            if active { white(245) } else { white(160) },
            Weight::Medium,
            Align::Center,
        );
        x += w + 12.0;
    }
}

pub fn hit_actions(state: &State, _ctx: &Ctx) -> Vec<(crate::scene::Rect, Action)> {
    let hits = Vec::new();
    if state.dual {
        return hits;
    }
    hits
}
