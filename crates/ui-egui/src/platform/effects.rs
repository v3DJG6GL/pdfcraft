//! Reduced effects: drawing less while a software rasterizer draws the window.
//!
//! Without a graphics processor (a virtual machine, a Remote Desktop or Citrix session), the
//! window is drawn by the CPU: wgpu reports a CPU adapter, WARP on Windows or llvmpipe and
//! lavapipe elsewhere. Measured on lavapipe, a frame of a 1440×920 window costs about 100 ms of
//! CPU against 2 ms on a GPU, almost all of it filling pixels, so the cost follows the area
//! drawn (#1582). In reduced mode the window drops what costs pixels and adds nothing a reader
//! needs: page drop shadows and the paper under a loaded page, the viewport's edge fades, the
//! shadows of windows, menus and floating bars, the text cursor's blink, and the animations
//! the system setting allows ([`crate::motion::AnimationEffects::System`] acts as Off; an
//! explicit On stays on).
//!
//! The desktop app decides: it tells the app which adapter draws and applies an override from
//! `PDFCRAFT_REDUCED_EFFECTS` ([`ReducedEffects::from_env`]). Nothing here reads the
//! environment, so tests, screenshots and the web build never change behaviour by themselves.
//! None of this is saved except which adapter the last start drew on ([`KEY`]): egui's
//! dithering is fixed before the adapter is known, so the next start turns it off from that
//! ([`ReducedEffects::dithering_at_start`]).
//!
//! Cheaper frames alone don't lower the CPU while the pointer moves: egui draws a frame for every
//! move, and over Remote Desktop moves arrive faster than a software rasterizer draws, so frames
//! run back to back. With reduced effects the desktop app also caps the frame rate
//! ([`FrameCap`], [`DEFAULT_MAX_FPS`] unless `PDFCRAFT_MAX_FPS` says otherwise): input that
//! arrives meanwhile waits in the system's queue and is handled in the next frame.

use web_time::{Duration, Instant};

/// Whether effects are reduced: decided by the adapter, or forced either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReducedEffects {
    /// Reduced while a software rasterizer draws the window.
    #[default]
    Auto,
    On,
    Off,
}

impl ReducedEffects {
    pub const ALL: [Self; 3] = [Self::Auto, Self::On, Self::Off];

    /// The name on the control channel and the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|choice| choice.name().eq_ignore_ascii_case(value.trim()))
    }

    /// `PDFCRAFT_REDUCED_EFFECTS`: `1` forces reduced effects, `0` forbids them, and anything
    /// else (unset included) leaves them to the adapter.
    pub fn from_env(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("1") => Self::On,
            Some("0") => Self::Off,
            _ => Self::Auto,
        }
    }

    /// Whether egui dithers this start. It is fixed before the adapter is known, so on its own
    /// it follows the adapter the last start drew on.
    pub fn dithering_at_start(self, last_start_software: bool) -> bool {
        match self {
            Self::On => false,
            Self::Off => true,
            Self::Auto => !last_start_software,
        }
    }
}

/// The saved-settings key for whether the last start drew on a software rasterizer.
pub const KEY: &str = "last_start_software_renderer";

/// Whether saved settings say the last start drew on a software rasterizer; false for anything
/// missing or malformed.
pub fn saved(settings: &serde_json::Value) -> bool {
    settings.get(KEY).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

/// Frames a second while effects are reduced, unless `PDFCRAFT_MAX_FPS` says otherwise: Remote
/// Desktop's own default rate, so nothing it would show is lost.
pub const DEFAULT_MAX_FPS: u32 = 30;

/// The most frames a second [`FrameCap`] allows.
pub const MAX_FPS_LIMIT: u32 = 240;

/// `PDFCRAFT_MAX_FPS`: frames a second while effects are reduced, `0` for no cap. `None` for
/// anything else (unset, not a number, above [`MAX_FPS_LIMIT`]).
pub fn max_fps_from_env(value: Option<&str>) -> Option<u32> {
    value?.trim().parse::<u32>().ok().filter(|fps| *fps <= MAX_FPS_LIMIT)
}

/// The shortest time between two frames while effects are reduced. Only the desktop app installs
/// one ([`Effects::frame_cap`]): tests, screenshots and the web build never wait.
#[derive(Debug, Clone)]
pub struct FrameCap {
    interval: Duration,
    last: Option<Instant>,
    /// The window size at the last frame: a resize is drawn at once.
    size: Option<egui::Vec2>,
}

impl FrameCap {
    /// A cap of `fps` frames a second; `None` for 0 (no cap). Above [`MAX_FPS_LIMIT`] is limited.
    pub fn per_second(fps: u32) -> Option<Self> {
        let fps = fps.min(MAX_FPS_LIMIT);
        (fps > 0).then(|| Self { interval: Duration::from_secs(1) / fps, last: None, size: None })
    }

    /// Frames a second this cap allows.
    pub fn fps(&self) -> u32 {
        let nanos = self.interval.as_nanos().max(1);
        u32::try_from(1_000_000_000 / nanos).unwrap_or(u32::MAX)
    }

    /// How long to wait before a frame of a window of `size` that starts `now`; `None` for none:
    /// the first frame, a resize, or the interval has passed.
    pub(crate) fn wait(&self, now: Instant, size: egui::Vec2) -> Option<Duration> {
        if self.size != Some(size) {
            return None;
        }
        let elapsed = now.checked_duration_since(self.last?)?;
        self.interval.checked_sub(elapsed).filter(|wait| !wait.is_zero())
    }

    /// A frame of a window of `size` started at `at`.
    pub(crate) fn frame_started(&mut self, at: Instant, size: egui::Vec2) {
        self.last = Some(at);
        self.size = Some(size);
    }
}

/// The app's reduced-effects state. Only [`Self::saved_value`] is saved.
#[derive(Debug, Default)]
pub struct Effects {
    /// The override for this session (environment, control channel, command line).
    pub mode: ReducedEffects,
    /// Whether this start's adapter is a software rasterizer; `None` when no adapter was
    /// reported (OpenGL, tests, the web).
    software_renderer: Option<bool>,
    /// What saved settings said about the last start.
    last_start_software: bool,
    /// The value last handed to egui ([`Self::publish`]).
    published: Option<bool>,
    /// The frame cap while effects are reduced; the desktop app installs it.
    pub frame_cap: Option<FrameCap>,
}

impl Effects {
    /// Whether effects are reduced now.
    pub fn reduced(&self) -> bool {
        match self.mode {
            ReducedEffects::On => true,
            ReducedEffects::Off => false,
            ReducedEffects::Auto => self.software_renderer == Some(true),
        }
    }

    /// The adapter of this start, as the desktop app found it.
    pub fn set_software_renderer(&mut self, cpu: bool) {
        self.software_renderer = Some(cpu);
    }

    /// This start's adapter, if one was reported.
    pub fn software_renderer(&self) -> Option<bool> {
        self.software_renderer
    }

    /// What to save: this start's adapter, or, when none was reported, what the last start
    /// said, so an OpenGL start doesn't forget it.
    pub(crate) fn saved_value(&self) -> bool {
        self.software_renderer.unwrap_or(self.last_start_software)
    }

    pub(crate) fn restore(&mut self, settings: &serde_json::Value) {
        self.last_start_software = saved(settings);
    }

    /// How long to wait before this frame: only while effects are reduced and a cap is installed.
    /// Records the frame, so call it once a frame, then wait.
    pub(crate) fn frame_wait(&mut self, now: Instant, size: egui::Vec2) -> Option<Duration> {
        let reduced = self.reduced();
        let cap = self.frame_cap.as_mut()?;
        let wait = if reduced { cap.wait(now, size) } else { None };
        cap.frame_started(now + wait.unwrap_or_default(), size);
        wait
    }

    /// The cap's frames a second while it applies.
    pub fn max_fps(&self) -> Option<u32> {
        self.frame_cap.as_ref().filter(|_| self.reduced()).map(FrameCap::fps)
    }

    /// Hand the current state to egui's context, where the theme and the painting code read it
    /// ([`reduced`]). True when it changed, so the theme is applied again.
    pub(crate) fn publish(&mut self, ctx: &egui::Context) -> bool {
        let reduced = self.reduced();
        if self.published == Some(reduced) {
            return false;
        }
        self.published = Some(reduced);
        ctx.data_mut(|d| d.insert_temp(key(), reduced));
        true
    }
}

/// Return the stable egui id holding whether effects are reduced.
fn key() -> egui::Id {
    egui::Id::unique("pdfcraft-reduced-effects")
}

/// Whether effects are reduced, as last published ([`Effects::publish`]); false before that.
pub(crate) fn reduced(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(key())).unwrap_or(false)
}

#[cfg(test)]
mod tests;
