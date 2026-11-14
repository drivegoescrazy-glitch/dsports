//! Host renderer: draws the exact same scene as the device and writes a PNG.
//! `cargo run --bin nxs-preview -- [--dual] [--settings] [--w 1920] [--h 1080] out.png`

use nxs::model::{demo_apps, AppEntry, Overlay, State};
use nxs::scene::{rect, Cmd, Images};
use nxs::ui;
use tiny_skia::{Color, Pixmap};

fn art(seed: u32, width: u32, height: u32, hero: bool) -> Pixmap {
    let mut pm = Pixmap::new(width, height).unwrap();
    let hue = seed as f32;
    let top = ui::hsv(hue, 0.55, 0.55);
    let bottom = ui::hsv(hue + 50.0, 0.65, 0.18);
    pm.fill(top);
    let mut cmds = vec![Cmd::RoundRect {
        rect: rect(0.0, 0.0, width as f32, height as f32),
        radius: 0.0,
        fill: Some(nxs::scene::Fill::Vertical(nxs::scene::Gradient { top, bottom })),
        stroke: None,
    }];
    // A few translucent blobs so the art reads as a picture, not a flat panel.
    for i in 0..5 {
        let f = i as f32;
        cmds.push(Cmd::Circle {
            cx: width as f32 * (0.2 + 0.16 * f),
            cy: height as f32 * (0.24 + 0.13 * f),
            radius: height as f32 * (0.22 - 0.025 * f),
            fill: Some(nxs::scene::Fill::Solid(Color::from_rgba8(
                255,
                255,
                255,
                (26.0 - f * 3.0) as u8,
            ))),
            stroke: None,
        });
    }
    let images = Images::new();
    nxs::scene::Painter::new(&images).paint(&mut pm, &cmds);
    if hero {
        // Darken the wide hero so overlaid chrome stays readable.
        let mut wash = Vec::new();
        wash.push(Cmd::RoundRect {
            rect: rect(0.0, 0.0, width as f32, height as f32),
            radius: 0.0,
            fill: Some(nxs::scene::Fill::Vertical(nxs::scene::Gradient {
                top: Color::from_rgba8(6, 10, 22, 40),
                bottom: Color::from_rgba8(6, 10, 22, 120),
            })),
            stroke: None,
        });
        nxs::scene::Painter::new(&images).paint(&mut pm, &wash);
    }
    pm
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut w = 1920u32;
    let mut h = 1080u32;
    let mut dual = false;
    let mut settings = false;
    let mut out = "docs/v102-home.png".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dual" => dual = true,
            "--settings" => settings = true,
            "--w" => {
                i += 1;
                w = args[i].parse().unwrap_or(w);
            }
            "--h" => {
                i += 1;
                h = args[i].parse().unwrap_or(h);
            }
            other => out = other.to_string(),
        }
        i += 1;
    }

    let mut images = Images::new();
    let mut apps: Vec<AppEntry> = demo_apps();
    for app in apps.iter_mut() {
        let boxart = art(app.seed, 600, 900, false);
        app.boxart = images.push(boxart);
        let hero = art(app.seed, 1600, 900, true);
        app.hero = images.push(hero);
    }

    let mut state = State::new(apps);
    state.dual = dual;
    if settings {
        state.overlay = Overlay::Settings;
    }
    // Start on the second card so the coverflow reads like the reference.
    state.selected = 1;
    state.scroll = 1.0;

    let pixmap = nxs::render(&state, w, h, &images).expect("render");
    if let Some(parent) = std::path::Path::new(&out).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::File::create(&out).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    // tiny-skia keeps premultiplied alpha; PNG wants straight alpha.
    let mut straight = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.data().chunks_exact(4) {
        let a = px[3] as u32;
        if a == 0 {
            straight.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            straight.push(((px[0] as u32 * 255) / a) as u8);
            straight.push(((px[1] as u32 * 255) / a) as u8);
            straight.push(((px[2] as u32 * 255) / a) as u8);
            straight.push(a as u8);
        }
    }
    writer.write_image_data(&straight).expect("png data");
    println!("wrote {out} ({}x{})", w, h);
}
