//! The scene the GL renderer draws: a few cached layers plus one quad per card.
//!
//! Everything expensive (artwork scaling, text, glass gradients) is rasterised
//! once into a layer keyed by content. A frame is then only quads, so the CPU
//! cost of a scroll no longer depends on the screen resolution.

use crate::home;
use crate::model::State;
use crate::scene::{rect, Cmd, Images, ImgId, Painter};
use crate::theme::{m, white};
use crate::ui::Ctx;
use tiny_skia::{Color, Pixmap};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LayerKey {
    /// Full-bleed backdrop for one app.
    Bg(usize, ImgId),
    /// Top bar, panel and any overlay; keyed by the state they were built from.
    Chrome(u64),
    /// One app's card artwork at its focused size.
    Card(usize, ImgId),
    /// The white focus rim, drawn over whichever card is centred.
    Rim,
}

#[derive(Clone, Copy)]
pub struct Quad {
    pub key: LayerKey,
    /// Design-space rectangle; the renderer maps it to pixels.
    pub left: f32,
    pub top: f32,
    pub w: f32,
    pub h: f32,
    pub alpha: f32,
}

pub struct Scene {
    /// Backdrops to draw, in order, with their alpha (cross-fade).
    pub bg: Vec<(usize, f32)>,
    pub cards: Vec<Quad>,
    pub rims: Vec<Quad>,
    pub chrome: LayerKey,
}

/// Caps expensive software-rasterised full-screen layers on large displays.
/// They are linearly upscaled by GL; individual card textures stay native-sized.
pub fn layer_pixels(w: u32, h: u32) -> (u32, u32) {
    const MAX_EDGE: f32 = 1600.0;
    let longest = w.max(h).max(1) as f32;
    let scale = (MAX_EDGE / longest).min(1.0);
    (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    )
}

/// Builds an artwork-aware key for a full-screen game's backdrop.
pub fn bg_key(state: &State, index: usize) -> LayerKey {
    let art = state.apps.get(index)
        .map(|app| if app.hero != ImgId::None { app.hero } else { app.boxart })
        .unwrap_or(ImgId::None);
    LayerKey::Bg(index, art)
}

pub fn scene(state: &State) -> Scene {
    let visible = state.visible();
    let mut bg = Vec::with_capacity(2);
    if !visible.is_empty() {
        let focus = state.scroll.clamp(0.0, (visible.len() - 1) as f32);
        let i0 = focus.floor() as usize;
        let frac = focus - i0 as f32;
        bg.push((visible[i0], 1.0));
        if frac > 0.002 {
            if let Some(next) = visible.get(i0 + 1) {
                bg.push((*next, frac));
            }
        }
    }

    let mut cards = Vec::with_capacity(7);
    let mut rims = Vec::with_capacity(2);
    for spec in home::card_specs(state) {
        let art = state.apps.get(spec.app)
            .map(|app| if app.boxart != ImgId::None { app.boxart } else { app.icon })
            .unwrap_or(ImgId::None);
        let quad = Quad {
            key: LayerKey::Card(spec.app, art),
            left: spec.left,
            top: spec.top,
            w: spec.w,
            h: spec.h,
            alpha: spec.alpha,
        };
        if spec.rim > 0.0 {
            rims.push(Quad {
                key: LayerKey::Rim,
                alpha: spec.rim,
                ..quad
            });
        }
        cards.push(quad);
    }

    Scene {
        bg,
        cards,
        rims,
        chrome: LayerKey::Chrome(crate::chrome_hash(state)),
    }
}

/// Size of a card layer in pixels, given the surface size.
pub fn card_pixels(state: &State, w: u32, h: u32) -> (u32, u32) {
    let ctx = Ctx::new(w as f32, h as f32);
    let (cw, ch) = home::card_size(state);
    (
        ((cw * ctx.s).round() as u32).max(8),
        ((ch * ctx.s).round() as u32).max(8),
    )
}

/// Rasterises one layer. Runs on the build thread, never on the render thread
/// for the expensive keys.
pub fn build_layer(state: &State, key: LayerKey, w: u32, h: u32, images: &Images) -> Option<Pixmap> {
    let ctx = Ctx::new(w as f32, h as f32);
    match key {
        LayerKey::Bg(i, art) => {
            let mut pixmap = Pixmap::new(w, h)?;
            pixmap.fill(Color::BLACK);
            let app = state.apps.get(i)?;
            let current_art = if app.hero != ImgId::None { app.hero } else { app.boxart };
            if current_art != art {
                return None;
            }
            let mut cmds = Vec::with_capacity(8);
            home::backdrop(&mut cmds, &ctx, art, app.seed);
            Painter::new(images).paint(&mut pixmap, &cmds);
            Some(pixmap)
        }
        LayerKey::Chrome(_) => {
            let mut pixmap = Pixmap::new(w, h)?;
            pixmap.fill(Color::TRANSPARENT);
            let mut cmds = Vec::with_capacity(256);
            crate::chrome_for(&mut cmds, &ctx, state);
            Painter::new(images).paint(&mut pixmap, &cmds);
            Some(pixmap)
        }
        LayerKey::Card(i, art) => {
            let (pw, ph) = card_pixels(state, w, h);
            let mut pixmap = Pixmap::new(pw, ph)?;
            pixmap.fill(Color::TRANSPARENT);
            let app = state.apps.get(i)?;
            let current_art = if app.boxart != ImgId::None { app.boxart } else { app.icon };
            // A changed box-art/icon ID must never reuse an older cached card.
            if current_art != art {
                return None;
            }
            let local = Ctx {
                s: ctx.s,
                ox: 0.0,
                oy: 0.0,
                w: pw as f32,
                h: ph as f32,
            };
            let (_, ch) = home::card_size(state);
            let radius = m::CARD_RADIUS * (ch / m::CARD_H) * ctx.s;
            let mut cmds = Vec::with_capacity(8);
            home::card_art(&mut cmds, &local, app, radius);
            Painter::new(images).paint(&mut pixmap, &cmds);
            Some(pixmap)
        }
        LayerKey::Rim => {
            let (pw, ph) = card_pixels(state, w, h);
            let mut pixmap = Pixmap::new(pw, ph)?;
            pixmap.fill(Color::TRANSPARENT);
            let (_, ch) = home::card_size(state);
            let radius = m::CARD_RADIUS * (ch / m::CARD_H) * ctx.s;
            let cmds = vec![Cmd::RoundRect {
                rect: rect(0.0, 0.0, pw as f32, ph as f32),
                radius,
                fill: None,
                stroke: Some((2.5 * ctx.s, white(255))),
            }];
            Painter::new(images).paint(&mut pixmap, &cmds);
            Some(pixmap)
        }
    }
}

#[cfg(test)]
mod card_texture_tests {
    use super::{scene, LayerKey};
    use crate::model::{demo_apps, State};
    use crate::scene::ImgId;

    #[test]
    fn card_cache_key_changes_when_box_art_changes() {
        let mut state = State::new(demo_apps());
        state.apps[0].boxart = ImgId::Index(1);
        let before = scene(&state).cards.iter().find(|q| matches!(q.key, LayerKey::Card(0, _))).unwrap().key;
        state.apps[0].boxart = ImgId::Index(2);
        let after = scene(&state).cards.iter().find(|q| matches!(q.key, LayerKey::Card(0, _))).unwrap().key;
        assert_ne!(before, after);
    }

    #[test]
    fn backdrop_cache_key_changes_when_art_changes() {
        let mut state = State::new(demo_apps());
        state.apps[0].boxart = ImgId::Index(4);
        let before = super::bg_key(&state, 0);
        state.apps[0].boxart = ImgId::Index(5);
        let after = super::bg_key(&state, 0);
        assert_ne!(before, after);
    }
}
