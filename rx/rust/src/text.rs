//! Text: the launcher ships no font, it borrows the platform one (Roboto on
//! Android, DejaVu on the preview host) and rasterises outlines straight into
//! the frame. No atlas, no system text APIs, no Kotlin.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use std::sync::OnceLock;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

pub const ANDROID_FONTS: &[(&str, Weight)] = &[
    ("/system/fonts/Roboto-Regular.ttf", Weight::Regular),
    ("/system/fonts/Roboto-Medium.ttf", Weight::Medium),
    ("/system/fonts/Roboto-Bold.ttf", Weight::Bold),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Regular,
    Medium,
    Bold,
}

impl Weight {
    fn host_paths(self) -> &'static [&'static str] {
        match self {
            Weight::Regular => &[
                "/opt/freestyle/python/lib/python3.12/site-packages/matplotlib/mpl-data/fonts/ttf/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
            ],
            Weight::Medium => &[
                "/opt/freestyle/python/lib/python3.12/site-packages/matplotlib/mpl-data/fonts/ttf/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ],
            Weight::Bold => &[
                "/opt/freestyle/python/lib/python3.12/site-packages/matplotlib/mpl-data/fonts/ttf/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ],
        }
    }
}

pub struct Fonts {
    regular: FontVec,
    medium: Option<FontVec>,
    bold: Option<FontVec>,
}

static FONTS: OnceLock<Fonts> = OnceLock::new();

fn load_first(paths: &[&str]) -> Option<FontVec> {
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = FontVec::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }
    None
}

impl Fonts {
    pub fn get() -> &'static Fonts {
        FONTS.get_or_init(|| {
            #[cfg(target_os = "android")]
            let (regular, medium, bold) = {
                let r = load_first(&["/system/fonts/Roboto-Regular.ttf"])
                    .expect("Android always ships Roboto-Regular.ttf");
                let m = load_first(&["/system/fonts/Roboto-Medium.ttf"]);
                let b = load_first(&["/system/fonts/Roboto-Bold.ttf"]);
                (r, m, b)
            };
            #[cfg(not(target_os = "android"))]
            let (regular, medium, bold) = {
                let r = load_first(Weight::Regular.host_paths()).expect("host font");
                let m = load_first(Weight::Medium.host_paths());
                let b = load_first(Weight::Bold.host_paths());
                (r, m, b)
            };
            Fonts { regular, medium, bold }
        })
    }

    fn font(&self, weight: Weight) -> &FontVec {
        match weight {
            Weight::Regular => &self.regular,
            Weight::Medium => self.medium.as_ref().unwrap_or(&self.regular),
            Weight::Bold => self
                .bold
                .as_ref()
                .or(self.medium.as_ref())
                .unwrap_or(&self.regular),
        }
    }
}

/// Horizontal advance of `text`, including kerning.
pub fn measure(text: &str, size: f32, weight: Weight) -> f32 {
    let fonts = Fonts::get();
    let font = fonts.font(weight);
    let scaled = font.as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut prev = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            width += scaled.kern(p, id);
        }
        width += scaled.h_advance(id);
        prev = Some(id);
    }
    width
}

/// Baseline-relative vertical extent used to centre text in a row.
pub fn ascent(size: f32, weight: Weight) -> f32 {
    let fonts = Fonts::get();
    let scaled = fonts.font(weight).as_scaled(PxScale::from(size));
    scaled.ascent()
}

fn build_path(
    text: &str,
    size: f32,
    weight: Weight,
    x: f32,
    baseline: f32,
    align: Align,
) -> Option<tiny_skia::Path> {
    let fonts = Fonts::get();
    let font = fonts.font(weight);
    let scaled = font.as_scaled(PxScale::from(size));
    let width = measure(text, size, weight);
    let mut pen_x = match align {
        Align::Left => x,
        Align::Center => x - width * 0.5,
        Align::Right => x - width,
    };
    let mut builder = PathBuilder::new();
    let mut prev = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            pen_x += scaled.kern(p, id);
        }
        let advance = scaled.h_advance(id);
        if let Some(outline) = font.outline(id) {
            // Outline coordinates are font units (y-up); map them to pixels.
            let sf = size / font.units_per_em().unwrap_or(1000.0);
            let tx = |x: f32| pen_x + x * sf;
            let ty = |y: f32| baseline - y * sf;
            // Curves arrive as a flat list. A new contour begins wherever the next
            // curve does not start on the current point, so chain them instead of
            // starting a fresh subpath per segment (that yields zero-area glyphs).
            let mut current: Option<(f32, f32)> = None;
            let mut open = false;
            for curve in &outline.curves {
                use ab_glyph::OutlineCurve::*;
                let start = match curve {
                    Line(a, _) | Quad(a, _, _) | Cubic(a, _, _, _) => (a.x, a.y),
                };
                let starts_new = match current {
                    None => true,
                    Some(c) => (c.0 - start.0).abs() > 0.01 || (c.1 - start.1).abs() > 0.01,
                };
                if starts_new {
                    if open {
                        builder.close();
                    }
                    builder.move_to(tx(start.0), ty(start.1));
                    open = true;
                }
                match *curve {
                    Line(_, b) => {
                        if (b.x - start.0).abs() < 0.01 && (b.y - start.1).abs() < 0.01 {
                            // Degenerate closing segment emitted by the parser.
                            builder.close();
                            open = false;
                            current = None;
                            continue;
                        }
                        builder.line_to(tx(b.x), ty(b.y));
                        current = Some((b.x, b.y));
                    }
                    Quad(_, b, c) => {
                        builder.quad_to(tx(b.x), ty(b.y), tx(c.x), ty(c.y));
                        current = Some((c.x, c.y));
                    }
                    Cubic(_, b, c, d) => {
                        builder.cubic_to(tx(b.x), ty(b.y), tx(c.x), ty(c.y), tx(d.x), ty(d.y));
                        current = Some((d.x, d.y));
                    }
                }
            }
            if open {
                builder.close();
            }
        }
        pen_x += advance;
        prev = Some(id);
    }
    builder.finish()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    pixmap: &mut Pixmap,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    color: Color,
    weight: Weight,
    align: Align,
) {
    if text.is_empty() || size <= 0.0 {
        return;
    }
    let Some(path) = build_path(text, size, weight, x, baseline, align) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

/// Trims `text` with an ellipsis until it fits `max_width`.
pub fn ellipsize(text: &str, max_width: f32, size: f32, weight: Weight) -> String {
    if measure(text, size, weight) <= max_width {
        return text.to_string();
    }
    let ellipsis = "\u{2026}";
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}{ellipsis}");
        if measure(&candidate, size, weight) > max_width {
            break;
        }
        out.push(ch);
    }
    out.push_str(ellipsis);
    out
}

pub fn draw_ellipsized(
    pixmap: &mut Pixmap,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    max_width: f32,
    color: Color,
    weight: Weight,
    align: Align,
) {
    let fitted = ellipsize(text, max_width, size, weight);
    draw(pixmap, &fitted, x, baseline, size, color, weight, align);
}
