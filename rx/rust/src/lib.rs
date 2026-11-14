//! NXS Launcher 1.0.2 — a Rust rewrite of the CardsNX launcher.
//!
//! Everything that draws is Rust: `ui` composes a `Cmd` list in design space,
//! `scene` rasterises it with tiny-skia. On Android the frame is uploaded as a
//! single texture through EGL; on the host the same frame is written to a PNG.

pub mod dual;
pub mod home;
pub mod model;
pub mod scene;
pub mod text;
pub mod theme;
pub mod ui;

#[cfg(target_os = "android")]
mod android;

use model::State;
use scene::{Cmd, Images, Painter};
use tiny_skia::Pixmap;
use ui::Ctx;

pub const VERSION: &str = "1.0.5";

/// Builds the full command list for the current state and viewport.
pub fn build_cmds(state: &State, width: u32, height: u32) -> Vec<Cmd> {
    let ctx = Ctx::new(width as f32, height as f32);
    let mut out = Vec::with_capacity(256);
    match state.overlay {
        model::Overlay::None => home::build(&mut out, &ctx, state),
        model::Overlay::Settings => {
            home::build(&mut out, &ctx, state);
            settings(&mut out, &ctx, state);
        }
    }
    out
}

/// Rasterises the current state into a fresh pixmap.
pub fn render(state: &State, width: u32, height: u32, images: &Images) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    pixmap.fill(tiny_skia::Color::BLACK);
    let cmds = build_cmds(state, width, height);
    Painter::new(images).paint(&mut pixmap, &cmds);
    Some(pixmap)
}

fn settings(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    use crate::scene::{rect, Fill};
    use crate::text::{Align, Weight};
    use crate::theme::white;
    use crate::ui::{glass, label, Glyph};

    // Dim the home screen behind the sheet.
    out.push(Cmd::RoundRect {
        rect: rect(0.0, 0.0, ctx.w, ctx.h),
        radius: 0.0,
        fill: Some(Fill::Solid(tiny_skia::Color::from_rgba8(4, 6, 14, 170))),
        stroke: None,
    });
    let sheet = ctx.r(320.0, 180.0, 1280.0, 760.0);
    // A settings sheet needs to read as a solid surface: the plain glass fill
    // lets the card gradients bleed through and washes the copy out.
    out.push(Cmd::RoundRect {
        rect: sheet,
        radius: ctx.d(44.0),
        fill: Some(Fill::Solid(tiny_skia::Color::from_rgba8(12, 14, 22, 236))),
        stroke: Some((1.4 * ctx.s, white(24))),
    });
    label(out, ctx, "Settings", 380.0, 260.0, 46.0, white(245), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        "Dual screen",
        380.0,
        340.0,
        34.0,
        white(230),
        Weight::Regular,
        Align::Left,
    );
    label(
        out,
        ctx,
        "Move the game preview to the bottom screen with three widgets above it.",
        380.0,
        382.0,
        26.0,
        white(150),
        Weight::Regular,
        Align::Left,
    );

    // Toggle.
    let track = ctx.r(1440.0, 316.0, 112.0, 60.0);
    let on = state.dual;
    out.push(Cmd::RoundRect {
        rect: track,
        radius: track.height() * 0.5,
        fill: Some(Fill::Solid(if on {
            tiny_skia::Color::from_rgba8(0xB4, 0x5C, 0xFF, 235)
        } else {
            white(46)
        })),
        stroke: None,
    });
    let knob_x = if on { track.right() - track.height() * 0.5 } else { track.left() + track.height() * 0.5 };
    out.push(Cmd::Circle {
        cx: knob_x,
        cy: track.top() + track.height() * 0.5,
        radius: ctx.d(22.0),
        fill: Some(Fill::Solid(white(250))),
        stroke: None,
    });

    label(
        out,
        ctx,
        "Custom media",
        380.0,
        500.0,
        34.0,
        white(230),
        Weight::Regular,
        Align::Left,
    );
    label(
        out,
        ctx,
        "Pick a box art icon or a hero background for the selected game.",
        380.0,
        542.0,
        26.0,
        white(150),
        Weight::Regular,
        Align::Left,
    );
    for (i, text) in ["Box art icon", "Hero background"].iter().enumerate() {
        let r = ctx.r(380.0 + i as f32 * 300.0, 580.0, 280.0, 72.0);
        out.push(Cmd::RoundRect {
            rect: r,
            radius: ctx.d(36.0),
            fill: Some(Fill::Solid(white(30))),
            stroke: Some((1.4 * ctx.s, white(40))),
        });
        label(
            out,
            ctx,
            text,
            (r.left() + r.width() * 0.5) / ctx.s - ctx.ox / ctx.s,
            (r.top() + r.height() * 0.5 + ctx.d(9.0)) / ctx.s - ctx.oy / ctx.s,
            26.0,
            white(235),
            Weight::Medium,
            Align::Center,
        );
    }

    // Updates: one button, a plain label underneath says what it is doing.
    label(
        out,
        ctx,
        "Updates",
        380.0,
        700.0,
        34.0,
        white(230),
        Weight::Regular,
        Align::Left,
    );
    let check = ctx.r(380.0, 716.0, 280.0, 72.0);
    out.push(Cmd::RoundRect {
        rect: check,
        radius: ctx.d(36.0),
        fill: Some(Fill::Solid(white(30))),
        stroke: Some((1.4 * ctx.s, white(40))),
    });
    label(
        out,
        ctx,
        "Check for updates",
        (check.left() + check.width() * 0.5 - ctx.ox) / ctx.s,
        (check.top() + check.height() * 0.5 + ctx.d(9.0) - ctx.oy) / ctx.s,
        26.0,
        white(235),
        Weight::Medium,
        Align::Center,
    );
    label(
        out,
        ctx,
        "Downloads the newest build from GitHub and installs it.",
        676.0,
        760.0,
        24.0,
        white(150),
        Weight::Regular,
        Align::Left,
    );

    let close = ctx.r(380.0, 850.0, 220.0, 76.0);
    out.push(Cmd::RoundRect {
        rect: close,
        radius: close.height() * 0.5,
        fill: Some(Fill::Solid(white(238))),
        stroke: None,
    });
    label(
        out,
        ctx,
        "Done",
        (close.left() + close.width() * 0.5 - ctx.ox) / ctx.s,
        (close.top() + close.height() * 0.5 + ctx.d(10.0) - ctx.oy) / ctx.s,
        28.0,
        tiny_skia::Color::from_rgba8(18, 14, 28, 255),
        Weight::Medium,
        Align::Center,
    );
    let _ = Glyph::Gear;
}
