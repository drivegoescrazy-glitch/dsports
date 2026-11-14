//! Times the rasteriser per command type, so the slow primitive is obvious.

use nxs::model::{demo_apps, State};
use nxs::scene::{Cmd, Fill, Images, ImgId, Painter};
use tiny_skia::Pixmap;

fn art(size: u32, seed: u8) -> Pixmap {
    let mut p = Pixmap::new(size, size).unwrap();
    for y in 0..size {
        for x in 0..size {
            let px = &mut p.data_mut()[((y * size + x) * 4) as usize..][..4];
            px[0] = (x as u8).wrapping_add(seed);
            px[1] = (y as u8).wrapping_add(seed);
            px[2] = seed;
            px[3] = 255;
        }
    }
    p
}

fn time(label: &str, frames: usize, mut f: impl FnMut()) {
    let started = std::time::Instant::now();
    for _ in 0..frames {
        f();
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("{label:<28} {ms:7.2} ms");
}

fn micro() {
    use tiny_skia::{FilterQuality, PixmapPaint, Transform};
    let mut target = Pixmap::new(1280, 720).unwrap();
    let big = art(1024, 10);
    let small = art(512, 20);
    let paint = PixmapPaint {
        opacity: 1.0,
        quality: FilterQuality::Bilinear,
        ..Default::default()
    };

    time("blit 1024 -> 1280x720 scale", 6, || {
        let t = Transform::from_scale(1.25, 1.25);
        target.draw_pixmap(0, 0, big.as_ref(), &paint, t, None);
    });
    time("blit 512 -> 360x531 scale", 6, || {
        let t = Transform::from_scale(360.0 / 512.0, 531.0 / 512.0)
            .pre_translate(-100.0, -100.0);
        target.draw_pixmap(0, 0, small.as_ref(), &paint, t, None);
    });
    time("blit 1280x720 1:1", 6, || {
        let src = Pixmap::new(1280, 720).unwrap();
        target.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::identity(), None);
    });
    time("fill_rect full screen x6", 6, || {
        let mut p = tiny_skia::Paint::default();
        p.set_color(tiny_skia::Color::from_rgba8(10, 20, 30, 120));
        for i in 0..6 {
            let r = tiny_skia::Rect::from_xywh(0.0, i as f32 * 100.0, 1280.0, 120.0).unwrap();
            target.fill_rect(r, &p, Transform::identity(), None);
        }
    });
}

fn main() {
    micro();
    println!();
    let mut images = Images::new();
    let hero = images.push(art(1024, 40));
    let card = images.push(art(512, 90));
    let icon = images.push(art(192, 160));

    let mut apps = demo_apps();
    for (i, app) in apps.iter_mut().enumerate() {
        app.hero = if i == 0 { hero } else { ImgId::None };
        app.boxart = card;
        app.icon = icon;
    }
    let state = State::new(apps.clone());
    let (w, h) = (1280u32, 720u32);
    let cmds = nxs::build_cmds(&state, w, h);

    let mut counts = std::collections::BTreeMap::new();
    for c in &cmds {
        let key = match c {
            Cmd::RoundRect { fill, .. } => match fill {
                Some(Fill::Solid(_)) => "RoundRect solid",
                Some(Fill::Vertical(_)) => "RoundRect gradient",
                None => "RoundRect stroke",
            },
            Cmd::Circle { .. } => "Circle",
            Cmd::Image { .. } => "Image",
            Cmd::Text { .. } => "Text",
            Cmd::Line { .. } => "Line",
            Cmd::Poly { .. } => "Poly",
            Cmd::Arc { .. } => "Arc",
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    println!("{counts:?}\n");

    let painter = Painter::new(&images);
    let mut pixmap = Pixmap::new(w, h).unwrap();

    time("blank fill", 10, || {
        let mut p = Pixmap::new(w, h).unwrap();
        p.fill(tiny_skia::Color::BLACK);
        std::hint::black_box(&p);
    });
    time("everything", 10, || {
        let mut p = Pixmap::new(w, h).unwrap();
        painter.paint(&mut p, &cmds);
        std::hint::black_box(&p);
    });

    let groups: [(&str, fn(&Cmd) -> bool); 6] = [
        ("Text only", |c| matches!(c, Cmd::Text { .. })),
        ("Images only", |c| matches!(c, Cmd::Image { .. })),
        ("Gradient fills only", |c| {
            matches!(c, Cmd::RoundRect { fill: Some(Fill::Vertical(_)), .. })
        }),
        ("Solid fills only", |c| {
            matches!(c, Cmd::RoundRect { fill: Some(Fill::Solid(_)), .. })
        }),
        ("Circles only", |c| matches!(c, Cmd::Circle { .. })),
        ("Lines+polys", |c| matches!(c, Cmd::Line { .. } | Cmd::Poly { .. })),
    ];
    for (label, keep) in groups {
        let subset: Vec<Cmd> = cmds.iter().filter(|c| keep(c)).cloned().collect();
        time(label, 6, || {
            pixmap.fill(tiny_skia::Color::BLACK);
            painter.paint(&mut pixmap, &subset);
        });
    }

    // The cached path must compose to exactly what the direct painter does.
    {
        let direct = nxs::render(&state, w, h, &images).unwrap();
        let mut cache = nxs::Backdrop::new();
        let plan = nxs::plan(&state, w, h, std::sync::Arc::new(images.clone()));
        let _ = nxs::paint_plan(&plan, &mut cache);
        let cached = nxs::paint_plan(&plan, &mut cache).unwrap();
        let diff: u32 = direct
            .data()
            .iter()
            .zip(cached.data())
            .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs())
            .sum();
        let mean = diff as f64 / direct.data().len() as f64;
        println!("cached vs direct mean channel diff: {mean:.3}\n");
    }

    // Cached path: the backdrop is built once, then each frame blends two cached
    // layers and paints the foreground.
    let images = std::sync::Arc::new(images);
    for (w, h) in [(1024u32, 576u32), (1280, 720), (1920, 864), (2400, 1080)] {
        let mut backdrop = nxs::Backdrop::new();
        let mut state = State::new(apps.clone());
        let warm = nxs::plan(&state, w, h, images.clone());
        let _ = nxs::paint_plan(&warm, &mut backdrop);
        time(&format!("cached {w}x{h} idle"), 8, || {
            state.tick(16.0);
            let plan = nxs::plan(&state, w, h, images.clone());
            let _ = nxs::paint_plan(&plan, &mut backdrop);
        });
        state.scroll_target = 3.0;
        state.scroll = 2.4;
        time(&format!("cached {w}x{h} mid-fade"), 8, || {
            let plan = nxs::plan(&state, w, h, images.clone());
            let _ = nxs::paint_plan(&plan, &mut backdrop);
        });
    }
}
