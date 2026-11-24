//! NXS Launcher 1.0.2 — a Rust rewrite of the CardsNX launcher.
//!
//! Everything that draws is Rust: `ui` composes a `Cmd` list in design space,
//! `scene` rasterises it with tiny-skia. On Android the frame is uploaded as a
//! single texture through EGL; on the host the same frame is written to a PNG.

pub mod dual;
pub mod gpu;
pub mod home;
pub mod model;
pub mod pulse;
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

pub const VERSION: &str = "1.1.0";

/// Builds the full command list for the current state and viewport.
pub fn build_cmds(state: &State, width: u32, height: u32) -> Vec<Cmd> {
    let ctx = Ctx::new(width as f32, height as f32);
    let mut out = Vec::with_capacity(256);
    if state.pulse.open {
        home::build(&mut out, &ctx, state);
        let game = state.selected_app().map(|a| a.label.as_str()).unwrap_or("");
        pulse::draw_full(&mut out, &ctx, &state.pulse, state.status.battery, game);
    } else {
        match state.overlay {
            model::Overlay::None => home::build(&mut out, &ctx, state),
            model::Overlay::Settings => {
                home::build(&mut out, &ctx, state);
                settings(&mut out, &ctx, state);
            }
        }
    }
    out
}

/// The cached chrome layer: bar, panel, the dual-screen stack and whichever
/// overlay is on top (settings or PULSE).
pub fn chrome_for(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    home::build_chrome(out, ctx, state);
    if state.pulse.open {
        let game = state.selected_app().map(|a| a.label.as_str()).unwrap_or("");
        pulse::draw_full(out, ctx, &state.pulse, state.status.battery, game);
    } else if state.overlay == model::Overlay::Settings {
        settings(out, ctx, state);
    }
}

/// Rasterises the current state into a fresh pixmap.
pub fn render(state: &State, width: u32, height: u32, images: &Images) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    pixmap.fill(tiny_skia::Color::BLACK);
    let cmds = build_cmds(state, width, height);
    Painter::new(images).paint(&mut pixmap, &cmds);
    Some(pixmap)
}

/// One app's full-screen artwork, already scaled and washed.
struct Layer {
    index: usize,
    art: scene::ImgId,
    w: u32,
    h: u32,
    pixmap: Pixmap,
}

/// A backdrop layer request: which app, its artwork and its colour seed.
#[derive(Clone, Copy, Debug)]
pub struct FrameArt {
    pub index: usize,
    pub art: scene::ImgId,
    pub seed: u32,
}

/// Layout produced under the engine lock; everything else happens outside it,
/// so a touch never waits for a frame to finish painting.
pub struct FramePlan {
    pub width: u32,
    pub height: u32,
    /// The moving part (cards).
    pub cmds: Vec<Cmd>,
    /// The static part (bar, panel); cached by `chrome_key`.
    pub chrome: Vec<Cmd>,
    pub chrome_key: u64,
    pub layers: [Option<FrameArt>; 2],
    pub frac: f32,
    pub images: std::sync::Arc<Images>,
}

/// Caches the backdrop layers. Scrolling then costs a blend of two ready
/// full-screen images instead of rescaling artwork every frame.
pub struct Backdrop {
    layers: Vec<Layer>,
    chrome: Option<(u64, u32, u32, Pixmap)>,
}

impl Backdrop {
    pub fn new() -> Self {
        Backdrop {
            layers: Vec::new(),
            chrome: None,
        }
    }

    /// Rasterises the chrome once per selection/size and reuses it, so the bar
    /// and panel are not repainted on every frame.
    fn ensure_chrome(&mut self, key: u64, w: u32, h: u32, cmds: &[Cmd], images: &Images) -> bool {
        if let Some((k, cw, ch, _)) = &self.chrome {
            if *k == key && *cw == w && *ch == h {
                return false;
            }
        }
        let Some(mut pixmap) = Pixmap::new(w, h) else { return false };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        Painter::new(images).paint(&mut pixmap, cmds);
        self.chrome = Some((key, w, h, pixmap));
        true
    }

    fn chrome(&self) -> Option<&Pixmap> {
        self.chrome.as_ref().map(|(_, _, _, p)| p)
    }

    fn art_of(state: &State, index: usize) -> scene::ImgId {
        state
            .apps
            .get(index)
            .map(|a| if a.hero != scene::ImgId::None { a.hero } else { a.boxart })
            .unwrap_or(scene::ImgId::None)
    }

    /// Builds the layer for one app if it isn't cached at this size.
    fn ensure(&mut self, art: FrameArt, w: u32, h: u32, images: &Images) {
        if self
            .layers
            .iter()
            .any(|l| l.index == art.index && l.art == art.art && l.w == w && l.h == h)
        {
            return;
        }
        let ctx = Ctx::new(w as f32, h as f32);
        let mut cmds = Vec::with_capacity(8);
        home::backdrop(&mut cmds, &ctx, art.art, art.seed);
        let Some(mut pixmap) = Pixmap::new(w, h) else { return };
        pixmap.fill(tiny_skia::Color::BLACK);
        Painter::new(images).paint(&mut pixmap, &cmds);
        self.layers
            .retain(|l| !(l.index == art.index && l.w == w && l.h == h));
        if self.layers.len() >= 4 {
            self.layers.remove(0);
        }
        self.layers.push(Layer {
            index: art.index,
            art: art.art,
            w,
            h,
            pixmap,
        });
    }

    fn get(&self, index: usize, w: u32, h: u32) -> Option<&Pixmap> {
        self.layers
            .iter()
            .find(|l| l.index == index && l.w == w && l.h == h)
            .map(|l| &l.pixmap)
    }
}

impl Default for Backdrop {
    fn default() -> Self {
        Self::new()
    }
}

/// `dst` becomes a cross-fade of itself and `src` at `t`.
fn blend_pixmaps(dst: &mut Pixmap, src: &Pixmap, t: f32) {
    let t16 = (t.clamp(0.0, 1.0) * 256.0) as u32;
    let inv = 256 - t16;
    let d = dst.data_mut();
    let s = src.data();
    for (a, b) in d.iter_mut().zip(s.iter()) {
        *a = ((*b as u32 * t16 + *a as u32 * inv) >> 8) as u8;
    }
}

/// Everything that needs the state lock: layout, layer choice, image handle.
pub fn plan(state: &State, width: u32, height: u32, images: std::sync::Arc<Images>) -> FramePlan {
    let ctx = Ctx::new(width as f32, height as f32);
    let mut cmds = Vec::with_capacity(64);
    home::build_cards(&mut cmds, &ctx, state);
    let mut chrome = Vec::with_capacity(192);
    chrome_for(&mut chrome, &ctx, state);
    let chrome_key = chrome_hash(state);
    let visible = state.visible();
    let mut layers = [None, None];
    let mut frac = 0.0;
    if !visible.is_empty() {
        let focus = state.scroll.clamp(0.0, (visible.len() - 1) as f32);
        let i0 = focus.floor() as usize;
        frac = focus - i0 as f32;
        layers[0] = state.apps.get(visible[i0]).map(|a| FrameArt {
            index: visible[i0],
            art: if a.hero != scene::ImgId::None { a.hero } else { a.boxart },
            seed: a.seed,
        });
        if frac > 0.002 {
            if let Some(next) = visible.get(i0 + 1) {
                layers[1] = state.apps.get(*next).map(|a| FrameArt {
                    index: *next,
                    art: if a.hero != scene::ImgId::None { a.hero } else { a.boxart },
                    seed: a.seed,
                });
            }
        }
    }
    FramePlan {
        width,
        height,
        cmds,
        chrome,
        chrome_key,
        layers,
        frac,
        images,
    }
}

/// Paints a plan outside the state lock, compositing cached backdrop layers.
pub fn paint_plan(plan: &FramePlan, backdrop: &mut Backdrop) -> Option<Pixmap> {
    let mut frame = Pixmap::new(plan.width, plan.height)?;
    frame.fill(tiny_skia::Color::BLACK);
    if let Some(a) = plan.layers[0] {
        backdrop.ensure(a, plan.width, plan.height, &plan.images);
        if let Some(layer) = backdrop.get(a.index, plan.width, plan.height) {
            frame.data_mut().copy_from_slice(layer.data());
        }
        if plan.frac > 0.002 {
            if let Some(b) = plan.layers[1] {
                backdrop.ensure(b, plan.width, plan.height, &plan.images);
                if let Some(layer) = backdrop.get(b.index, plan.width, plan.height) {
                    blend_pixmaps(&mut frame, layer, plan.frac);
                }
            }
        }
    }
    Painter::new(&plan.images).paint(&mut frame, &plan.cmds);
    // The bar and panel come from a cached layer; compositing is a straight
    // premultiplied over, far cheaper than repainting them.
    backdrop.ensure_chrome(
        plan.chrome_key,
        plan.width,
        plan.height,
        &plan.chrome,
        &plan.images,
    );
    if let Some(chrome) = backdrop.chrome() {
        over(&mut frame, chrome);
    }
    Some(frame)
}

/// `src` over `dst`, both premultiplied.
fn over(dst: &mut Pixmap, src: &Pixmap) {
    for (d, s) in dst.data_mut().chunks_exact_mut(4).zip(src.data().chunks_exact(4)) {
        let a = s[3] as u32;
        if a == 0 {
            continue;
        }
        if a == 255 {
            d.copy_from_slice(s);
            continue;
        }
        let inv = 255 - a;
        for c in 0..3 {
            d[c] = (s[c] as u32 + (d[c] as u32 * inv) / 255).min(255) as u8;
        }
        d[3] = (a + (d[3] as u32 * inv) / 255).min(255) as u8;
    }
}

/// Convenience wrapper: plan and paint in one go (preview and tests).
pub fn render_cached(
    state: &State,
    width: u32,
    height: u32,
    images: &Images,
    backdrop: &mut Backdrop,
) -> Option<Pixmap> {
    let plan = plan(state, width, height, std::sync::Arc::new(images.clone()));
    paint_plan(&plan, backdrop)
}

/// Identifies everything the chrome layer draws; changes when it must rebuild.
pub fn chrome_hash(state: &State) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    state.selected.hash(&mut hasher);
    state.page.index().hash(&mut hasher);
    (state.sort as u8).hash(&mut hasher);
    state.dual.hash(&mut hasher);
    state.scroll_speed.to_bits().hash(&mut hasher);
    (state.overlay as u8).hash(&mut hasher);
    state.status.clock.hash(&mut hasher);
    state.status.battery.hash(&mut hasher);
    state.status.free_bytes.hash(&mut hasher);
    state.apps.len().hash(&mut hasher);
    state.displays.count.hash(&mut hasher);
    state.displays.swap.hash(&mut hasher);
    state.pulse.hash_into(&mut hasher);
    // Refresh the selected game's panel art when custom media is replaced.
    if let Some(app) = state.apps.get(state.selected) {
        app.boxart.hash(&mut hasher);
        app.hero.hash(&mut hasher);
        app.icon.hash(&mut hasher);
        app.screen.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn settings(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
    use crate::model::LaunchScreen;
    use crate::scene::{rect, Fill};
    use crate::settings_theme::*;
    use crate::text::{self, Align, Weight};
    use crate::theme::white;
    use crate::ui::label;

    // Dim the home screen behind the sheet.
    out.push(Cmd::RoundRect {
        rect: rect(0.0, 0.0, ctx.w, ctx.h),
        radius: 0.0,
        fill: Some(Fill::Solid(tiny_skia::Color::from_rgba8(4, 6, 14, 190))),
        stroke: None,
    });
    let sheet = ctx.r(300.0, 90.0, 1320.0, 920.0);
    // A settings sheet needs to read as a solid surface: the plain glass fill
    // lets the card gradients bleed through and washes the copy out.
    out.push(Cmd::RoundRect {
        rect: sheet,
        radius: ctx.d(44.0),
        fill: Some(Fill::Solid(tiny_skia::Color::from_rgba8(12, 14, 22, 255))),
        stroke: Some((1.4 * ctx.s, white(24))),
    });
    label(out, ctx, "Settings", 356.0, 172.0, 46.0, white(245), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        "Everything saves as you touch it.",
        356.0,
        206.0,
        22.0,
        white(140),
        Weight::Regular,
        Align::Left,
    );

    // ---------------------------------------------------------- left column
    label(out, ctx, "Launch screen", 356.0, 254.0, 32.0, white(232), Weight::Medium, Align::Left);
    let game = state
        .selected_app()
        .map(|a| a.label.as_str())
        .unwrap_or("No game selected");
    let game_line = text::ellipsize(
        &format!("Per game · {game}"),
        ctx.d(560.0),
        ctx.d(22.0),
        Weight::Regular,
    );
    label(out, ctx, &game_line, 356.0, 290.0, 22.0, white(150), Weight::Regular, Align::Left);

    let current = state
        .selected_app()
        .map(|a| a.screen)
        .unwrap_or(LaunchScreen::Auto);
    let has_game = state.selected_app().is_some();
    let chips = [
        (356.0, 164.0, LaunchScreen::Auto, "Auto"),
        (532.0, 168.0, LaunchScreen::Top, "Top screen"),
        (712.0, 188.0, LaunchScreen::Bottom, "Bottom"),
    ];
    for (x, w, screen, text) in chips {
        settings_chip(
            out,
            ctx,
            x,
            306.0,
            w,
            64.0,
            text,
            current == screen,
            has_game,
        );
    }

    // Which physical panel "top" means on this device.
    let top_display = if state.displays.swap { 2 } else { 1 };
    label(
        out,
        ctx,
        &format!("Top screen = display {top_display}"),
        356.0,
        428.0,
        22.0,
        white(205),
        Weight::Regular,
        Align::Left,
    );
    settings_toggle(out, ctx, ctx.r(836.0, 396.0, 76.0, 40.0), state.displays.swap);
    let displays = if state.displays.count > 1 {
        format!("{} displays detected", state.displays.count)
    } else {
        "Only one display detected".into()
    };
    label(out, ctx, &displays, 356.0, 476.0, 20.0, white(130), Weight::Regular, Align::Left);
    label(
        out,
        ctx,
        "Flip this if games open on the wrong panel.",
        356.0,
        502.0,
        19.0,
        white(105),
        Weight::Regular,
        Align::Left,
    );

    label(out, ctx, "Dual screen", 356.0, 552.0, 32.0, white(232), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        "Cards on top, PULSE and widgets in the bottom screen area.",
        356.0,
        584.0,
        21.0,
        white(150),
        Weight::Regular,
        Align::Left,
    );
    label(
        out,
        ctx,
        "The compact tuner replaces the widgets until you switch back.",
        356.0,
        610.0,
        19.0,
        white(112),
        Weight::Regular,
        Align::Left,
    );
    settings_toggle(out, ctx, ctx.r(836.0, 528.0, 76.0, 40.0), state.dual);

    label(out, ctx, "PULSE", 356.0, 674.0, 32.0, white(232), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        "PServer underclocker · CPU clusters, GPU cap, per-game profiles.",
        356.0,
        706.0,
        20.0,
        white(145),
        Weight::Regular,
        Align::Left,
    );
    settings_button(out, ctx, ctx.r(356.0, 724.0, 280.0, 64.0), "Open PULSE", true);
    label(
        out,
        ctx,
        "The power button in the top bar opens it anywhere.",
        652.0,
        762.0,
        18.0,
        white(105),
        Weight::Regular,
        Align::Left,
    );

    settings_button(out, ctx, ctx.r(356.0, 860.0, 220.0, 76.0), "Done", true);

    // --------------------------------------------------------- right column
    label(out, ctx, "Scroll speed", 1020.0, 254.0, 32.0, white(232), Weight::Medium, Align::Left);
    label(out, ctx, "1x–8x, default 4x.", 1020.0, 286.0, 21.0, white(145), Weight::Regular, Align::Left);
    let speed_ratio = ((state.scroll_speed.clamp(1.0, 8.0) - 1.0) / 7.0).clamp(0.0, 1.0);
    let track = ctx.r(1020.0, 310.0, 440.0, 16.0);
    out.push(Cmd::RoundRect {
        rect: track,
        radius: track.height() * 0.5,
        fill: Some(Fill::Solid(white(38))),
        stroke: None,
    });
    out.push(Cmd::RoundRect {
        rect: ctx.r(1020.0, 310.0, 440.0 * speed_ratio, 16.0),
        radius: ctx.d(8.0),
        fill: Some(Fill::Solid(tiny_skia::Color::from_rgba8(180, 92, 255, 230))),
        stroke: None,
    });
    out.push(Cmd::Circle {
        cx: ctx.x(1020.0 + 440.0 * speed_ratio),
        cy: ctx.y(318.0),
        radius: ctx.d(18.0),
        fill: Some(Fill::Solid(white(250))),
        stroke: Some((1.2 * ctx.s, white(90))),
    });
    label(
        out,
        ctx,
        &format!("{:.1}x", state.scroll_speed),
        1500.0,
        324.0,
        28.0,
        white(245),
        Weight::Medium,
        Align::Left,
    );

    label(out, ctx, "Custom media", 1020.0, 386.0, 32.0, white(232), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        "Pick art for the selected game.",
        1020.0,
        416.0,
        21.0,
        white(145),
        Weight::Regular,
        Align::Left,
    );
    settings_button(out, ctx, ctx.r(1020.0, 432.0, 250.0, 64.0), "Box art", false);
    settings_button(out, ctx, ctx.r(1286.0, 432.0, 250.0, 64.0), "Hero", false);

    label(out, ctx, "Updates", 1020.0, 550.0, 32.0, white(232), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        "Downloads the newest build from GitHub.",
        1020.0,
        580.0,
        20.0,
        white(145),
        Weight::Regular,
        Align::Left,
    );
    settings_button(out, ctx, ctx.r(1020.0, 596.0, 300.0, 64.0), "Check for updates", false);

    label(out, ctx, "PULSE status", 1020.0, 724.0, 28.0, white(232), Weight::Medium, Align::Left);
    let pulse_line = if state.pulse.probe.available {
        format!(
            "{} · {}% target · {}",
            state.pulse.tier.label(),
            state.pulse.target,
            state.pulse.device_name()
        )
    } else if state.pulse.probe.error.is_empty() {
        "Checking this device…".to_string()
    } else {
        state.pulse.probe.error.clone()
    };
    let pulse_line = text::ellipsize(&pulse_line, ctx.d(540.0), ctx.d(20.0), Weight::Regular);
    label(
        out,
        ctx,
        &pulse_line,
        1020.0,
        756.0,
        20.0,
        if state.pulse.probe.available { white(190) } else { crate::theme::Palette::DANGER() },
        Weight::Regular,
        Align::Left,
    );
    label(
        out,
        ctx,
        "Per-game profiles apply automatically on launch.",
        1020.0,
        784.0,
        18.0,
        white(110),
        Weight::Regular,
        Align::Left,
    );

    label(out, ctx, "NXS Launcher 1.1.0", 1020.0, 940.0, 20.0, white(120), Weight::Regular, Align::Left);
    label(
        out,
        ctx,
        "PULSE credit: keiretrogaming/pulse",
        1020.0,
        968.0,
        18.0,
        white(92),
        Weight::Regular,
        Align::Left,
    );
}

mod settings_theme {
    //! Small controls shared only by the settings sheet.
    use crate::scene::{Cmd, Fill, Rect};
    use crate::text::{Align, Weight};
    use crate::theme::white;
    use crate::ui::{label, Ctx};
    use tiny_skia::Color;

    pub fn settings_chip(
        out: &mut Vec<Cmd>,
        ctx: &Ctx,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        text: &str,
        selected: bool,
        enabled: bool,
    ) {
        let r = ctx.r(x, y, w, h);
        out.push(Cmd::RoundRect {
            rect: r,
            radius: r.height() * 0.5,
            fill: Some(Fill::Solid(if selected {
                if enabled {
                    Color::from_rgba8(0xB4, 0x5C, 0xFF, 235)
                } else {
                    white(40)
                }
            } else {
                white(if enabled { 26 } else { 14 })
            })),
            stroke: Some((1.2 * ctx.s, white(if selected { 70 } else { 36 }))),
        });
        out.push(Cmd::Text {
            text: text.into(),
            x: r.left() + r.width() * 0.5,
            baseline: r.top() + r.height() * 0.5 + ctx.d(9.0),
            size: ctx.d(24.0),
            color: if selected {
                white(252)
            } else {
                white(if enabled { 215 } else { 90 })
            },
            weight: Weight::Medium,
            align: Align::Center,
        });
    }

    pub fn settings_toggle(out: &mut Vec<Cmd>, ctx: &Ctx, r: Rect, on: bool) {
        out.push(Cmd::RoundRect {
            rect: r,
            radius: r.height() * 0.5,
            fill: Some(Fill::Solid(if on {
                Color::from_rgba8(0xB4, 0x5C, 0xFF, 235)
            } else {
                white(46)
            })),
            stroke: None,
        });
        out.push(Cmd::Circle {
            cx: if on {
                r.right() - r.height() * 0.5
            } else {
                r.left() + r.height() * 0.5
            },
            cy: r.top() + r.height() * 0.5,
            radius: ctx.d(16.0),
            fill: Some(Fill::Solid(white(250))),
            stroke: None,
        });
    }

    pub fn settings_button(out: &mut Vec<Cmd>, ctx: &Ctx, r: Rect, text: &str, primary: bool) {
        let (fill, stroke, color) = if primary {
            (white(240), white(0), Color::from_rgba8(18, 14, 28, 255))
        } else {
            (white(30), white(44), white(232))
        };
        out.push(Cmd::RoundRect {
            rect: r,
            radius: r.height() * 0.5,
            fill: Some(Fill::Solid(fill)),
            stroke: Some((1.2 * ctx.s, stroke)),
        });
        label(
            out,
            ctx,
            text,
            (r.left() + r.width() * 0.5 - ctx.ox) / ctx.s,
            (r.top() + r.height() * 0.5 + ctx.d(9.0) - ctx.oy) / ctx.s,
            24.0,
            color,
            Weight::Medium,
            Align::Center,
        );
    }
}

#[cfg(test)]
mod chrome_hash_tests {
    use crate::model::{demo_apps, State};

    #[test]
    fn settings_speed_change_invalidates_cached_chrome() {
        let mut state = State::new(demo_apps());
        let before = super::chrome_hash(&state);
        state.scroll_speed = 5.0;
        let after = super::chrome_hash(&state);
        assert_ne!(before, after);
    }
}
