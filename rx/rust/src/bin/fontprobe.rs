//! Diagnostic: inspect the outline data ab_glyph hands back for one glyph.
use ab_glyph::{Font, FontVec};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "/opt/freestyle/python/lib/python3.12/site-packages/matplotlib/mpl-data/fonts/ttf/DejaVuSans.ttf".into()
    });
    let bytes = std::fs::read(&path).expect("font file");
    let font = FontVec::try_from_vec(bytes).expect("parse");
    println!("units_per_em = {:?}", font.units_per_em());
    let id = font.glyph_id('A');
    println!("glyph_id = {:?}", id);
    if let Some(outline) = font.outline(id) {
        println!("bounds = {:?}", outline.bounds);
        println!("curves = {}", outline.curves.len());
        for curve in outline.curves.iter().take(6) {
            println!("  {:?}", curve);
        }
        let first = outline.curves.first();
        if let Some(ab_glyph::OutlineCurve::Line(a, _)) = first {
            println!("first point = ({}, {})", a.x, a.y);
        }
    }
}
