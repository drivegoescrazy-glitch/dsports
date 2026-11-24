//! PULSE — the built-in performance tuner.
//!
//! The tuner follows the no-root PServer approach pioneered by ClusterTune and
//! used by keiretrogaming/pulse: cap per-cluster CPU frequencies by writing
//! `scaling_max_freq`, and cap Adreno by power-level index in `max_pwrlevel`.
//! The launcher owns the profile/UI state; Kotlin (`Pulse.kt`) only discovers
//! sysfs nodes and executes the shell script through PServer.
//!
//! Profiles can be bound to a package and applied automatically when that game
//! launches, and every control is available both in the full panel and in the
//! compact panel that occupies the bottom screen area of the dual-screen home.

use crate::scene::{rect, Cmd, Fill, Gradient, Rect};
use crate::text::{self, Align, Weight};
use crate::theme::{white, Palette};
use crate::ui::{glass, glyph_at, label, Glyph, Ctx};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};
use tiny_skia::Color;

/// What the Kotlin side discovered from sysfs. Field names match the JSON it
/// writes (camelCase), so no manual marshalling is needed.
#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Probe {
    pub available: bool,
    pub error: String,
    pub device: String,
    pub soc_model: String,
    pub clusters: Vec<Cluster>,
    pub gpu: Option<Gpu>,
    pub cpu_temp: String,
    pub battery_temp: String,
    pub battery_capacity: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cluster {
    pub id: u32,
    pub label: String,
    pub min: u32,
    pub max: u32,
    pub current: u32,
    pub freqs: Vec<u32>,
    pub cur_path: String,
    pub max_path: String,
    pub min_path: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Gpu {
    pub min_khz: u32,
    pub max_khz: u32,
    pub current_khz: u32,
    pub levels: Vec<u32>,
    pub cur_path: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Max,
    Balanced,
    Saver,
    Custom,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::Max, Tier::Balanced, Tier::Saver, Tier::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Tier::Max => "Max",
            Tier::Balanced => "Balanced",
            Tier::Saver => "Saver",
            Tier::Custom => "Custom",
        }
    }

    /// Percent of the selectable ceiling a tier asks for.
    pub fn target(self) -> u8 {
        match self {
            Tier::Max => 100,
            Tier::Balanced => 75,
            Tier::Saver => 55,
            Tier::Custom => 0,
        }
    }

    fn code(self) -> u8 {
        match self {
            Tier::Max => 0,
            Tier::Balanced => 1,
            Tier::Saver => 2,
            Tier::Custom => 3,
        }
    }

    fn from_code(code: u8) -> Tier {
        match code {
            0 => Tier::Max,
            1 => Tier::Balanced,
            2 => Tier::Saver,
            _ => Tier::Custom,
        }
    }
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Profile {
    pub tier: u8,
    pub target: u8,
    pub caps: Vec<u32>,
    pub gpu_khz: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drag {
    Target,
    Row(usize),
}

#[derive(Clone, Default, PartialEq)]
pub struct Telemetry {
    /// Live per-cluster clock, kHz.
    pub cpu_khz: Vec<u32>,
    /// Live GPU clock, kHz.
    pub gpu_khz: u32,
    pub soc_temp: Option<f32>,
    pub battery_temp: Option<f32>,
    pub battery: Option<u8>,
    pub read_ok: bool,
}

#[derive(Clone)]
pub struct PulseState {
    pub probe: Probe,
    /// Full-screen panel.
    pub open: bool,
    /// Compact panel in the dual-screen bottom area.
    pub bottom: bool,
    pub tier: Tier,
    pub target: u8,
    pub caps: Vec<u32>,
    pub gpu_khz: u32,
    pub telemetry: Telemetry,
    pub status: String,
    pub status_ok: bool,
    pub auto_game: bool,
    pub profiles: BTreeMap<String, Profile>,
    pub drag: Option<Drag>,
    drag_changed: bool,
    last_poll: Instant,
}

impl Default for PulseState {
    fn default() -> Self {
        Self::new()
    }
}

impl PulseState {
    pub fn new() -> Self {
        PulseState {
            probe: Probe::default(),
            open: false,
            bottom: false,
            tier: Tier::Max,
            target: 100,
            caps: Vec::new(),
            gpu_khz: 0,
            telemetry: Telemetry::default(),
            status: String::new(),
            status_ok: true,
            auto_game: true,
            profiles: BTreeMap::new(),
            drag: None,
            drag_changed: false,
            last_poll: Instant::now(),
        }
    }

    pub fn visible(&self) -> bool {
        self.open || self.bottom
    }

    /// Installs freshly detected hardware and resets the editable caps to the
    /// limits the device currently has. Saved profiles are applied afterwards.
    pub fn apply_probe(&mut self, probe: Probe) {
        self.probe = probe;
        if self.probe.error.is_empty() && !self.probe.available {
            self.probe.error =
                "This device does not expose the PServer service PULSE needs.".into();
        }
        self.caps = self
            .probe
            .clusters
            .iter()
            .map(|c| snap(&c.freqs, c.current.max(c.min)))
            .collect();
        self.gpu_khz = self
            .probe
            .gpu
            .as_ref()
            .map(|g| snap(&g.levels, g.current_khz.max(g.min_khz)))
            .unwrap_or(0);
        self.telemetry = Telemetry {
            cpu_khz: self.probe.clusters.iter().map(|c| c.current).collect(),
            gpu_khz: self.probe.gpu.as_ref().map(|g| g.current_khz).unwrap_or(0),
            ..Telemetry::default()
        };
        let all_max = self
            .probe
            .clusters
            .iter()
            .zip(self.caps.iter())
            .all(|(c, cap)| *cap >= c.max)
            && self
                .probe
                .gpu
                .as_ref()
                .map(|g| self.gpu_khz >= g.max_khz)
                .unwrap_or(true);
        self.tier = if all_max { Tier::Max } else { Tier::Custom };
        self.target = self.target_from_caps().max(1);
        self.status = if self.probe.available {
            format!("Ready · {}", self.device_name())
        } else {
            self.probe.error.clone()
        };
        self.status_ok = self.probe.available;
        self.refresh_telemetry();
    }

    pub fn device_name(&self) -> &str {
        if !self.probe.device.is_empty() {
            &self.probe.device
        } else if !self.probe.soc_model.is_empty() {
            &self.probe.soc_model
        } else {
            "handheld"
        }
    }

    /// Restores the user's saved global profile over the detected hardware.
    pub fn apply_saved(&mut self, saved: Saved) {
        self.auto_game = saved.auto_game;
        self.profiles = saved.profiles;
        if self.probe.clusters.is_empty() {
            return;
        }
        if saved.caps.len() == self.probe.clusters.len() && !saved.caps.is_empty() {
            self.caps = self
                .probe
                .clusters
                .iter()
                .zip(saved.caps.iter())
                .map(|(c, cap)| snap(&c.freqs, (*cap).clamp(c.min, c.max)))
                .collect();
            if let Some(gpu) = &self.probe.gpu {
                self.gpu_khz = snap(&gpu.levels, saved.gpu_khz.clamp(gpu.min_khz, gpu.max_khz));
            }
            self.tier = Tier::from_code(saved.tier);
            self.target = saved.target.clamp(1, 100);
        } else if saved.tier != Tier::Custom.code() {
            self.tier = Tier::from_code(saved.tier);
            self.target = saved.target.max(1);
            self.recompute_caps();
        }
    }

    // ------------------------------------------------------------ the profile

    fn tier_caps(&self) -> Vec<u32> {
        let percent = self.target.clamp(1, 100) as u32;
        self.probe
            .clusters
            .iter()
            .map(|c| snap(&c.freqs, c.max.saturating_mul(percent) / 100))
            .collect()
    }

    fn gpu_for_target(&self) -> u32 {
        match &self.probe.gpu {
            Some(gpu) => snap(&gpu.levels, gpu.max_khz.saturating_mul(self.target.clamp(1, 100) as u32) / 100),
            None => 0,
        }
    }

    fn recompute_caps(&mut self) {
        self.caps = self.tier_caps();
        self.gpu_khz = self.gpu_for_target();
    }

    fn target_from_caps(&self) -> u8 {
        let mut sum = 0u64;
        let mut n = 0u64;
        for (i, cluster) in self.probe.clusters.iter().enumerate() {
            if cluster.max == 0 {
                continue;
            }
            let cap = self.caps.get(i).copied().unwrap_or(cluster.max);
            sum += (cap.min(cluster.max) as u64) * 100 / cluster.max as u64;
            n += 1;
        }
        if let Some(gpu) = &self.probe.gpu {
            if gpu.max_khz > 0 {
                sum += (self.gpu_khz.min(gpu.max_khz) as u64) * 100 / gpu.max_khz as u64;
                n += 1;
            }
        }
        if n == 0 {
            100
        } else {
            (sum / n).clamp(1, 100) as u8
        }
    }

    pub fn set_tier(&mut self, tier: Tier) {
        if tier == Tier::Custom {
            self.tier = Tier::Custom;
            return;
        }
        self.tier = tier;
        self.target = tier.target().max(1);
        self.recompute_caps();
        if self.probe.available {
            self.status = format!("{} tier selected", tier.label());
            self.status_ok = true;
        } else {
            self.status = self.probe.error.clone();
            self.status_ok = false;
        }
    }

    fn drag_target_to(&mut self, ratio: f32) {
        // Keep a 20% floor: below that the device can feel unresponsive, and
        // PULSE's own tiers never go that low.
        let percent = (20.0 + ratio.clamp(0.0, 1.0) * 80.0).round() as u8;
        if percent != self.target {
            self.target = percent.clamp(20, 100);
            self.tier = Tier::Custom;
            self.recompute_caps();
            self.drag_changed = true;
        }
    }

    fn drag_row_to(&mut self, row: usize, ratio: f32) {
        let ratio = ratio.clamp(0.0, 1.0);
        if let Some(cluster) = self.probe.clusters.get(row) {
            let min = cluster.min.min(cluster.max);
            let span = cluster.max.saturating_sub(min);
            let wanted = min + (span as f32 * ratio) as u32;
            let value = snap(&cluster.freqs, wanted);
            if self.caps.get(row).copied() != Some(value) {
                if self.caps.len() != self.probe.clusters.len() {
                    self.caps = self.tier_caps();
                }
                if let Some(slot) = self.caps.get_mut(row) {
                    *slot = value;
                }
                self.tier = Tier::Custom;
                self.target = self.target_from_caps();
                self.drag_changed = true;
            }
            return;
        }
        if let Some(gpu) = &self.probe.gpu {
            let value = snap(
                &gpu.levels,
                gpu.min_khz + ((gpu.max_khz.saturating_sub(gpu.min_khz)) as f32 * ratio) as u32,
            );
            if self.gpu_khz != value {
                self.gpu_khz = value;
                self.tier = Tier::Custom;
                self.target = self.target_from_caps();
                self.drag_changed = true;
            }
        }
    }

    fn reset_values(&mut self) {
        self.caps = self.probe.clusters.iter().map(|c| c.max).collect();
        self.gpu_khz = self.probe.gpu.as_ref().map(|g| g.max_khz).unwrap_or(0);
        self.target = 100;
        self.tier = Tier::Max;
        self.status = "Restoring stock limits…".into();
        self.status_ok = self.probe.available;
    }

    pub fn has_profile(&self, pkg: &str) -> bool {
        self.profiles.contains_key(pkg)
    }

    fn save_profile(&mut self, pkg: &str, label: &str) {
        self.profiles.insert(
            pkg.to_string(),
            Profile {
                tier: self.tier.code(),
                target: self.target,
                caps: self.caps.clone(),
                gpu_khz: self.gpu_khz,
            },
        );
        self.status = format!("Profile saved for {label}");
        self.status_ok = true;
    }

    fn clear_profile(&mut self, pkg: &str, label: &str) {
        self.profiles.remove(pkg);
        self.status = format!("Profile cleared for {label}");
        self.status_ok = true;
    }

    fn adopt(&mut self, profile: &Profile) {
        if profile.caps.len() == self.probe.clusters.len() && !profile.caps.is_empty() {
            self.caps = self
                .probe
                .clusters
                .iter()
                .zip(profile.caps.iter())
                .map(|(c, cap)| snap(&c.freqs, (*cap).clamp(c.min, c.max)))
                .collect();
            if let Some(gpu) = &self.probe.gpu {
                self.gpu_khz = snap(&gpu.levels, profile.gpu_khz.clamp(gpu.min_khz, gpu.max_khz));
            }
            self.tier = Tier::from_code(profile.tier);
            self.target = profile.target.clamp(1, 100);
        } else {
            self.tier = Tier::from_code(profile.tier);
            self.target = profile.target.max(1);
            self.recompute_caps();
        }
    }

    /// Applies this game's saved profile before it launches.
    pub fn game_apply_json(&mut self, pkg: &str) -> Option<String> {
        if !self.auto_game || !self.probe.available {
            return None;
        }
        let profile = self.profiles.get(pkg)?.clone();
        self.adopt(&profile);
        self.status = format!("Applied game profile for {pkg}");
        self.status_ok = true;
        self.plan_json(false)
    }

    // -------------------------------------------------------------- apply path

    pub fn plan_json(&self, restore: bool) -> Option<String> {
        if !self.probe.available {
            return None;
        }
        let mut clusters = serde_json::Map::new();
        for (i, cluster) in self.probe.clusters.iter().enumerate() {
            let cap = self.caps.get(i).copied().unwrap_or(cluster.max).clamp(cluster.min, cluster.max);
            clusters.insert(cluster.id.to_string(), serde_json::json!(cap));
        }
        let mut obj = serde_json::json!({
            "restore": restore,
            "clusters": clusters,
        });
        if let Some(gpu) = &self.probe.gpu {
            obj["gpuKhz"] = serde_json::json!(self.gpu_khz.clamp(gpu.min_khz, gpu.max_khz));
        }
        Some(obj.to_string())
    }

    pub fn note_applied(&mut self, reply: &str) {
        let text = reply.trim();
        if text.is_empty() || text.eq_ignore_ascii_case("ok") || text == "null" {
            self.status = "Applied".into();
            self.status_ok = true;
        } else {
            self.status = text.to_string();
            self.status_ok = false;
        }
    }

    // ------------------------------------------------------------- telemetry

    pub fn refresh_telemetry(&mut self) -> bool {
        if self.probe.clusters.is_empty() && self.probe.gpu.is_none() {
            return false;
        }
        let before = self.telemetry.clone();
        self.telemetry.cpu_khz = self
            .probe
            .clusters
            .iter()
            .enumerate()
            .map(|(i, c)| {
                read_clock(&c.cur_path)
                    .or_else(|| before.cpu_khz.get(i).copied())
                    .unwrap_or(c.current)
            })
            .collect();
        self.telemetry.gpu_khz = self
            .probe
            .gpu
            .as_ref()
            .map(|g| read_clock(&g.cur_path).unwrap_or(before.gpu_khz).max(g.min_khz))
            .unwrap_or(0);
        self.telemetry.soc_temp = read_temp(&self.probe.cpu_temp).or(before.soc_temp);
        self.telemetry.battery_temp = read_temp(&self.probe.battery_temp).or(before.battery_temp);
        self.telemetry.battery =
            read_int(&self.probe.battery_capacity).map(|v| v.clamp(0, 100) as u8).or(before.battery);
        self.telemetry.read_ok = true;
        self.telemetry != before
    }

    /// The one-second poll is driven by the render loop; this keeps the reads
    /// out of the frame path when the panel is closed.
    pub fn want_poll(&self, now: Instant) -> bool {
        self.visible() && now.duration_since(self.last_poll) >= Duration::from_millis(1000)
    }

    pub fn poll_if_due(&mut self, now: Instant) -> bool {
        if !self.want_poll(now) {
            return false;
        }
        self.last_poll = now;
        self.refresh_telemetry();
        true
    }

    // ------------------------------------------------------------------ input

    pub fn press(&mut self, x: f32, y: f32) -> bool {
        let layout = if self.open {
            Layout::full(&self.probe)
        } else if self.bottom {
            Layout::bottom(&self.probe)
        } else {
            return false;
        };
        if inside(layout.target_hit, x, y) {
            self.drag = Some(Drag::Target);
            self.drag_changed = false;
            let ratio = ratio_in(layout.target_track, x);
            self.drag_target_to(ratio);
            return true;
        }
        for (i, row) in layout.rows.iter().enumerate() {
            if inside(row.hit, x, y) {
                self.drag = Some(Drag::Row(i));
                self.drag_changed = false;
                self.drag_row_to(i, ratio_in(row.track, x));
                return true;
            }
        }
        false
    }

    pub fn drag_to(&mut self, x: f32) {
        let layout = if self.open {
            Layout::full(&self.probe)
        } else if self.bottom {
            Layout::bottom(&self.probe)
        } else {
            return;
        };
        match self.drag {
            Some(Drag::Target) => {
                let ratio = ratio_in(layout.target_track, x);
                self.drag_target_to(ratio);
            }
            Some(Drag::Row(i)) => {
                if let Some(row) = layout.rows.get(i) {
                    let ratio = ratio_in(row.track, x);
                    self.drag_row_to(i, ratio);
                }
            }
            None => {}
        }
    }

    pub fn release(&mut self) -> Option<String> {
        let changed = self.drag.take().map(|_| self.drag_changed).unwrap_or(false);
        self.drag_changed = false;
        if !changed {
            return None;
        }
        if !self.probe.available {
            self.status = self.probe.error.clone();
            self.status_ok = false;
            return None;
        }
        self.persist_status();
        self.plan_json(false)
    }

    fn persist_status(&mut self) {
        self.status = "Applying…".into();
        self.status_ok = true;
    }

    pub fn hit_full(&mut self, x: f32, y: f32, game: Option<(&str, &str)>) -> PulseHit {
        let layout = Layout::full(&self.probe);
        if !inside(layout.sheet, x, y) {
            self.open = false;
            return PulseHit::Changed;
        }
        if inside(layout.close, x, y) {
            self.open = false;
            return PulseHit::Changed;
        }
        if let Some(tier) = hit_tier(&layout, x, y) {
            self.set_tier(tier);
            return self.apply_or_changed();
        }
        if inside(layout.apply, x, y) {
            if !self.probe.available {
                self.status = self.probe.error.clone();
                self.status_ok = false;
                return PulseHit::Changed;
            }
            self.persist_status();
            return self
                .plan_json(false)
                .map(PulseHit::Apply)
                .unwrap_or(PulseHit::None);
        }
        if inside(layout.reset, x, y) {
            if !self.probe.available {
                self.status = self.probe.error.clone();
                self.status_ok = false;
                return PulseHit::Changed;
            }
            self.reset_values();
            return self
                .plan_json(true)
                .map(PulseHit::Apply)
                .unwrap_or(PulseHit::None);
        }
        if inside(layout.auto, x, y) {
            self.auto_game = !self.auto_game;
            self.status = if self.auto_game {
                "Per-game profiles apply on launch".into()
            } else {
                "Per-game profiles are not auto-applied".into()
            };
            self.status_ok = true;
            return PulseHit::Changed;
        }
        if let Some((label, pkg)) = game {
            if inside(layout.save, x, y) {
                if self.probe.available {
                    self.save_profile(pkg, label);
                }
                return PulseHit::Changed;
            }
            if inside(layout.clear, x, y) {
                self.clear_profile(pkg, label);
                return PulseHit::Changed;
            }
        }
        PulseHit::None
    }

    pub fn hit_bottom(&mut self, x: f32, y: f32, game: Option<(&str, &str)>) -> PulseHit {
        let layout = Layout::bottom(&self.probe);
        if !inside(layout.sheet, x, y) {
            return PulseHit::None;
        }
        if let Some(tier) = hit_tier(&layout, x, y) {
            self.set_tier(tier);
            return self.apply_or_changed();
        }
        if inside(layout.apply, x, y) {
            if !self.probe.available {
                self.status = self.probe.error.clone();
                self.status_ok = false;
                return PulseHit::Changed;
            }
            self.persist_status();
            return self
                .plan_json(false)
                .map(PulseHit::Apply)
                .unwrap_or(PulseHit::None);
        }
        if inside(layout.reset, x, y) {
            if !self.probe.available {
                self.status = self.probe.error.clone();
                self.status_ok = false;
                return PulseHit::Changed;
            }
            self.reset_values();
            return self
                .plan_json(true)
                .map(PulseHit::Apply)
                .unwrap_or(PulseHit::None);
        }
        if inside(layout.widgets, x, y) {
            self.bottom = false;
            return PulseHit::Changed;
        }
        if inside(layout.full, x, y) {
            self.open = true;
            self.bottom = false;
            return PulseHit::Changed;
        }
        if let Some((label, pkg)) = game {
            if inside(layout.save, x, y) {
                if self.probe.available {
                    self.save_profile(pkg, label);
                }
                return PulseHit::Changed;
            }
        }
        if inside(layout.auto, x, y) {
            self.auto_game = !self.auto_game;
            self.status = if self.auto_game {
                "Per-game profiles apply on launch".into()
            } else {
                "Per-game profiles are not auto-applied".into()
            };
            self.status_ok = true;
            return PulseHit::Changed;
        }
        PulseHit::None
    }

    fn apply_or_changed(&self) -> PulseHit {
        if !self.probe.available {
            return PulseHit::Changed;
        }
        match self.plan_json(false) {
            Some(json) => PulseHit::Apply(json),
            None => PulseHit::Changed,
        }
    }

    // ------------------------------------------------------------ persistence

    pub fn encode(&self) -> String {
        let caps = self
            .caps
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut out = String::new();
        out.push_str(&format!("auto={}\n", u8::from(self.auto_game)));
        out.push_str(&format!("tier={}\n", self.tier.code()));
        out.push_str(&format!("target={}\n", self.target));
        out.push_str(&format!("caps={caps}\n"));
        out.push_str(&format!("gpu={}\n", self.gpu_khz));
        for (pkg, profile) in &self.profiles {
            let caps = profile
                .caps
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&format!(
                "profile.{pkg}={}|{}|{}|{}\n",
                profile.tier, profile.target, caps, profile.gpu_khz
            ));
        }
        out
    }

    pub fn decode(text: &str) -> Saved {
        let mut saved = Saved::default();
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once('=') else {
                continue;
            };
            match key {
                "auto" => saved.auto_game = value.trim() != "0",
                "tier" => saved.tier = value.trim().parse::<u8>().unwrap_or(0).min(3),
                "target" => {
                    saved.target = value.trim().parse::<u8>().unwrap_or(100).clamp(1, 100)
                }
                "caps" => saved.caps = parse_caps(value),
                "gpu" => saved.gpu_khz = value.trim().parse::<u32>().unwrap_or(0),
                other if other.starts_with("profile.") => {
                    let pkg = &other["profile.".len()..];
                    let mut parts = value.split('|');
                    let tier = parts.next().and_then(|v| v.trim().parse().ok()).unwrap_or(3);
                    let target = parts
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(100)
                        .clamp(1, 100);
                    let caps = parts.next().map(parse_caps).unwrap_or_default();
                    let gpu = parts.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    if !pkg.is_empty() {
                        saved.profiles.insert(
                            pkg.to_string(),
                            Profile {
                                tier,
                                target,
                                caps,
                                gpu_khz: gpu,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        saved
    }

    pub fn hash_into<H: Hasher>(&self, hasher: &mut H) {
        self.open.hash(hasher);
        self.bottom.hash(hasher);
        (self.tier.code()).hash(hasher);
        self.target.hash(hasher);
        self.caps.hash(hasher);
        self.gpu_khz.hash(hasher);
        self.status.hash(hasher);
        self.status_ok.hash(hasher);
        self.auto_game.hash(hasher);
        self.probe.available.hash(hasher);
        self.probe.device.hash(hasher);
        self.probe.clusters.len().hash(hasher);
        self.telemetry.cpu_khz.hash(hasher);
        self.telemetry.gpu_khz.hash(hasher);
        self.telemetry
            .soc_temp
            .map(|v| (v * 10.0) as i32)
            .hash(hasher);
        self.telemetry
            .battery_temp
            .map(|v| (v * 10.0) as i32)
            .hash(hasher);
        self.telemetry.battery.hash(hasher);
        // The selected game's profile presence controls the Clear affordance.
        self.profiles.len().hash(hasher);
    }

    // ------------------------------------------------------------- demo data

    pub fn demo() -> Self {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.set_tier(Tier::Balanced);
        pulse.open = true;
        pulse.status = "Ready · AYN Thor".into();
        pulse.telemetry.soc_temp = Some(58.0);
        pulse.telemetry.battery_temp = Some(39.0);
        pulse.telemetry.battery = Some(87);
        pulse.last_poll = Instant::now();
        pulse
    }
}

#[derive(Clone)]
pub struct Saved {
    pub auto_game: bool,
    pub tier: u8,
    pub target: u8,
    pub caps: Vec<u32>,
    pub gpu_khz: u32,
    pub profiles: BTreeMap<String, Profile>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            auto_game: true,
            tier: Tier::Max.code(),
            target: 100,
            caps: Vec::new(),
            gpu_khz: 0,
            profiles: BTreeMap::new(),
        }
    }
}

pub fn save(dir: &str, pulse: &PulseState) {
    if dir.is_empty() {
        return;
    }
    let _ = std::fs::write(format!("{dir}/nxs.pulse"), pulse.encode());
}

pub fn load(dir: &str) -> Saved {
    if dir.is_empty() {
        return Saved::default();
    }
    std::fs::read_to_string(format!("{dir}/nxs.pulse"))
        .map(|text| PulseState::decode(&text))
        .unwrap_or_default()
}

pub enum PulseHit {
    None,
    Changed,
    Apply(String),
}

fn parse_caps(text: &str) -> Vec<u32> {
    text.split(',')
        .filter_map(|v| v.trim().parse::<u32>().ok())
        .collect()
}

/// Snaps to the nearest supported step; ties go to the lower frequency so both
/// the host preview and the device land on the same OPP.
pub fn snap(freqs: &[u32], target: u32) -> u32 {
    freqs
        .iter()
        .copied()
        .min_by_key(|f| ((*f as i64 - target as i64).abs(), *f))
        .unwrap_or(target)
}

fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.left() && x <= r.right() && y >= r.top() && y <= r.bottom()
}

fn ratio_in(track: Rect, x: f32) -> f32 {
    if track.width() <= 0.0 {
        0.0
    } else {
        ((x - track.left()) / track.width()).clamp(0.0, 1.0)
    }
}

fn read_int(path: &str) -> Option<i64> {
    if path.is_empty() {
        return None;
    }
    std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()
}

fn read_clock(path: &str) -> Option<u32> {
    let value = read_int(path)?;
    if value <= 0 {
        return None;
    }
    // cpufreq reports kHz, kgsl reports Hz. The 10 GHz cut keeps both sane.
    let khz = if value >= 10_000_000 { value / 1000 } else { value };
    (khz > 0).then_some(khz as u32)
}

fn read_temp(path: &str) -> Option<f32> {
    let value = read_int(path)?;
    if value <= 0 {
        return None;
    }
    let celsius = if value >= 1000 { value as f32 / 1000.0 } else { value as f32 };
    (celsius > 0.0 && celsius < 150.0).then_some(celsius)
}

fn fmt_khz(khz: u32) -> String {
    if khz == 0 {
        "—".into()
    } else if khz >= 1_000_000 {
        format!("{:.2} GHz", khz as f32 / 1_000_000.0)
    } else {
        format!("{:.0} MHz", khz as f32 / 1000.0)
    }
}

fn temp_text(temp: Option<f32>) -> String {
    temp.map(|v| format!("{v:.0} °C")).unwrap_or_else(|| "—".into())
}

// ------------------------------------------------------------------- layout

struct SliderRow {
    y: f32,
    track: Rect,
    hit: Rect,
}

struct Layout {
    sheet: Rect,
    close: Rect,
    tiers: [Rect; 4],
    target_track: Rect,
    target_hit: Rect,
    rows: Vec<SliderRow>,
    apply: Rect,
    reset: Rect,
    save: Rect,
    clear: Rect,
    auto: Rect,
    widgets: Rect,
    full: Rect,
}

impl Layout {
    fn full(probe: &Probe) -> Self {
        let n = probe.clusters.len() + usize::from(probe.gpu.is_some());
        let top = 556.0;
        let pitch = if n == 0 { 72.0 } else { (252.0 / n as f32).min(72.0) };
        let rows = (0..n)
            .map(|i| {
                let y = top + i as f32 * pitch;
                SliderRow {
                    y,
                    track: rect(520.0, y + 12.0, 1040.0, 16.0),
                    hit: rect(110.0, y, 1650.0, 54.0),
                }
            })
            .collect();
        Layout {
            sheet: rect(60.0, 40.0, 1800.0, 1000.0),
            close: rect(1740.0, 62.0, 68.0, 68.0),
            tiers: [
                rect(110.0, 388.0, 250.0, 64.0),
                rect(374.0, 388.0, 250.0, 64.0),
                rect(638.0, 388.0, 250.0, 64.0),
                rect(902.0, 388.0, 250.0, 64.0),
            ],
            target_track: rect(390.0, 498.0, 1230.0, 18.0),
            target_hit: rect(370.0, 476.0, 1270.0, 58.0),
            rows,
            apply: rect(110.0, 936.0, 280.0, 70.0),
            reset: rect(406.0, 936.0, 280.0, 70.0),
            save: rect(1160.0, 848.0, 220.0, 62.0),
            clear: rect(1394.0, 848.0, 150.0, 62.0),
            auto: rect(620.0, 852.0, 64.0, 40.0),
            widgets: rect(0.0, 0.0, 0.0, 0.0),
            full: rect(0.0, 0.0, 0.0, 0.0),
        }
    }

    fn bottom(probe: &Probe) -> Self {
        let n = probe.clusters.len() + usize::from(probe.gpu.is_some());
        let top = 600.0;
        let pitch = if n == 0 { 58.0 } else { (290.0 / n as f32).min(58.0) };
        let rows = (0..n)
            .map(|i| {
                let y = top + i as f32 * pitch;
                SliderRow {
                    y,
                    track: rect(1240.0, y + 13.0, 400.0, 14.0),
                    hit: rect(1024.0, y, 800.0, 50.0),
                }
            })
            .collect();
        Layout {
            sheet: rect(96.0, 528.0, 1728.0, 518.0),
            close: rect(0.0, 0.0, 0.0, 0.0),
            tiers: [
                rect(130.0, 602.0, 210.0, 56.0),
                rect(354.0, 602.0, 210.0, 56.0),
                rect(578.0, 602.0, 210.0, 56.0),
                rect(802.0, 602.0, 210.0, 56.0),
            ],
            target_track: rect(330.0, 704.0, 500.0, 14.0),
            target_hit: rect(300.0, 684.0, 560.0, 56.0),
            rows,
            apply: rect(130.0, 900.0, 190.0, 62.0),
            reset: rect(334.0, 900.0, 190.0, 62.0),
            save: rect(538.0, 900.0, 190.0, 62.0),
            clear: rect(0.0, 0.0, 0.0, 0.0),
            auto: rect(1206.0, 908.0, 54.0, 40.0),
            widgets: rect(976.0, 900.0, 200.0, 62.0),
            full: rect(742.0, 900.0, 220.0, 62.0),
        }
    }
}

fn hit_tier(layout: &Layout, x: f32, y: f32) -> Option<Tier> {
    for (i, r) in layout.tiers.iter().enumerate() {
        if inside(*r, x, y) {
            return Some(match i {
                0 => Tier::Max,
                1 => Tier::Balanced,
                2 => Tier::Saver,
                _ => Tier::Custom,
            });
        }
    }
    None
}

// ------------------------------------------------------------------ drawing

pub fn draw_full(out: &mut Vec<Cmd>, ctx: &Ctx, pulse: &PulseState, battery: u8, game: &str) {
    out.push(Cmd::RoundRect {
        rect: rect(0.0, 0.0, ctx.w, ctx.h),
        radius: 0.0,
        fill: Some(Fill::Solid(Color::from_rgba8(4, 6, 14, 196))),
        stroke: None,
    });
    let l = Layout::full(&pulse.probe);
    out.push(Cmd::RoundRect {
        rect: l.sheet,
        radius: ctx.d(44.0),
        fill: Some(Fill::Solid(Color::from_rgba8(11, 13, 21, 246))),
        stroke: Some((1.4 * ctx.s, white(26))),
    });
    // A faint accent rule under the header keeps the panel in the launcher's
    // visual language without stealing attention from the readings.
    out.push(Cmd::RoundRect {
        rect: rect(110.0, 182.0, 1700.0, 2.0),
        radius: 0.0,
        fill: Some(Fill::Solid(Color::from_rgba8(180, 92, 255, 110))),
        stroke: None,
    });

    label(out, ctx, "PULSE", 110.0, 126.0, 54.0, white(250), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        "Performance utility · no root",
        112.0,
        166.0,
        24.0,
        white(150),
        Weight::Regular,
        Align::Left,
    );

    // Status pill, top right.
    let pill = ctx.r(1160.0, 78.0, 560.0, 56.0);
    out.push(Cmd::RoundRect {
        rect: pill,
        radius: pill.height() * 0.5,
        fill: Some(Fill::Solid(if pulse.probe.available {
            Color::from_rgba8(0xB4, 0x5C, 0xFF, 42)
        } else {
            Color::from_rgba8(0xFF, 0x6B, 0x6B, 38)
        })),
        stroke: Some((1.2 * ctx.s, white(36))),
    });
    let status = text::ellipsize(&pulse.status, ctx.d(500.0), ctx.d(22.0), Weight::Medium);
    out.push(Cmd::Text {
        text: status,
        x: pill.left() + pill.width() * 0.5,
        baseline: pill.top() + pill.height() * 0.5 + ctx.d(8.0),
        size: ctx.d(22.0),
        color: if pulse.status_ok { white(232) } else { Palette::DANGER() },
        weight: Weight::Medium,
        align: Align::Center,
    });
    // Close.
    out.push(Cmd::Circle {
        cx: ctx.x(1774.0),
        cy: ctx.y(96.0),
        radius: ctx.d(32.0),
        fill: Some(Fill::Solid(white(24))),
        stroke: Some((1.2 * ctx.s, white(44))),
    });
    glyph_at(out, ctx, Glyph::Close, 1774.0, 96.0, 30.0, white(230));

    // Telemetry: four cards.
    let telemetry = [
        (
            "SOC TEMP",
            temp_text(pulse.telemetry.soc_temp),
            pulse.telemetry.soc_temp.map(temp_color).unwrap_or(white(220)),
        ),
        (
            "CPU CLOCK",
            fmt_khz(pulse.telemetry.cpu_khz.iter().copied().max().unwrap_or(0)),
            white(240),
        ),
        ("GPU CLOCK", fmt_khz(pulse.telemetry.gpu_khz), white(240)),
        (
            "BATTERY",
            format!(
                "{}%",
                pulse.telemetry.battery.unwrap_or(battery).min(100)
            ),
            white(240),
        ),
    ];
    for (i, (name, value, color)) in telemetry.iter().enumerate() {
        let r = ctx.r(110.0 + i as f32 * 434.0, 200.0, 420.0, 116.0);
        out.push(Cmd::RoundRect {
            rect: r,
            radius: ctx.d(24.0),
            fill: Some(Fill::Vertical(Gradient {
                top: white(30),
                bottom: white(14),
            })),
            stroke: Some((1.2 * ctx.s, white(28))),
        });
        label(out, ctx, name, 134.0 + i as f32 * 434.0, 240.0, 20.0, white(130), Weight::Medium, Align::Left);
        label(out, ctx, value, 134.0 + i as f32 * 434.0, 288.0, 34.0, *color, Weight::Medium, Align::Left);
    }

    label(out, ctx, "Power tier", 110.0, 372.0, 22.0, white(140), Weight::Regular, Align::Left);
    for (i, tier) in Tier::ALL.iter().enumerate() {
        chip(
            out,
            ctx,
            l.tiers[i],
            tier.label(),
            pulse.tier == *tier,
            pulse.probe.available,
        );
    }

    label(out, ctx, "Power target", 110.0, 510.0, 22.0, white(150), Weight::Regular, Align::Left);
    let ratio = ((pulse.target.clamp(20, 100) - 20) as f32) / 80.0;
    slider(out, ctx, l.target_track, ratio, pulse.probe.available);
    label(
        out,
        ctx,
        &format!("{}%", pulse.target),
        1650.0,
        512.0,
        28.0,
        white(238),
        Weight::Medium,
        Align::Left,
    );

    // Per-cluster sliders, GPU last.
    for (i, row) in l.rows.iter().enumerate() {
        let (name, value, min_label, max_label, ratio) = if let Some(cluster) = pulse.probe.clusters.get(i) {
            let cap = pulse.caps.get(i).copied().unwrap_or(cluster.max);
            (
                cluster.label.clone(),
                cap,
                fmt_khz(cluster.min),
                fmt_khz(cluster.max),
                row_ratio(cluster.min, cluster.max, cap),
            )
        } else if let Some(gpu) = &pulse.probe.gpu {
            (
                "GPU".to_string(),
                pulse.gpu_khz,
                fmt_khz(gpu.min_khz),
                fmt_khz(gpu.max_khz),
                row_ratio(gpu.min_khz, gpu.max_khz, pulse.gpu_khz),
            )
        } else {
            continue;
        };
        let _ = min_label;
        let _ = max_label;
        let name = text::ellipsize(&name, ctx.d(380.0), ctx.d(24.0), Weight::Regular);
        label(out, ctx, &name, 110.0, row.y + 30.0, 24.0, white(220), Weight::Regular, Align::Left);
        slider(out, ctx, row.track, ratio, pulse.probe.available);
        label(
            out,
            ctx,
            &fmt_khz(value),
            1600.0,
            row.y + 30.0,
            24.0,
            white(238),
            Weight::Medium,
            Align::Left,
        );
        // Lowest/highest step as tiny end labels, like PULSE's sliders.
        label(
            out,
            ctx,
            &fmt_khz(if i < pulse.probe.clusters.len() {
                pulse.probe.clusters[i].min
            } else {
                pulse.probe.gpu.as_ref().map(|g| g.min_khz).unwrap_or(0)
            }),
            520.0,
            row.y + 48.0,
            16.0,
            white(92),
            Weight::Regular,
            Align::Left,
        );
        label(
            out,
            ctx,
            &fmt_khz(if i < pulse.probe.clusters.len() {
                pulse.probe.clusters[i].max
            } else {
                pulse.probe.gpu.as_ref().map(|g| g.max_khz).unwrap_or(0)
            }),
            1560.0,
            row.y + 48.0,
            16.0,
            white(92),
            Weight::Regular,
            Align::Right,
        );
    }

    // Per-game profile row.
    let game_label = if game.is_empty() { "No game selected" } else { game };
    let profile_label = text::ellipsize(
        &format!("Profile · {game_label}"),
        ctx.d(470.0),
        ctx.d(24.0),
        Weight::Regular,
    );
    label(out, ctx, &profile_label, 110.0, 884.0, 24.0, white(215), Weight::Regular, Align::Left);
    toggle(out, ctx, l.auto, pulse.auto_game, true);
    label(
        out,
        ctx,
        "Auto-apply on launch",
        700.0,
        882.0,
        20.0,
        white(165),
        Weight::Regular,
        Align::Left,
    );
    let save_enabled = pulse.probe.available && !game.is_empty();
    button(out, ctx, l.save, "Save for game", false, save_enabled);
    button(out, ctx, l.clear, "Clear", false, !game.is_empty());
    if game.is_empty() {
        label(
            out,
            ctx,
            "Select a card on HOME to bind a profile.",
            1200.0,
            832.0,
            18.0,
            white(110),
            Weight::Regular,
            Align::Left,
        );
    }

    button(out, ctx, l.apply, "Apply", true, pulse.probe.available);
    button(out, ctx, l.reset, "Reset to stock", false, pulse.probe.available);
    label(
        out,
        ctx,
        "No root · PServer · credit keiretrogaming/pulse",
        1830.0,
        984.0,
        18.0,
        white(92),
        Weight::Regular,
        Align::Right,
    );
}

pub fn draw_bottom(out: &mut Vec<Cmd>, ctx: &Ctx, pulse: &PulseState, battery: u8, game: &str) {
    let l = Layout::bottom(&pulse.probe);
    glass(out, ctx, l.sheet, ctx.d(34.0), 1.0);
    label(out, ctx, "PULSE", 130.0, 580.0, 38.0, white(248), Weight::Medium, Align::Left);
    label(
        out,
        ctx,
        &text::ellipsize(&pulse.status, ctx.d(560.0), ctx.d(20.0), Weight::Regular),
        330.0,
        580.0,
        20.0,
        if pulse.status_ok { white(150) } else { Palette::DANGER() },
        Weight::Regular,
        Align::Left,
    );
    let temps = format!(
        "{}  ·  GPU {}  ·  {}%",
        temp_text(pulse.telemetry.soc_temp),
        fmt_khz(pulse.telemetry.gpu_khz),
        pulse.telemetry.battery.unwrap_or(battery).min(100)
    );
    label(out, ctx, &temps, 1790.0, 580.0, 22.0, white(200), Weight::Regular, Align::Right);

    for (i, tier) in Tier::ALL.iter().enumerate() {
        chip(
            out,
            ctx,
            l.tiers[i],
            tier.label(),
            pulse.tier == *tier,
            pulse.probe.available,
        );
    }
    label(out, ctx, "Power target", 130.0, 724.0, 20.0, white(150), Weight::Regular, Align::Left);
    let ratio = ((pulse.target.clamp(20, 100) - 20) as f32) / 80.0;
    slider(out, ctx, l.target_track, ratio, pulse.probe.available);
    label(
        out,
        ctx,
        &format!("{}%", pulse.target),
        850.0,
        724.0,
        22.0,
        white(238),
        Weight::Medium,
        Align::Left,
    );

    for (i, row) in l.rows.iter().enumerate() {
        let (name, value, ratio) = if let Some(cluster) = pulse.probe.clusters.get(i) {
            let cap = pulse.caps.get(i).copied().unwrap_or(cluster.max);
            (cluster.label.clone(), cap, row_ratio(cluster.min, cluster.max, cap))
        } else if let Some(gpu) = &pulse.probe.gpu {
            (
                "GPU".to_string(),
                pulse.gpu_khz,
                row_ratio(gpu.min_khz, gpu.max_khz, pulse.gpu_khz),
            )
        } else {
            continue;
        };
        let name = text::ellipsize(&name, ctx.d(180.0), ctx.d(20.0), Weight::Regular);
        label(out, ctx, &name, 1040.0, row.y + 28.0, 20.0, white(205), Weight::Regular, Align::Left);
        slider(out, ctx, row.track, ratio, pulse.probe.available);
        label(
            out,
            ctx,
            &fmt_khz(value),
            1790.0,
            row.y + 28.0,
            20.0,
            white(238),
            Weight::Medium,
            Align::Right,
        );
    }

    button(out, ctx, l.apply, "Apply", true, pulse.probe.available);
    button(out, ctx, l.reset, "Reset", false, pulse.probe.available);
    button(out, ctx, l.save, "Save", false, pulse.probe.available && !game.is_empty());
    button(out, ctx, l.full, "Full PULSE", false, true);
    button(out, ctx, l.widgets, "Widgets", false, true);
    toggle(out, ctx, l.auto, pulse.auto_game, true);
    label(
        out,
        ctx,
        "Auto",
        1270.0,
        938.0,
        20.0,
        white(165),
        Weight::Regular,
        Align::Left,
    );
}

fn temp_color(temp: f32) -> Color {
    if temp >= 85.0 {
        Palette::DANGER()
    } else if temp >= 72.0 {
        Palette::WARN()
    } else {
        Palette::GOOD()
    }
}

fn row_ratio(min: u32, max: u32, value: u32) -> f32 {
    if max <= min {
        1.0
    } else {
        ((value.clamp(min, max) - min) as f32) / ((max - min) as f32)
    }
}

fn slider(out: &mut Vec<Cmd>, ctx: &Ctx, track: Rect, ratio: f32, enabled: bool) {
    let ratio = ratio.clamp(0.0, 1.0);
    let track = ctx.r(track.left(), track.top(), track.width(), track.height());
    out.push(Cmd::RoundRect {
        rect: track,
        radius: track.height() * 0.5,
        fill: Some(Fill::Solid(white(if enabled { 34 } else { 18 }))),
        stroke: None,
    });
    out.push(Cmd::RoundRect {
        rect: rect(track.left(), track.top(), track.width() * ratio, track.height()),
        radius: track.height() * 0.5,
        fill: Some(Fill::Solid(if enabled {
            Color::from_rgba8(0xB4, 0x5C, 0xFF, 225)
        } else {
            white(40)
        })),
        stroke: None,
    });
    out.push(Cmd::Circle {
        cx: track.left() + track.width() * ratio,
        cy: track.top() + track.height() * 0.5,
        radius: ctx.d(17.0),
        fill: Some(Fill::Solid(if enabled { white(250) } else { white(90) })),
        stroke: Some((1.2 * ctx.s, white(70))),
    });
}

fn chip(out: &mut Vec<Cmd>, ctx: &Ctx, r: Rect, text: &str, selected: bool, enabled: bool) {
    let r = ctx.r(r.left(), r.top(), r.width(), r.height());
    out.push(Cmd::RoundRect {
        rect: r,
        radius: r.height() * 0.5,
        fill: Some(Fill::Solid(if selected {
            if enabled {
                Color::from_rgba8(0xB4, 0x5C, 0xFF, 235)
            } else {
                white(46)
            }
        } else {
            white(if enabled { 28 } else { 14 })
        })),
        stroke: Some((1.2 * ctx.s, white(if selected { 70 } else { 34 }))),
    });
    out.push(Cmd::Text {
        text: text.into(),
        x: r.left() + r.width() * 0.5,
        baseline: r.top() + r.height() * 0.5 + ctx.d(10.0),
        size: ctx.d(26.0),
        color: if selected { white(252) } else { white(if enabled { 210 } else { 90 }) },
        weight: Weight::Medium,
        align: Align::Center,
    });
}

fn button(out: &mut Vec<Cmd>, ctx: &Ctx, r: Rect, text: &str, primary: bool, enabled: bool) {
    let r = ctx.r(r.left(), r.top(), r.width(), r.height());
    let (fill, stroke, color) = if !enabled {
        (white(16), white(22), white(90))
    } else if primary {
        (white(240), white(0), Color::from_rgba8(18, 14, 28, 255))
    } else {
        (white(30), white(46), white(232))
    };
    out.push(Cmd::RoundRect {
        rect: r,
        radius: r.height() * 0.5,
        fill: Some(Fill::Solid(fill)),
        stroke: Some((1.2 * ctx.s, stroke)),
    });
    out.push(Cmd::Text {
        text: text.into(),
        x: r.left() + r.width() * 0.5,
        baseline: r.top() + r.height() * 0.5 + ctx.d(9.0),
        size: ctx.d(24.0),
        color,
        weight: Weight::Medium,
        align: Align::Center,
    });
}

fn toggle(out: &mut Vec<Cmd>, ctx: &Ctx, r: Rect, on: bool, enabled: bool) {
    let track = ctx.r(r.left(), r.top(), r.width(), r.height());
    out.push(Cmd::RoundRect {
        rect: track,
        radius: track.height() * 0.5,
        fill: Some(Fill::Solid(if on {
            if enabled {
                Color::from_rgba8(0xB4, 0x5C, 0xFF, 235)
            } else {
                white(44)
            }
        } else {
            white(46)
        })),
        stroke: None,
    });
    let knob = track.height() * 0.5 - ctx.d(4.0);
    out.push(Cmd::Circle {
        cx: if on {
            track.right() - track.height() * 0.5
        } else {
            track.left() + track.height() * 0.5
        },
        cy: track.top() + track.height() * 0.5,
        radius: knob,
        fill: Some(Fill::Solid(white(if enabled { 250 } else { 120 }))),
        stroke: None,
    });
}

pub fn demo_probe() -> Probe {
    let clusters = vec![
        Cluster {
            id: 0,
            label: "Cluster 0 · 4 cores".into(),
            min: 300_000,
            max: 1_785_600,
            current: 1_785_600,
            freqs: vec![300_000, 691_200, 1_036_800, 1_497_600, 1_785_600],
            cur_path: String::new(),
            max_path: String::new(),
            min_path: String::new(),
        },
        Cluster {
            id: 4,
            label: "Cluster 1 · 3 cores".into(),
            min: 710_400,
            max: 2_496_000,
            current: 2_496_000,
            freqs: vec![710_400, 1_209_600, 1_708_800, 2_150_400, 2_496_000],
            cur_path: String::new(),
            max_path: String::new(),
            min_path: String::new(),
        },
        Cluster {
            id: 7,
            label: "Cluster 2 · 1 core".into(),
            min: 844_800,
            max: 3_360_000,
            current: 3_360_000,
            freqs: vec![844_800, 1_440_000, 2_016_000, 2_688_000, 3_360_000],
            cur_path: String::new(),
            max_path: String::new(),
            min_path: String::new(),
        },
    ];
    Probe {
        available: true,
        error: String::new(),
        device: "AYN Thor".into(),
        soc_model: "QCS8550".into(),
        clusters,
        gpu: Some(Gpu {
            min_khz: 160_000,
            max_khz: 818_000,
            current_khz: 818_000,
            levels: vec![160_000, 220_000, 305_000, 415_000, 519_000, 650_000, 818_000],
            cur_path: String::new(),
        }),
        ..Probe::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_picks_nearest_and_prefers_lower_on_a_tie() {
        let freqs = [300_000, 600_000, 900_000];
        assert_eq!(snap(&freqs, 590_000), 600_000);
        assert_eq!(snap(&freqs, 450_000), 300_000);
        assert_eq!(snap(&[], 123), 123);
    }

    #[test]
    fn tier_scales_every_cluster_and_the_gpu() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.set_tier(Tier::Balanced);
        assert_eq!(pulse.target, 75);
        assert_eq!(pulse.caps.len(), 3);
        // 75% of 1785600 = 1339200 -> nearest supported 1497600 or 1036800; 1036800 is closer.
        assert!(pulse.caps[0] < 1_785_600);
        assert!(pulse.gpu_khz < 818_000);
    }

    #[test]
    fn dragging_a_cluster_marks_custom_and_retargets() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.set_tier(Tier::Max);
        let row = Layout::full(&pulse.probe).rows[1].track;
        pulse.drag = Some(Drag::Row(1));
        pulse.drag_changed = false;
        pulse.drag_row_to(1, 0.0);
        assert_eq!(pulse.tier, Tier::Custom);
        assert!(pulse.caps[1] <= 710_400);
        assert!(pulse.target < 100);
        let _ = row;
    }

    #[test]
    fn profiles_round_trip_through_the_prefs_format() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.set_tier(Tier::Saver);
        pulse.caps[0] = 1_036_800;
        pulse.gpu_khz = 415_000;
        pulse.auto_game = false;
        pulse.save_profile("com.example.game", "Example");
        let decoded = PulseState::decode(&pulse.encode());
        assert!(!decoded.auto_game);
        assert_eq!(decoded.caps, pulse.caps);
        assert_eq!(decoded.gpu_khz, pulse.gpu_khz);
        let profile = decoded.profiles.get("com.example.game").expect("profile");
        assert_eq!(profile.caps, pulse.caps);
        assert_eq!(profile.tier, Tier::Saver.code());
    }

    #[test]
    fn game_profile_applies_on_launch_only_when_enabled() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.caps = vec![691_200, 1_209_600, 1_440_000];
        pulse.gpu_khz = 305_000;
        pulse.save_profile("com.example.game", "Example");
        let json = pulse.game_apply_json("com.example.game").expect("apply json");
        assert!(json.contains("\"clusters\""));
        pulse.auto_game = false;
        assert!(pulse.game_apply_json("com.example.game").is_none());
    }

    #[test]
    fn unavailable_device_never_builds_an_apply_script() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(Probe {
            available: false,
            error: "no pserver".into(),
            ..Probe::default()
        });
        assert!(pulse.plan_json(false).is_none());
        pulse.reset_values();
        assert!(pulse.plan_json(true).is_none());
        assert!(!pulse.status_ok);
    }

    #[test]
    fn full_panel_buttons_are_inside_the_sheet() {
        let layout = Layout::full(&demo_probe());
        for r in [layout.apply, layout.reset, layout.save, layout.auto]
            .iter()
            .chain(layout.tiers.iter())
        {
            assert!(r.left() >= layout.sheet.left());
            assert!(r.right() <= layout.sheet.right());
            assert!(r.top() >= layout.sheet.top());
            assert!(r.bottom() <= layout.sheet.bottom());
        }
        for row in &layout.rows {
            assert!(row.track.right() <= layout.sheet.right());
        }
    }

    #[test]
    fn bottom_panel_controls_fit_the_dual_area() {
        let layout = Layout::bottom(&demo_probe());
        assert_eq!(layout.sheet.top(), 528.0);
        assert_eq!(layout.sheet.bottom(), 1046.0);
        for row in &layout.rows {
            assert!(row.hit.right() <= layout.sheet.right());
        }
        // Tier chips and the slider column must never share a hit area.
        for tier in &layout.tiers {
            for row in &layout.rows {
                assert!(tier.right() <= row.hit.left());
            }
        }
    }

    #[test]
    fn tapping_a_tier_returns_an_apply_script() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.open = true;
        let hit = pulse.hit_full(400.0, 420.0, Some(("Game", "com.example.game")));
        match hit {
            PulseHit::Apply(json) => assert!(json.contains("\"clusters\"")),
            _ => panic!("expected an apply"),
        }
        assert_eq!(pulse.tier, Tier::Balanced);
        assert_eq!(pulse.target, 75);
    }

    #[test]
    fn slider_drag_updates_the_cap_and_release_applies_it() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.open = true;
        let layout = Layout::full(&pulse.probe);
        let row = &layout.rows[0];
        let x = row.track.left() + row.track.width() * 0.25;
        let y = row.track.top() + row.track.height() * 0.5;
        assert!(pulse.press(x, y));
        pulse.drag_to(row.track.left() + row.track.width() * 0.5);
        let json = pulse.release().expect("apply after drag");
        assert!(json.contains("\"clusters\""));
        assert_eq!(pulse.tier, Tier::Custom);
        assert!(pulse.caps[0] > 691_200);
    }

    #[test]
    fn save_and_clear_bind_the_profile_to_the_selected_game() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.open = true;
        let save = Layout::full(&pulse.probe).save;
        let hit = pulse.hit_full(
            save.left() + 10.0,
            save.top() + 10.0,
            Some(("Game", "com.example.game")),
        );
        assert!(matches!(hit, PulseHit::Changed));
        assert!(pulse.has_profile("com.example.game"));
        let clear = Layout::full(&pulse.probe).clear;
        pulse.hit_full(
            clear.left() + 10.0,
            clear.top() + 10.0,
            Some(("Game", "com.example.game")),
        );
        assert!(!pulse.has_profile("com.example.game"));
    }

    #[test]
    fn widgets_button_leaves_the_bottom_panel() {
        let mut pulse = PulseState::new();
        pulse.apply_probe(demo_probe());
        pulse.bottom = true;
        let widgets = Layout::bottom(&pulse.probe).widgets;
        let hit = pulse.hit_bottom(
            widgets.left() + 10.0,
            widgets.top() + 10.0,
            Some(("Game", "com.example.game")),
        );
        assert!(matches!(hit, PulseHit::Changed));
        assert!(!pulse.bottom);
    }
}
