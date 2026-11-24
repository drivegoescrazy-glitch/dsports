//! Launcher state: apps, pages, selection and the persisted preferences.
//! Platform data (installed apps, battery, clock) arrives through `platform`.

use crate::pulse::{PulseHit, PulseState};
use crate::scene::ImgId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    Recent,
    Name,
    Size,
}

/// Which physical panel a game opens on. `Auto` keeps Android's own choice,
/// which is what the launcher did before per-game screens existed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LaunchScreen {
    Auto,
    Top,
    Bottom,
}

impl LaunchScreen {
    pub fn label(self) -> &'static str {
        match self {
            LaunchScreen::Auto => "Auto",
            LaunchScreen::Top => "Top screen",
            LaunchScreen::Bottom => "Bottom screen",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LaunchScreen::Auto => "auto",
            LaunchScreen::Top => "top",
            LaunchScreen::Bottom => "bottom",
        }
    }

    pub fn from_str(text: &str) -> LaunchScreen {
        match text.trim().to_ascii_lowercase().as_str() {
            "top" => LaunchScreen::Top,
            "bottom" => LaunchScreen::Bottom,
            _ => LaunchScreen::Auto,
        }
    }
}

/// The physical display layout reported by Android. `swap` exists because a few
/// dual-screen handhelds expose the bottom panel as the primary display.
#[derive(Clone, Copy, Debug)]
pub struct Displays {
    pub count: usize,
    pub swap: bool,
}

impl Default for Displays {
    fn default() -> Self {
        Displays { count: 1, swap: false }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Page {
    Games,
    Apps,
    Media,
    All,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::Games, Page::Apps, Page::Media, Page::All];

    pub fn title(&self) -> &'static str {
        match self {
            Page::Games => "Games",
            Page::Apps => "Apps",
            Page::Media => "Media",
            Page::All => "All",
        }
    }

    pub fn index(&self) -> usize {
        Page::ALL.iter().position(|p| p == self).unwrap_or(0)
    }
}

#[derive(Clone)]
pub struct AppEntry {
    pub package: String,
    pub label: String,
    pub version: String,
    /// Bytes on disk, 0 when unknown.
    pub size_bytes: u64,
    /// Milliseconds of tracked play time.
    pub play_ms: u64,
    /// Minutes since the epoch of the last launch, 0 when never.
    pub last_played_min: u64,
    /// True for games (category GAME, or a known emulator/front-end).
    pub is_game: bool,
    pub is_media: bool,
    /// Per-game launch display choice.
    pub screen: LaunchScreen,
    /// Custom box art / hero, when the user picked one.
    pub boxart: ImgId,
    pub hero: ImgId,
    /// Live app icon, sampled from PackageManager.
    pub icon: ImgId,
    /// Colour seed used when there is no artwork yet.
    pub seed: u32,
}

impl AppEntry {
    pub fn page(&self) -> Page {
        if self.is_game {
            Page::Games
        } else if self.is_media {
            Page::Media
        } else {
            Page::Apps
        }
    }
}

#[derive(Clone)]
pub struct StatusBar {
    pub clock: String,
    pub battery: u8,
    pub free_bytes: u64,
    pub online: bool,
}

impl Default for StatusBar {
    fn default() -> Self {
        StatusBar {
            clock: "12:17 AM".into(),
            battery: 97,
            free_bytes: 24_300_000_000,
            online: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    Settings,
}

#[derive(Clone)]
pub struct State {
    pub apps: Vec<AppEntry>,
    pub selected: usize,
    pub page: Page,
    pub sort: Sort,
    pub status: StatusBar,
    /// Dual-screen layout: cards on top, PULSE + widgets below.
    pub dual: bool,
    /// Detected physical displays and which one the user calls the top panel.
    pub displays: Displays,
    /// PULSE underclocker state and detected hardware.
    pub pulse: PulseState,
    pub overlay: Overlay,
    /// Page scroll offset in cards (animated toward `scroll_target`).
    pub scroll: f32,
    /// Card position the row is easing toward.
    pub scroll_target: f32,
    /// Scroll interpolation multiplier (1x–8x; default 4x).
    pub scroll_speed: f32,
    pub hit: Vec<Hit>,
    /// In-flight gesture, in design space.
    pub gesture: Option<Gesture>,
    /// Set when any preference (display, speed, launch screens, PULSE) changed.
    pub persist: bool,
    last_tap: Option<(usize, u64)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Gesture {
    pub x0: f32,
    pub y0: f32,
    pub dx: f32,
    pub t0: u64,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    None,
    /// Launch a package; `slot` is -1 for auto, 0 for primary, 1 for secondary.
    Launch { package: String, slot: i32 },
    /// Send a PULSE profile to the PServer backend.
    PulseApply(String),
    PickImage(i32),
    HomeSettings,
    CheckUpdate,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub rect: crate::scene::Rect,
    pub action: Action,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Launch(usize),
    Select(usize),
    NextPage,
    PrevPage,
    ToggleDual,
    OpenSettings,
    CloseOverlay,
    PickBoxart(usize),
    PickHero(usize),
    CycleSort,
    None,
}

impl State {
    pub fn new(apps: Vec<AppEntry>) -> Self {
        State {
            apps,
            selected: 0,
            page: Page::Games,
            sort: Sort::Recent,
            status: StatusBar::default(),
            dual: false,
            displays: Displays::default(),
            pulse: PulseState::new(),
            overlay: Overlay::None,
            scroll: 0.0,
            scroll_target: 0.0,
            scroll_speed: 4.0,
            hit: Vec::new(),
            gesture: None,
            persist: false,
            last_tap: None,
        }
    }

    /// Swaps in the app list once discovery finishes on its worker thread.
    pub fn replace_apps(&mut self, apps: Vec<AppEntry>) {
        self.apps = apps;
        if self.apps.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.apps.len() {
            self.selected = self.apps.len() - 1;
        }
        self.hit.clear();
        // Keep the row on whatever card stayed selected after the swap.
        let pos = self
            .visible()
            .iter()
            .position(|i| *i == self.selected)
            .unwrap_or(0) as f32;
        self.scroll = pos;
        self.scroll_target = pos;
    }

    /// Switches page, wrapping around, and focuses its first card.
    pub fn page_shift(&mut self, delta: i32) {
        let pages = Page::ALL;
        let n = pages.len() as i32;
        let next = (self.page.index() as i32 + delta).rem_euclid(n);
        self.page = pages[next as usize].clone();
        if let Some(first) = self.visible().first().copied() {
            self.selected = first;
        }
        self.scroll = 0.0;
        self.scroll_target = 0.0;
    }

    pub fn press(&mut self, x: f32, y: f32) {
        self.gesture = Some(Gesture {
            x0: x,
            y0: y,
            dx: 0.0,
            t0: 0,
        });
    }

    pub fn touch_start(&mut self, x: f32, y: f32, _t: i64) {
        self.press(x, y);
        if self.pulse.visible() {
            self.pulse.press(x, y);
        }
    }

    pub fn touch_move(&mut self, x: f32, _y: f32, _t: i64) {
        if let Some(g) = &mut self.gesture {
            g.dx = x - g.x0;
        }
        if self.pulse.drag.is_some() {
            self.pulse.drag_to(x);
        }
    }

    /// Resolves the gesture into an action and updates the selection.
    pub fn touch_end(&mut self, x: f32, y: f32, cancel: bool, _down: i64, _up: i64) -> Outcome {
        let gesture = self.gesture.take();
        if cancel {
            self.pulse.drag = None;
            return Outcome::None;
        }
        // Slider drags always finish, even if the finger ended outside the
        // sheet: otherwise a fast pull could leave the new cap unapplied.
        if self.pulse.drag.is_some() {
            if let Some(json) = self.pulse.release() {
                self.persist = true;
                return Outcome::PulseApply(json);
            }
            return Outcome::None;
        }
        // The full PULSE panel owns the whole screen while it is open.
        if self.pulse.open {
            let game = self.selected_app().map(|a| (a.label.clone(), a.package.clone()));
            let hit = self.pulse.hit_full(
                x,
                y,
                game.as_ref().map(|(label, pkg)| (label.as_str(), pkg.as_str())),
            );
            return self.pulse_outcome(hit);
        }
        // In dual-screen mode the compact PULSE panel replaces the widgets in
        // the bottom screen area; the cards above keep working normally.
        if self.dual && self.pulse.bottom && y >= 520.0 {
            let game = self.selected_app().map(|a| (a.label.clone(), a.package.clone()));
            let hit = self.pulse.hit_bottom(
                x,
                y,
                game.as_ref().map(|(label, pkg)| (label.as_str(), pkg.as_str())),
            );
            return self.pulse_outcome(hit);
        }
        if self.overlay == Overlay::Settings {
            return self.settings_hit(x, y);
        }
        // Horizontal swipe pages through the row.
        if let Some(g) = gesture {
            if g.dx.abs() > 48.0 {
                self.step(if g.dx < 0.0 { 1 } else { -1 });
                return Outcome::None;
            }
        }
        // Play pill, inside the CardsNX panel.
        if inside(x, y, 316.0, 950.0, 300.0, 76.0) {
            if let Some(outcome) = self.launch_outcome() {
                return outcome;
            }
        }
        // Power button: opens the full PULSE panel, or flips the compact one
        // into the dual-screen bottom area.
        if (x - crate::theme::m::BAR_POWER_X).powi(2)
            + (y - crate::theme::m::TOP_MARGIN).powi(2)
            < (crate::theme::m::BTN_D * 0.5).powi(2)
        {
            if self.dual {
                self.pulse.bottom = !self.pulse.bottom;
            } else {
                self.pulse.open = true;
                self.overlay = Overlay::None;
            }
            self.persist = true;
            return Outcome::None;
        }
        // Settings gear (kGearX 1146 on the 1280 layout).
        if (x - crate::theme::m::BAR_GEAR_X).powi(2) + (y - crate::theme::m::TOP_MARGIN).powi(2)
            < crate::theme::m::BTN_D.powi(2)
        {
            self.overlay = Overlay::Settings;
            return Outcome::None;
        }
        // Status pill: cycle pages.
        if inside(
            x,
            y,
            crate::theme::m::BAR_PILL_LEFT,
            crate::theme::m::TOP_MARGIN - crate::theme::m::BTN_D * 0.5,
            crate::theme::m::BAR_PILL_RIGHT - crate::theme::m::BAR_PILL_LEFT,
            crate::theme::m::BTN_D,
        ) {
            self.page_shift(1);
            return Outcome::None;
        }

        // Cards: pick the nearest card centre, tap again to launch.
        let (top, h) = if self.dual { (148.0, 372.0) } else { (crate::theme::m::CARD_ROW_TOP, crate::theme::m::CARD_H) };
        if y >= top - 20.0 && y <= top + h + 40.0 {
            let w = h * crate::theme::m::CARD_RATIO;
            let pitch = w + crate::theme::m::CARD_GAP;
            let visible = self.visible();
            if visible.is_empty() {
                return Outcome::None;
            }
            let pos = visible.iter().position(|i| *i == self.selected).unwrap_or(0) as f32;
            let delta = ((x - 960.0) / pitch).round();
            let target = (pos + delta).clamp(0.0, (visible.len() - 1) as f32) as usize;
            let index = visible[target];
            if index == self.selected {
                if let Some((last, _)) = self.last_tap {
                    if last == index {
                        if let Some(outcome) = self.launch_outcome() {
                            return outcome;
                        }
                    }
                }
                self.last_tap = Some((index, 0));
            } else {
                self.selected = index;
                // Keep the current offset and animate toward the tapped game,
                // so the focus border crossfades just like keyboard navigation.
                self.scroll_target = target as f32;
                self.last_tap = None;
            }
        }
        Outcome::None
    }

    fn settings_hit(&mut self, x: f32, y: f32) -> Outcome {
        // Per-game launch screen.
        let screen = if inside(x, y, 356.0, 304.0, 164.0, 64.0) {
            Some(LaunchScreen::Auto)
        } else if inside(x, y, 532.0, 304.0, 168.0, 64.0) {
            Some(LaunchScreen::Top)
        } else if inside(x, y, 712.0, 304.0, 188.0, 64.0) {
            Some(LaunchScreen::Bottom)
        } else {
            None
        };
        if let Some(screen) = screen {
            if let Some(app) = self.apps.get_mut(self.selected) {
                app.screen = screen;
                self.persist = true;
            }
            return Outcome::None;
        }
        // Which physical panel "top" means on this device.
        if inside(x, y, 356.0, 388.0, 584.0, 56.0) {
            self.displays.swap = !self.displays.swap;
            self.persist = true;
            return Outcome::None;
        }

        // Horizontal speed slider, snapped to half steps for precise control.
        if inside(x, y, 1000.0, 286.0, 520.0, 60.0) {
            let ratio = ((x - 1020.0) / 440.0).clamp(0.0, 1.0);
            let next_speed = ((1.0 + ratio * 7.0) * 2.0).round() / 2.0;
            if (next_speed - self.scroll_speed).abs() >= 0.01 {
                self.scroll_speed = next_speed.clamp(1.0, 8.0);
                self.persist = true;
            }
            return Outcome::None;
        }
        if inside(x, y, 800.0, 518.0, 140.0, 54.0) {
            self.dual = !self.dual;
            if !self.dual {
                self.pulse.bottom = false;
            }
            self.persist = true;
            return Outcome::None;
        }
        if inside(x, y, 356.0, 724.0, 280.0, 64.0) {
            self.pulse.open = true;
            self.pulse.bottom = false;
            self.overlay = Overlay::None;
            return Outcome::None;
        }
        if inside(x, y, 1020.0, 596.0, 300.0, 64.0) {
            return Outcome::CheckUpdate;
        }
        if inside(x, y, 1020.0, 428.0, 250.0, 64.0) {
            return Outcome::PickImage(0);
        }
        if inside(x, y, 1286.0, 428.0, 250.0, 64.0) {
            return Outcome::PickImage(1);
        }
        if inside(x, y, 356.0, 860.0, 220.0, 76.0) {
            self.overlay = Overlay::None;
        }
        Outcome::None
    }

    fn pulse_outcome(&mut self, hit: PulseHit) -> Outcome {
        match hit {
            PulseHit::None => Outcome::None,
            PulseHit::Changed => {
                self.persist = true;
                Outcome::None
            }
            PulseHit::Apply(json) => {
                self.persist = true;
                Outcome::PulseApply(json)
            }
        }
    }

    /// Maps a game's stored choice onto a display slot (0 primary, 1 secondary).
    pub fn screen_slot(&self, screen: LaunchScreen) -> i32 {
        if screen == LaunchScreen::Auto {
            return -1;
        }
        let top = if self.displays.swap { 1 } else { 0 };
        let bottom = 1 - top;
        let wanted = if screen == LaunchScreen::Top { top } else { bottom };
        if (wanted as usize) < self.displays.count.max(1) {
            wanted as i32
        } else {
            0
        }
    }

    fn launch_outcome(&self) -> Option<Outcome> {
        let app = self.selected_app()?;
        Some(Outcome::Launch {
            package: app.package.clone(),
            slot: self.screen_slot(app.screen),
        })
    }

    /// Indices of the apps on the active page, in display order.
    pub fn visible(&self) -> Vec<usize> {
        // Android apps hardly ever declare android:category="game", so the
        // Games page is the whole shelf (as CardsNX's HOME is) rather than a
        // filter that comes back empty. Apps/Media stay narrower views.
        let mut idx: Vec<usize> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, a)| match self.page {
                Page::Games | Page::All => true,
                Page::Apps => !a.is_media,
                Page::Media => a.is_media,
            })
            .map(|(i, _)| i)
            .collect();
        match self.sort {
            Sort::Name => idx.sort_by(|a, b| self.apps[*a].label.to_lowercase().cmp(&self.apps[*b].label.to_lowercase())),
            Sort::Size => idx.sort_by(|a, b| self.apps[*b].size_bytes.cmp(&self.apps[*a].size_bytes)),
            Sort::Recent => idx.sort_by(|a, b| self.apps[*b].last_played_min.cmp(&self.apps[*a].last_played_min)),
        }
        idx
    }

    pub fn selected_app(&self) -> Option<&AppEntry> {
        self.apps.get(self.selected)
    }

    pub fn step(&mut self, delta: i32) {
        let visible = self.visible();
        if visible.is_empty() {
            return;
        }
        let pos = visible.iter().position(|i| *i == self.selected).unwrap_or(0) as i32;
        let next = (pos + delta).clamp(0, visible.len() as i32 - 1);
        self.selected = visible[next as usize];
        // The row follows the selection; without this the panel changed game
        // while the cards stayed put.
        self.scroll_target = next as f32;
    }

    /// Eases the row toward its target. Returns true while it is still moving,
    /// so the render thread knows to paint another frame.
    pub fn tick(&mut self, dt_ms: f32) -> bool {
        let delta = self.scroll_target - self.scroll;
        if delta.abs() < 0.002 {
            self.scroll = self.scroll_target;
            return false;
        }
        // Scale the existing frame-rate independent easing by the saved speed.
        let speed = self.scroll_speed.clamp(1.0, 8.0);
        let k = 1.0 - (-(dt_ms.max(0.1) * speed) / 55.0).exp();
        self.scroll += delta * k.clamp(0.0, 1.0);
        true
    }
}

fn inside(x: f32, y: f32, rx: f32, ry: f32, rw: f32, rh: f32) -> bool {
    x >= rx && x <= rx + rw && y >= ry && y <= ry + rh
}

pub fn demo_apps() -> Vec<AppEntry> {
    let names = [
        ("com.platinum.bayonetta2", "Bayonetta 2", 14_900_000_000u64, 0u64, 2_100, true),
        ("com.nintendo.zelda", "The Legend of Zelda", 12_100_000_000, 5_400_000, 1_900, true),
        ("com.horizon.zerodawn", "Horizon Zero Dawn", 38_000_000_000, 12_600_000, 1_400, true),
        ("com.playstation.ghost", "Ghost of Tsushima", 52_400_000_000, 48_000_000, 640, true),
        ("com.square.ff7", "Final Fantasy VII", 9_800_000_000, 240_000, 3_000, true),
        ("com.capcom.dmc", "Devil May Cry 5", 21_300_000_000, 0, 2_600, true),
        ("com.gog.cyberpunk", "Cyberpunk 2077", 61_000_000_000, 96_000_000, 180, true),
        ("com.valve.portal", "Portal 2", 4_200_000_000, 3_600_000, 900, true),
        ("com.moon.hollow", "Hollow Knight", 2_700_000_000, 18_000_000, 420, true),
        ("com.supergiant.hades", "Hades", 6_400_000_000, 7_200_000, 520, true),
        ("com.spotify.music", "Spotify", 380_000_000, 0, 60, false),
        ("com.netflix", "Netflix", 210_000_000, 0, 30, false),
    ];
    names
        .iter()
        .enumerate()
        .map(|(i, (package, label, size, play, last, game))| AppEntry {
            package: (*package).to_string(),
            label: (*label).to_string(),
            version: format!("1.{}", i % 5),
            size_bytes: *size,
            play_ms: *play,
            last_played_min: *last,
            is_game: *game,
            is_media: !*game && (label.contains("Spotify") || label.contains("Netflix")),
            screen: LaunchScreen::Auto,
            boxart: ImgId::None,
            hero: ImgId::None,
            icon: ImgId::None,
            seed: (i as u32 * 47) % 360,
        })
        .collect()
}

#[cfg(test)]
mod scroll_speed_tests {
    use super::{demo_apps, LaunchScreen, Overlay, State};

    #[test]
    fn default_scroll_speed_is_four_times() {
        let state = State::new(demo_apps());
        assert!((state.scroll_speed - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn faster_setting_advances_scroll_faster() {
        let mut normal = State::new(demo_apps());
        let mut fast = State::new(demo_apps());
        normal.scroll_speed = 1.0;
        fast.scroll_speed = 4.0;
        normal.scroll_target = 1.0;
        fast.scroll_target = 1.0;
        assert!(normal.tick(16.0));
        assert!(fast.tick(16.0));
        assert!(fast.scroll > normal.scroll);
    }

    #[test]
    fn launch_screen_maps_to_displays_and_swap() {
        let mut state = State::new(demo_apps());
        state.displays.count = 2;
        assert_eq!(state.screen_slot(LaunchScreen::Auto), -1);
        assert_eq!(state.screen_slot(LaunchScreen::Top), 0);
        assert_eq!(state.screen_slot(LaunchScreen::Bottom), 1);
        state.displays.swap = true;
        assert_eq!(state.screen_slot(LaunchScreen::Top), 1);
        assert_eq!(state.screen_slot(LaunchScreen::Bottom), 0);
        state.displays.count = 1;
        assert_eq!(state.screen_slot(LaunchScreen::Bottom), 0);
    }

    #[test]
    fn settings_chip_sets_a_per_game_screen() {
        let mut state = State::new(demo_apps());
        state.overlay = Overlay::Settings;
        state.selected = 0;
        let outcome = state.touch_end(600.0, 330.0, false, 0, 10);
        assert!(matches!(outcome, super::Outcome::None));
        assert_eq!(state.apps[0].screen, LaunchScreen::Top);
        assert!(state.persist);
        // Another card keeps its own choice.
        assert_eq!(state.apps[1].screen, LaunchScreen::Auto);
    }

    #[test]
    fn power_button_opens_the_bottom_panel_in_dual_mode() {
        let mut state = State::new(demo_apps());
        state.dual = true;
        state.touch_end(crate::theme::m::BAR_POWER_X, crate::theme::m::TOP_MARGIN, false, 0, 10);
        assert!(state.pulse.bottom);
        assert!(!state.pulse.open);
        state.touch_end(crate::theme::m::BAR_POWER_X, crate::theme::m::TOP_MARGIN, false, 0, 10);
        assert!(!state.pulse.bottom);
    }
}
