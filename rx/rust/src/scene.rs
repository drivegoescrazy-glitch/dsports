//! The scene: a flat list of draw commands produced by layout code and painted
//! by tiny-skia. The Android build uploads the resulting pixmap as one texture;
//! the preview binary writes the same pixmap to a PNG, so both are identical.

use crate::text::{self, Align, Weight};
use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap,
    PixmapPaint, Point, Rect as SkRect, SpreadMode, Stroke, Transform,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

pub struct Images {
    pub items: Vec<Pixmap>,
}

impl Default for Images {
    fn default() -> Self {
        Self::new()
    }
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
                    rect,
                    radius,
                    fill,
                    stroke,
                } => {
                    if let Some(path) = rrect_path(*rect, *radius) {
                        if let Some(fill) = fill {
                            let paint = paint_for(*fill, *rect);
                            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                        }
                        if let Some((w, color)) = stroke {
                            let mut paint = Paint::default();
                            paint.set_color(*color);
                            paint.anti_alias = true;
                            let style = Stroke { width: *w, ..Default::default() };
                            pixmap.stroke_path(&path, &paint, &style, Transform::identity(), None);
                        }
                    }
                }
                Cmd::Circle { cx, cy, radius, fill, stroke } => {
                    let mut pb = PathBuilder::new();
                    pb.push_circle(*cx, *cy, *radius);
                    if let Some(path) = pb.finish() {
                        if let Some(fill) = fill {
                            let bounds = rect(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
                            let paint = paint_for(*fill, bounds);
                            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                        }
                        if let Some((w, color)) = stroke {
                            let mut paint = Paint::default();
                            paint.set_color(*color);
                            paint.anti_alias = true;
                            let style = Stroke { width: *w, ..Default::default() };
                            pixmap.stroke_path(&path, &paint, &style, Transform::identity(), None);
                        }
                    }
                }
                Cmd::Image { rect: dst, radius, img, uv, alpha, tint } => {
                    let Some(source) = self.images.get(*img) else { continue };
                    let Some(path) = rrect_path(*dst, *radius) else { continue };
                    let sw = source.width() as f32;
                    let sh = source.height() as f32;
                    let src = rect(uv.left() * sw, uv.top() * sh, uv.width() * sw, uv.height() * sh);
                    let sx = dst.width() / src.width().max(1.0);
                    let sy = dst.height() / src.height().max(1.0);
                    let transform = Transform::from_translate(
                        dst.left() - src.left() * sx,
                        dst.top() - src.top() * sy,
                    )
                    .pre_scale(sx, sy);
                    if let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) {
                        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
                        let paint = PixmapPaint {
                            opacity: alpha.clamp(0.0, 1.0),
                            quality: FilterQuality::Bilinear,
                            ..Default::default()
                        };
                        pixmap.draw_pixmap(0, 0, source.as_ref(), &paint, transform, Some(&mask));
                    }
                    if let Some(tint) = tint {
                        let mut paint = Paint::default();
                        paint.set_color(*tint);
                        paint.anti_alias = true;
                        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                    }
                }
                Cmd::Text { text, x, baseline, size, color, weight, align } => {
                    text::draw(pixmap, text, *x, *baseline, *size, *color, *weight, *align);
                }
                Cmd::Line { x0, y0, x1, y1, width, color } => {
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
                            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                        }
                    }
                }
                Cmd::Arc { cx, cy, radius, start_deg, sweep_deg, width, color } => {
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

fn capsule(pixmap: &mut Pixmap, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
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
