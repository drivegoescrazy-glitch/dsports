//! Composes the GL scene on the host, so the layer/quads path can be inspected
//! without a device. This is the same order and geometry the renderer uses.

use nxs::gpu::{self, LayerKey};
use nxs::model::{demo_apps, State};
use nxs::scene::{rect, Cmd, Images, Painter};
use tiny_skia::Pixmap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let w: u32 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(2400);
    let h: u32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1080);
    let out = args.get(1).cloned().unwrap_or_else(|| "gpu.png".into());

    let mut images = Images::new();
    let art = |size: u32, seed: u8| {
        let mut p = Pixmap::new(size, size).unwrap();
        for y in 0..size {
            for x in 0..size {
                let px = &mut p.data_mut()[((y * size + x) * 4) as usize..][..4];
                px[0] = (x as u8).wrapping_add(seed);
                px[1] = (y as u8).wrapping_add(seed);
                px[2] = seed.wrapping_mul(3);
                px[3] = 255;
            }
        }
        p
    };
    let hero = images.push(art(1024, 40));
    let card = images.push(art(512, 90));
    let icon = images.push(art(192, 160));
    let mut apps = demo_apps();
    for (i, app) in apps.iter_mut().enumerate() {
        app.hero = if i == 0 { hero } else { nxs::scene::ImgId::None };
        app.boxart = card;
        app.icon = icon;
    }
    let state = State::new(apps);

    let scene = gpu::scene(&state);
    // Layers are pushed into their own image set; the painter then stamps them
    // exactly where the renderer's quads would put them.
    let mut layers = Images::new();
    let mut cmds = Vec::new();
    let mut full = |cmds: &mut Vec<Cmd>, key: LayerKey, x: f32, y: f32, w: f32, h: f32, alpha: f32, layers: &mut Images| {
        if let Some(pixmap) = gpu::build_layer(&state, key, w.max(1.0) as u32, h.max(1.0) as u32, &images) {
            let id = layers.push(pixmap);
            cmds.push(Cmd::Image {
                rect: rect(x, y, w, h),
                radius: 0.0,
                img: id,
                uv: rect(0.0, 0.0, 1.0, 1.0),
                alpha,
                tint: None,
            });
        }
    };

    for (app, alpha) in &scene.bg {
        full(&mut cmds, gpu::bg_key(&state, *app), 0.0, 0.0, w as f32, h as f32, *alpha, &mut layers);
    }
    let ctx = nxs::ui::Ctx::new(w as f32, h as f32);
    for quad in &scene.cards {
        let (cw, ch) = gpu::card_pixels(&state, w, h);
        if let Some(pixmap) = gpu::build_layer(&state, quad.key, w, h, &images) {
            let _ = (cw, ch);
            let id = layers.push(pixmap);
            cmds.push(Cmd::Image {
                rect: rect(ctx.x(quad.left), ctx.y(quad.top), ctx.d(quad.w), ctx.d(quad.h)),
                radius: 0.0,
                img: id,
                uv: rect(0.0, 0.0, 1.0, 1.0),
                alpha: quad.alpha,
                tint: None,
            });
        }
    }
    if !scene.rims.is_empty() {
        if let Some(pixmap) = gpu::build_layer(&state, LayerKey::Rim, w, h, &images) {
            for rim in &scene.rims {
                let id = layers.push(pixmap.clone());
                cmds.push(Cmd::Image {
                    rect: rect(ctx.x(rim.left), ctx.y(rim.top), ctx.d(rim.w), ctx.d(rim.h)),
                    radius: 0.0,
                    img: id,
                    uv: rect(0.0, 0.0, 1.0, 1.0),
                    alpha: rim.alpha,
                    tint: None,
                });
            }
        }
    }
    if let Some(pixmap) = gpu::build_layer(&state, scene.chrome, w, h, &images) {
        let id = layers.push(pixmap);
        cmds.push(Cmd::Image {
            rect: rect(0.0, 0.0, w as f32, h as f32),
            radius: 0.0,
            img: id,
            uv: rect(0.0, 0.0, 1.0, 1.0),
            alpha: 1.0,
            tint: None,
        });
    }

    let mut frame = Pixmap::new(w, h).unwrap();
    frame.fill(tiny_skia::Color::BLACK);
    Painter::new(&layers).paint(&mut frame, &cmds);
    frame.save_png(&out).unwrap();
    println!(
        "wrote {out} ({w}x{h}) — {} bg, {} cards, rim {}, chrome",
        scene.bg.len(),
        scene.cards.len(),
        !scene.rims.is_empty()
    );
}
