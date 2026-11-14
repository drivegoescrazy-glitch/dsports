//! The scene: a flat list of draw commands produced by layout code and painted
//! by tiny-skia. The Android build uploads the resulting pixmap as one texture;
//! the preview binary writes the same pixmap to a PNG, so both are identical.
//!
//! Two primitives deliberately avoid tiny-skia's own paths: `draw_pixmap` for
//! artwork (111 ms for a 720p blit) and full-screen `Mask`s (60 ms each). Both
//! are replaced by direct, premultiplied pixel loops over the destination area.

use crate::text::{self, Align, Weight};
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Rect as SkRect,
    SpreadMode, Stroke, Transform,
};

pub type Rect = SkRect;

#[inline]
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    SkRect::from_xywh(x, y, w.max(0.0), h.max(0.0))
        .or_else(|| SkRect::from_xywh(0.0, 0.0, 0.0, 0.0))
        .unwrap()
}

#[derive(Clone, Copy, Debug)]
pub struct Gradient {
    pub top: Color,
    pub bottom: Color,
}

#[derive(Clone, Copy, Debug)]
pub enum Fill {
    Solid(Color),
    Vertical(Gradient),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImgId {
    None,
    Index(usize),
}

#[derive(Clone, Debug)]
pub enum Cmd {
    RoundRect {
        rect: Rect,
        radius: f32,
        fill: Option<Fill>,
        stroke: Option<(f32, Color)>,
    },
    Circle {
        cx: f32,
        cy: f32,
        radius: f32,
        fill: Option<Fill>,
        stroke: Option<(f32, Color)>,
    },
    Image {
        rect: Rect,
        radius: f32,
        img: ImgId,
        /// Crop inside the source, in 0..1 source space.
        uv: Rect,
        alpha: f32,
        /// Translucent wash painted over the image (cool/warm tint).
        tint: Option<Color>,
    },
    Text {
        text: String,
        x: f32,
        baseline: f32,
        size: f32,
        color: Color,
        weight: Weight,
        align: Align,
    },
    Line {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        width: f32,
        color: Color,
    },
    /// Filled polygon; used for the play triangle and chevrons.
    Poly {
        points: Vec<(f32, f32)>,
        color: Color,
    },
    /// Arc segment drawn as a stroked polyline.
    Arc {
        cx: f32,
        cy: f32,
        radius: f32,
        start_deg: f32,
        sweep_deg: f32,
        width: f32,
        color: Color,
    },
}

#[derive(Clone, Default)]
pub struct Images {
    pub items: Vec<Pixmap>,
}

impl Images {
    pub fn new() -> Self {
        Images { items: Vec::new() }
    }
    pub fn push(&mut self, pixmap: Pixmap) -> ImgId {
        self.items.push(pixmap);
        ImgId::Index(self.items.len() - 1)
    }
    pub fn get(&self, id: ImgId) -> Option<&Pixmap> {
        match id {
            ImgId::Index(i) => self.items.get(i),
            ImgId::None => None,
        }
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

pub fn rrect_path(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let rad = radius.min(r.width().min(r.height()) * 0.5).max(0.0);
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    if rad <= 0.01 {
        pb.push_rect(r);
        return pb.finish();
    }
    pb.move_to(l + rad, t);
    pb.line_to(rt - rad, t);
    pb.quad_to(rt, t, rt, t + rad);
    pb.line_to(rt, b - rad);
    pb.quad_to(rt, b, rt - rad, b);
    pb.line_to(l + rad, b);
    pb.quad_to(l, b, l, b - rad);
    pb.line_to(l, t + rad);
    pb.quad_to(l, t, l + rad, t);
    pb.close();
    pb.finish()
}

fn paint_for(fill: Fill, r: Rect) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.anti_alias = true;
    match fill {
        Fill::Solid(color) => paint.set_color(color),
        Fill::Vertical(g) => {
            if let Some(shader) = LinearGradient::new(
                Point::from_xy(r.left(), r.top()),
                Point::from_xy(r.left(), r.bottom().max(r.top() + 0.01)),
                vec![GradientStop::new(0.0, g.top), GradientStop::new(1.0, g.bottom)],
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                paint.shader = shader;
            }
        }
    }
    paint
}

/// Coverage of a rounded rect corner at pixel (x, y), 0..1. Outside the corner
/// squares it is exactly 1, so the common case costs two comparisons.
#[inline]
fn corner_cover(x: i32, y: i32, w: i32, h: i32, rad: f32) -> f32 {
    if rad <= 0.5 {
        return 1.0;
    }
    let r = rad.min(w.min(h) as f32 * 0.5);
    let fx = x as f32 + 0.5;
    let fy = y as f32 + 0.5;
    let cx = if fx < r {
        r
    } else if fx > w as f32 - r {
        w as f32 - r
    } else {
        return 1.0;
    };
    let cy = if fy < r {
        r
    } else if fy > h as f32 - r {
        h as f32 - r
    } else {
        return 1.0;
    };
    let dx = fx - cx;
    let dy = fy - cy;
    let dist = (dx * dx + dy * dy).sqrt();
    (r - dist + 0.5).clamp(0.0, 1.0)
}

/// `src` over `dst`, both premultiplied. `cover` scales the source's own
/// coverage (rounded corner, image opacity); the source alpha does the rest, so
/// translucent surfaces stay translucent.
#[inline]
fn blend(dst: &mut [u8], src: [f32; 4], cover: f32) {
    if cover <= 0.0 {
        return;
    }
    // Effective alpha of the source after coverage: src.a is 0..255.
    let a = (src[3] / 255.0) * cover;
    if a <= 0.0 {
        return;
    }
    if a >= 0.999 && cover >= 0.999 {
        for c in 0..4 {
            dst[c] = src[c].clamp(0.0, 255.0) as u8;
        }
        return;
    }
    let inv = 1.0 - a;
    for c in 0..4 {
        dst[c] = (src[c] * cover + dst[c] as f32 * inv).clamp(0.0, 255.0) as u8;
    }
}

fn premultiplied(c: Color) -> [f32; 4] {
    let a = c.alpha();
    [c.red() * 255.0 * a, c.green() * 255.0 * a, c.blue() * 255.0 * a, a * 255.0]
}

/// Vertical gradient straight into the target: one colour per row, blended
/// across that row, with the rounded corners applied per pixel.
fn paint_gradient(pixmap: &mut Pixmap, dst: Rect, radius: f32, g: Gradient) {
    let x0 = dst.left().round() as i32;
    let y0 = dst.top().round() as i32;
    let w = dst.width().round() as i32;
    let h = dst.height().round() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let top = premultiplied(g.top);
    let bottom = premultiplied(g.bottom);
    let data = pixmap.data_mut();
    for j in 0..h {
        let py = y0 + j;
        if py < 0 || py >= ph {
            continue;
        }
        let t = (j as f32 + 0.5) / h as f32;
        let src = [
            top[0] + (bottom[0] - top[0]) * t,
            top[1] + (bottom[1] - top[1]) * t,
            top[2] + (bottom[2] - top[2]) * t,
            top[3] + (bottom[3] - top[3]) * t,
        ];
        let row = (py * pw * 4) as usize;
        for i in 0..w {
            let px = x0 + i;
            if px < 0 || px >= pw {
                continue;
            }
            let cover = corner_cover(i, j, w, h, radius);
            let idx = row + (px * 4) as usize;
            blend(&mut data[idx..idx + 4], src, cover);
        }
    }
}

/// Bilinear artwork blit into the target, with rounded corners and a tint.
fn paint_image(
    pixmap: &mut Pixmap,
    dst: Rect,
    radius: f32,
    source: &Pixmap,
    src: Rect,
    alpha: f32,
    tint: Option<Color>,
) {
    let x0 = dst.left().round() as i32;
    let y0 = dst.top().round() as i32;
    let w = dst.width().round() as i32;
    let h = dst.height().round() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let sw = source.width() as f32;
    let sh = source.height() as f32;
    let src_w = source.width() as usize;
    let src_data = source.data();
    let tint_px = tint.map(premultiplied);
    let tint_a = tint.map(|t| t.alpha()).unwrap_or(0.0);
    let data = pixmap.data_mut();
    let step_x = src.width() / w as f32;
    let step_y = src.height() / h as f32;
    let max_x = (sw - 1.0).max(0.0);
    let max_y = (sh - 1.0).max(0.0);
    let clamp_x = x0 < 0 || x0 + w > pw;
    let rows = y0 >= 0 && y0 + h <= ph;
    let fx_pre: Vec<(f32, usize, usize)> = (0..w)
        .map(|i| {
            let u = (src.left() + (i as f32 + 0.5) * step_x - 0.5).clamp(0.0, max_x);
            let x1 = u.floor();
            let x2 = (x1 + 1.0).min(max_x);
            (u - x1, x1 as usize * 4, x2 as usize * 4)
        })
        .collect();

    for j in 0..h {
        let py = y0 + j;
        if py < 0 || py >= ph {
            continue;
        }
        let v = (src.top() + (j as f32 + 0.5) * step_y - 0.5).clamp(0.0, max_y);
        let y1 = v.floor();
        let fy = v - y1;
        let y2 = (y1 + 1.0).min(max_y);
        let row1 = y1 as usize * src_w * 4;
        let row2 = y2 as usize * src_w * 4;
        let row = (py * pw * 4) as usize;
        for i in 0..w {
            let px = x0 + i;
            if clamp_x && (px < 0 || px >= pw) {
                continue;
            }
            let (fx, o1, o2) = fx_pre[i as usize];
            let mut c = [0.0f32; 4];
            for k in 0..4 {
                let a0 = src_data[row1 + o1 + k] as f32;
                let a1 = src_data[row1 + o2 + k] as f32;
                let b0 = src_data[row2 + o1 + k] as f32;
                let b1 = src_data[row2 + o2 + k] as f32;
                let top = a0 + (a1 - a0) * fx;
                let bot = b0 + (b1 - b0) * fx;
                c[k] = top + (bot - top) * fy;
            }
            if let (Some(t), a) = (tint_px, tint_a) {
                for k in 0..4 {
                    c[k] = c[k] * (1.0 - a) + t[k];
                }
            }
            let cover = if radius > 0.5 {
                corner_cover(i, j, w, h, radius)
            } else {
                1.0
            } * alpha;
            let idx = row + (px * 4) as usize;
            blend(&mut data[idx..idx + 4], c, cover);
        }
    }
    let _ = rows;
}

pub struct Painter<'a> {
    pub images: &'a Images,
}

impl<'a> Painter<'a> {
    pub fn new(images: &'a Images) -> Self {
        Painter { images }
    }

    pub fn paint(&self, pixmap: &mut Pixmap, cmds: &[Cmd]) {
        for cmd in cmds {
            match cmd {
                Cmd::RoundRect {
                    rect: r,
                    radius,
                    fill,
                    stroke,
                } => {
                    match fill {
                        Some(Fill::Vertical(g)) => {
                            paint_gradient(pixmap, *r, *radius, *g);
                        }
                        Some(Fill::Solid(color)) => {
                            if let Some(path) = rrect_path(*r, *radius) {
                                let mut paint = Paint::default();
                                paint.set_color(*color);
                                paint.anti_alias = true;
                                pixmap.fill_path(
                                    &path,
                                    &paint,
                                    FillRule::Winding,
                                    Transform::identity(),
                                    None,
                                );
                            }
                        }
                        None => {}
                    }
                    if let Some((w, color)) = stroke {
                        if let Some(path) = rrect_path(*r, *radius) {
                            let mut paint = Paint::default();
                            paint.set_color(*color);
                            paint.anti_alias = true;
                            let style = Stroke {
                                width: *w,
                                ..Default::default()
                            };
                            pixmap
                                .stroke_path(&path, &paint, &style, Transform::identity(), None);
                        }
                    }
                }
                Cmd::Circle {
                    cx,
                    cy,
                    radius,
                    fill,
                    stroke,
                } => {
                    let mut pb = PathBuilder::new();
                    pb.push_circle(*cx, *cy, *radius);
                    if let Some(path) = pb.finish() {
                        if let Some(fill) = fill {
                            let bounds = rect(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
                            let paint = paint_for(*fill, bounds);
                            pixmap.fill_path(
                                &path,
                                &paint,
                                FillRule::Winding,
                                Transform::identity(),
                                None,
                            );
                        }
                        if let Some((w, color)) = stroke {
                            let mut paint = Paint::default();
                            paint.set_color(*color);
                            paint.anti_alias = true;
                            let style = Stroke {
                                width: *w,
                                ..Default::default()
                            };
                            pixmap
                                .stroke_path(&path, &paint, &style, Transform::identity(), None);
                        }
                    }
                }
                Cmd::Image {
                    rect: dst,
                    radius,
                    img,
                    uv,
                    alpha,
                    tint,
                } => {
                    let Some(source) = self.images.get(*img) else {
                        continue;
                    };
                    let sw = source.width() as f32;
                    let sh = source.height() as f32;
                    let src = rect(
                        uv.left() * sw,
                        uv.top() * sh,
                        uv.width() * sw,
                        uv.height() * sh,
                    );
                    paint_image(pixmap, *dst, *radius, source, src, *alpha, *tint);
                }
                Cmd::Text {
                    text,
                    x,
                    baseline,
                    size,
                    color,
                    weight,
                    align,
                } => {
                    text::draw(pixmap, text, *x, *baseline, *size, *color, *weight, *align);
                }
                Cmd::Line {
                    x0,
                    y0,
                    x1,
                    y1,
                    width,
                    color,
                } => {
                    capsule(pixmap, *x0, *y0, *x1, *y1, *width, *color);
                }
                Cmd::Poly { points, color } => {
                    if points.len() >= 3 {
                        let mut pb = PathBuilder::new();
                        pb.move_to(points[0].0, points[0].1);
                        for p in &points[1..] {
                            pb.line_to(p.0, p.1);
                        }
                        pb.close();
                        if let Some(path) = pb.finish() {
                            let mut paint = Paint::default();
                            paint.set_color(*color);
                            paint.anti_alias = true;
                            pixmap.fill_path(
                                &path,
                                &paint,
                                FillRule::Winding,
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                }
                Cmd::Arc {
                    cx,
                    cy,
                    radius,
                    start_deg,
                    sweep_deg,
                    width,
                    color,
                } => {
                    let steps = ((sweep_deg.abs() / 6.0).ceil() as usize).max(3);
                    let mut prev: Option<(f32, f32)> = None;
                    for i in 0..=steps {
                        let t = i as f32 / steps as f32;
                        let angle = (start_deg + sweep_deg * t).to_radians();
                        let p = (cx + radius * angle.cos(), cy + radius * angle.sin());
                        if let Some(q) = prev {
                            capsule(pixmap, q.0, q.1, p.0, p.1, *width, *color);
                        }
                        prev = Some(p);
                    }
                }
            }
        }
    }
}

fn capsule(
    pixmap: &mut Pixmap,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    width: f32,
    color: Color,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len >= 0.01 {
        let nx = -dy / len * width * 0.5;
        let ny = dx / len * width * 0.5;
        let mut pb = PathBuilder::new();
        pb.move_to(x0 + nx, y0 + ny);
        pb.line_to(x1 + nx, y1 + ny);
        pb.line_to(x1 - nx, y1 - ny);
        pb.line_to(x0 - nx, y0 - ny);
        pb.close();
        if let Some(path) = pb.finish() {
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
    for (cx, cy) in [(x0, y0), (x1, y1)] {
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, width * 0.5);
        if let Some(path) = pb.finish() {
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
}
