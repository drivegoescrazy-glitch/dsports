//! NXS Launcher 1.0.2 — a Rust rewrite of the CardsNX launcher.
//!
//! Everything that draws is Rust: `ui` composes a `Cmd` list in design space,
//! `scene` rasterises it with tiny-skia. On Android the frame is uploaded as a
//! single texture through EGL; on the host the same frame is written to a PNG.

pub mod dual;
pub mod gpu;
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

pub const VERSION: &str = "1.0.9";

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
    home::build_chrome(&mut chrome, &ctx, state);
    if state.overlay == model::Overlay::Settings {
        settings(&mut chrome, &ctx, state);
    }
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
    (state.overlay as u8).hash(&mut hasher);
    state.status.clock.hash(&mut hasher);
    state.status.battery.hash(&mut hasher);
    state.status.free_bytes.hash(&mut hasher);
    state.apps.len().hash(&mut hasher);
    // Refresh the selected game's panel art when custom media is replaced.
    if let Some(app) = state.apps.get(state.selected) {
        app.boxart.hash(&mut hasher);
        app.hero.hash(&mut hasher);
        app.icon.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn settings(out: &mut Vec<Cmd>, ctx: &Ctx, state: &State) {
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

    // Scroll speed: a slider from 1x to 8x, with 4x as the default.
    label(out, ctx, "Scroll speed", 380.0, 440.0, 34.0, white(230), Weight::Regular, Align::Left);
    label(out, ctx, "Choose 1x–8x. Default: 4x.", 380.0, 476.0, 24.0, white(150), Weight::Regular, Align::Left);
    let speed_ratio = ((state.scroll_speed.clamp(1.0, 8.0) - 1.0) / 7.0).clamp(0.0, 1.0);
    let speed_track = ctx.r(830.0, 420.0, 580.0, 16.0);
    out.push(Cmd::RoundRect {
        rect: speed_track,
        radius: ctx.d(8.0),
        fill: Some(Fill::Solid(white(40))),
        stroke: None,
    });
    out.push(Cmd::RoundRect {
        rect: ctx.r(830.0, 420.0, 580.0 * speed_ratio, 16.0),
        radius: ctx.d(8.0),
        fill: Some(Fill::Solid(tiny_skia::Color::from_rgba8(180, 92, 255, 230))),
        stroke: None,
    });
    out.push(Cmd::Circle {
        cx: ctx.x(830.0 + 580.0 * speed_ratio),
        cy: ctx.y(428.0),
        radius: ctx.d(18.0),
        fill: Some(Fill::Solid(white(250))),
        stroke: Some((1.2 * ctx.s, white(90))),
    });
    label(out, ctx, &format!("{:.1}x", state.scroll_speed), 1460.0, 440.0, 30.0, white(245), Weight::Medium, Align::Left);

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
