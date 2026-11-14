//! Palette and metrics, ported from the v1.0.1 `Look.kt` and measured off the
//! CardsNX reference frame (1280x720) so the Rust port keeps the same identity.

use tiny_skia::Color;

#[inline]
pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

#[inline]
pub fn white(a: u8) -> Color {
    rgba(255, 255, 255, a)
}

#[inline]
pub fn black(a: u8) -> Color {
    rgba(6, 4, 12, a)
}

pub struct Palette;

impl Palette {
    #[allow(non_snake_case)]
    pub fn TEXT() -> Color {
        rgba(0xF7, 0xF3, 0xFF, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn TEXT_DIM() -> Color {
        rgba(0xC3, 0xB8, 0xD8, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn TEXT_FAINT() -> Color {
        rgba(0x8E, 0x83, 0xA6, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn ACCENT() -> Color {
        rgba(0xB4, 0x5C, 0xFF, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn AMBER() -> Color {
        rgba(0xE9, 0xB5, 0x6B, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn DANGER() -> Color {
        rgba(0xFF, 0x6B, 0x6B, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn WARN() -> Color {
        rgba(0xFF, 0xC4, 0x6B, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn GOOD() -> Color {
        rgba(0xEF, 0xFA, 0xF2, 0xFF)
    }
    #[allow(non_snake_case)]
    pub fn PANEL() -> Color {
        rgba(0x14, 0x10, 0x20, 0xFF)
    }
}

/// Everything is laid out in a 1920x1080 design space and scaled to the surface,
/// so the same numbers hold on a phone, a tablet and a 16:9 handheld.
pub const DESIGN_W: f32 = 1920.0;
pub const DESIGN_H: f32 = 1080.0;

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Design-space to pixel scale.
    pub s: f32,
    /// Letterbox offset for aspect ratios that are not 16:9.
    pub ox: f32,
    pub oy: f32,
    pub w: f32,
    pub h: f32,
}

impl Metrics {
    pub fn new(width: f32, height: f32) -> Self {
        // Cover the surface: fill the whole window, crop the design box.
        let sx = width / DESIGN_W;
        let sy = height / DESIGN_H;
        let s = sx.max(sy);
        Metrics {
            s,
            ox: (width - DESIGN_W * s) * 0.5,
            oy: (height - DESIGN_H * s) * 0.5,
            w: width,
            h: height,
        }
    }

    #[inline]
    pub fn px(&self, v: f32) -> f32 {
        v * self.s
    }
}

// The CardsNX source (projects/cards/src/CardsActivity.cpp) lays out on
// 1280x720. This port works in 1920x1080, so every number below is the source
// constant multiplied by 1.5 and named the same way.
pub mod m {
    // Top bar (kTopY 47, kCircle 77) ------------------------------------------
    pub const BTN_D: f32 = 115.5;
    pub const TOP_MARGIN: f32 = 70.5;
    pub const BTN_GAP: f32 = 12.0;
    /// kMenuX / kDiscordX / kSortX.
    pub const BAR_MENU_X: f32 = 201.0;
    pub const BAR_DISCORD_X: f32 = 328.5;
    pub const BAR_SORT_X: f32 = 456.0;
    /// kPillRight: the title pill ends here, everything right of it is a circle.
    pub const BAR_PILL_LEFT: f32 = 558.0;
    pub const BAR_PILL_RIGHT: f32 = 1308.0;
    /// kBellX / kMusicX / kPowerX / kGearX.
    pub const BAR_BELL_X: f32 = 1362.0;
    pub const BAR_MUSIC_X: f32 = 1464.0;
    pub const BAR_POWER_X: f32 = 1591.5;
    pub const BAR_GEAR_X: f32 = 1719.0;

    // Cards (kCardW 240, kCardH 340, grow 10x14, mid 291, pitch 250) ----------
    /// Focused card height; neighbours are kCardH tall.
    pub const CARD_H: f32 = 531.0;
    pub const CARD_RATIO: f32 = 240.0 / 354.0;
    pub const CARD_BACK_SCALE: f32 = 0.80;
    pub const CARD_BACK_DROP: f32 = 10.5;
    pub const CARD_GAP: f32 = 15.0;
    pub const CARD_RADIUS: f32 = 39.0;
    /// Mid line 291 puts the focused card's top at 114 in source space.
    pub const CARD_ROW_TOP: f32 = 171.0;

    // Panel (kPanel 186,503 907x207, radius 35) -------------------------------
    pub const PANEL_W: f32 = 1360.5;
    pub const PANEL_H: f32 = 310.5;
    pub const PANEL_RADIUS: f32 = 52.5;
    pub const PANEL_TOP: f32 = 754.5;

    // Dual screen widgets -----------------------------------------------------
    pub const WIDGET_GAP: f32 = 20.0;
    pub const WIDGET_RADIUS: f32 = 34.0;
}
