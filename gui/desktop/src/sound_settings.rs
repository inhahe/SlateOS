//! Sound settings panel.
//!
//! Configures audio output/input devices, master volume, per-app volume,
//! system sounds, spatial audio, and microphone settings. Renders as a
//! sub-page of the desktop's Settings application.

use appearance::Palette;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// Every colour comes out of the resolved [`Palette`] this panel is handed, so
// the shell repaints together when the mode or the accent changes. Four
// judgements the source does not show:
//
// 1. **Three sites follow the accent, and all three are "you are here" or
//    "drag me":** the active tab's label, the label of the selected spatial
//    mode, and the filled part of every volume bar. The first two are radio
//    groups — exactly one member is chosen, and the chosen one is *you are
//    here*. The third is the slider rule: a volume bar is a control the user
//    drags, its track is a surface (`surface1`) and its fill is the accent.
//    Held per-site by `the_three_accent_sites_follow_the_accent`.
//
// 2. **A muted volume bar is red, and stays red.** Mute is the one state that
//    overrides the slider rule: the fill stops saying "this is how far you
//    dragged it" and starts saying "no sound is coming out of this". That is
//    a reading, and a reading is never the accent's — a red-accented desktop
//    would otherwise draw an unmuted bar in the mute colour. Held by
//    `a_muted_volume_bar_never_looks_like_an_unmuted_one`.
//
// 3. **On/off status is a frozen pair, not an accent pair.** The system-sound
//    rows' "On"/"Off" and the mic toggles' pills are green-against-neutral in
//    every accent, because green *means* enabled here rather than decorating
//    it. Held by `nothing_that_reports_a_state_follows_the_accent`.
//
// 4. **Section headings keep `lavender`.** "Microphone Settings" and "Spatial
//    Audio" name a category, and the accent never marks category. Held by the
//    same test as (3).
//
// The green "on" toggles are kept exactly as they were drawn before the
// conversion; making a toggle's on-state follow the accent is a redesign, not
// a conversion, and is noted as such in `known-issues.md`.

// ============================================================================
// Audio device
// ============================================================================

/// Identifier for an audio endpoint.
///
/// 64 bits rather than 32 so that [`IdSeq::issue_infallible`] is available:
/// the sequence that hands these out cannot then run out, so there is no
/// error path for `add_device` to invent and no caller left holding a
/// `None` it has nothing useful to do with.
pub type DeviceId = u64;

/// Kind of audio device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Output,
    Input,
}

/// Audio endpoint device (speaker, headphones, microphone, etc.).
#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    /// Whether this device is currently the system default.
    pub is_default: bool,
    /// Volume 0–100.
    pub volume: u32,
    /// Muted.
    pub muted: bool,
    /// Sample rate in Hz (e.g. 44100, 48000, 96000).
    pub sample_rate: u32,
    /// Bit depth (16, 24, 32).
    pub bit_depth: u32,
    /// Number of channels (1 = mono, 2 = stereo, 6 = 5.1, 8 = 7.1).
    pub channels: u32,
    /// Whether this device is connected / available.
    pub connected: bool,
}

impl AudioDevice {
    pub fn new(id: DeviceId, name: &str, kind: DeviceKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            is_default: false,
            volume: 80,
            muted: false,
            sample_rate: 48000,
            bit_depth: 16,
            channels: 2,
            connected: true,
        }
    }

    pub fn set_volume(&mut self, vol: u32) {
        self.volume = vol.min(100);
    }

    /// Human-readable format string e.g. "48000 Hz / 16-bit / Stereo".
    pub fn format_string(&self) -> String {
        let ch = match self.channels {
            1 => "Mono",
            2 => "Stereo",
            6 => "5.1 Surround",
            8 => "7.1 Surround",
            n => {
                return format!(
                    "{} Hz / {}-bit / {} ch",
                    self.sample_rate, self.bit_depth, n
                );
            }
        };
        format!("{} Hz / {}-bit / {}", self.sample_rate, self.bit_depth, ch)
    }
}

// ============================================================================
// Spatial audio
// ============================================================================

/// Spatial audio mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialAudioMode {
    Off,
    /// Head-related transfer function for headphones.
    HeadphoneHrtf,
    /// Virtual surround for speakers.
    VirtualSurround,
}

impl SpatialAudioMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::HeadphoneHrtf => "Headphone HRTF",
            Self::VirtualSurround => "Virtual Surround",
        }
    }

    pub const ALL: [Self; 3] = [Self::Off, Self::HeadphoneHrtf, Self::VirtualSurround];
}

// ============================================================================
// System sound event
// ============================================================================

/// System sound event that can be configured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemSoundEvent {
    Notification,
    Error,
    Warning,
    DeviceConnect,
    DeviceDisconnect,
    LowBattery,
    Screenshot,
    VolumeChange,
    Startup,
    Shutdown,
    LockScreen,
    UnlockScreen,
}

impl SystemSoundEvent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Notification => "Notification",
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::DeviceConnect => "Device connected",
            Self::DeviceDisconnect => "Device disconnected",
            Self::LowBattery => "Low battery",
            Self::Screenshot => "Screenshot",
            Self::VolumeChange => "Volume change",
            Self::Startup => "Startup",
            Self::Shutdown => "Shutdown",
            Self::LockScreen => "Lock screen",
            Self::UnlockScreen => "Unlock screen",
        }
    }

    pub const ALL: [Self; 12] = [
        Self::Notification,
        Self::Error,
        Self::Warning,
        Self::DeviceConnect,
        Self::DeviceDisconnect,
        Self::LowBattery,
        Self::Screenshot,
        Self::VolumeChange,
        Self::Startup,
        Self::Shutdown,
        Self::LockScreen,
        Self::UnlockScreen,
    ];
}

/// Configuration for one system sound event.
#[derive(Clone, Debug)]
pub struct SystemSoundConfig {
    pub event: SystemSoundEvent,
    /// Whether this event plays a sound.
    pub enabled: bool,
    /// Custom sound file path, or `None` for the built-in default.
    pub custom_sound: Option<String>,
    /// Volume override (0–100), or `None` for system default.
    pub volume_override: Option<u32>,
}

impl SystemSoundConfig {
    pub fn new(event: SystemSoundEvent) -> Self {
        Self {
            event,
            enabled: true,
            custom_sound: None,
            volume_override: None,
        }
    }
}

// ============================================================================
// Per-app volume entry
// ============================================================================

/// Per-application volume override.
#[derive(Clone, Debug)]
pub struct AppVolumeEntry {
    pub app_id: String,
    pub display_name: String,
    /// Volume 0–100.
    pub volume: u32,
    /// Whether this app is individually muted.
    pub muted: bool,
    /// Which output device this app uses, or `None` for system default.
    pub output_device_id: Option<DeviceId>,
}

impl AppVolumeEntry {
    pub fn new(app_id: &str, display_name: &str) -> Self {
        Self {
            app_id: app_id.into(),
            display_name: display_name.into(),
            volume: 100,
            muted: false,
            output_device_id: None,
        }
    }

    pub fn set_volume(&mut self, vol: u32) {
        self.volume = vol.min(100);
    }
}

// ============================================================================
// Microphone settings
// ============================================================================

/// Microphone-specific configuration.
#[derive(Clone, Debug)]
pub struct MicConfig {
    /// Input volume / gain (0–100).
    pub gain: u32,
    /// Whether noise suppression is enabled.
    pub noise_suppression: bool,
    /// Whether echo cancellation is enabled.
    pub echo_cancellation: bool,
    /// Whether automatic gain control is enabled.
    pub auto_gain: bool,
    /// Monitor (loopback) — hear your own mic in the output.
    pub monitor: bool,
    /// Monitor volume (0–100).
    pub monitor_volume: u32,
}

impl Default for MicConfig {
    fn default() -> Self {
        Self {
            gain: 80,
            noise_suppression: true,
            echo_cancellation: true,
            auto_gain: true,
            monitor: false,
            monitor_volume: 50,
        }
    }
}

impl MicConfig {
    pub fn set_gain(&mut self, g: u32) {
        self.gain = g.min(100);
    }

    pub fn set_monitor_volume(&mut self, v: u32) {
        self.monitor_volume = v.min(100);
    }
}

// ============================================================================
// Sound settings manager
// ============================================================================

/// Central sound settings state.
pub struct SoundSettings {
    /// All known audio devices.
    devices: Vec<AudioDevice>,
    /// Per-app volume entries.
    app_volumes: Vec<AppVolumeEntry>,
    /// System sound configurations.
    system_sounds: Vec<SystemSoundConfig>,
    /// Master volume (0–100).
    pub master_volume: u32,
    /// Master mute.
    pub master_muted: bool,
    /// Spatial audio mode.
    pub spatial_mode: SpatialAudioMode,
    /// Whether system sounds are globally enabled.
    pub system_sounds_enabled: bool,
    /// Microphone configuration.
    pub mic: MicConfig,
    /// Source of device IDs.
    ids: IdSeq<DeviceId>,
}

impl SoundSettings {
    pub fn new() -> Self {
        let mut s = Self {
            devices: Vec::new(),
            app_volumes: Vec::new(),
            system_sounds: Vec::new(),
            master_volume: 80,
            master_muted: false,
            spatial_mode: SpatialAudioMode::Off,
            system_sounds_enabled: true,
            mic: MicConfig::default(),
            ids: IdSeq::new(),
        };
        // Populate default system sounds.
        for event in SystemSoundEvent::ALL {
            s.system_sounds.push(SystemSoundConfig::new(event));
        }
        s
    }

    /// Create a pre-populated instance with default devices.
    pub fn with_defaults() -> Self {
        let mut s = Self::new();
        let speakers = s.add_device("Speakers", DeviceKind::Output);
        s.set_default_device(speakers);
        let hdmi = s.add_device("HDMI Audio", DeviceKind::Output);
        let _ = hdmi; // available but not default
        let mic = s.add_device("Built-in Microphone", DeviceKind::Input);
        s.set_default_device(mic);
        s
    }

    // ------------------------------------------------------------------
    // Device management
    // ------------------------------------------------------------------

    pub fn add_device(&mut self, name: &str, kind: DeviceKind) -> DeviceId {
        let id = self.ids.issue_infallible();
        self.devices.push(AudioDevice::new(id, name, kind));
        id
    }

    pub fn remove_device(&mut self, id: DeviceId) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != id);
        self.devices.len() < before
    }

    pub fn get_device(&self, id: DeviceId) -> Option<&AudioDevice> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn get_device_mut(&mut self, id: DeviceId) -> Option<&mut AudioDevice> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    pub fn output_devices(&self) -> Vec<&AudioDevice> {
        self.devices
            .iter()
            .filter(|d| d.kind == DeviceKind::Output)
            .collect()
    }

    pub fn input_devices(&self) -> Vec<&AudioDevice> {
        self.devices
            .iter()
            .filter(|d| d.kind == DeviceKind::Input)
            .collect()
    }

    pub fn set_default_device(&mut self, id: DeviceId) {
        if let Some(dev) = self.devices.iter().find(|d| d.id == id) {
            let kind = dev.kind;
            for d in &mut self.devices {
                if d.kind == kind {
                    d.is_default = d.id == id;
                }
            }
        }
    }

    pub fn default_output(&self) -> Option<&AudioDevice> {
        self.devices
            .iter()
            .find(|d| d.kind == DeviceKind::Output && d.is_default)
    }

    pub fn default_input(&self) -> Option<&AudioDevice> {
        self.devices
            .iter()
            .find(|d| d.kind == DeviceKind::Input && d.is_default)
    }

    pub fn set_device_volume(&mut self, id: DeviceId, vol: u32) {
        if let Some(d) = self.get_device_mut(id) {
            d.set_volume(vol);
        }
    }

    pub fn set_device_muted(&mut self, id: DeviceId, muted: bool) {
        if let Some(d) = self.get_device_mut(id) {
            d.muted = muted;
        }
    }

    pub fn set_device_format(
        &mut self,
        id: DeviceId,
        sample_rate: u32,
        bit_depth: u32,
        channels: u32,
    ) {
        if let Some(d) = self.get_device_mut(id) {
            d.sample_rate = sample_rate;
            d.bit_depth = bit_depth;
            d.channels = channels;
        }
    }

    // ------------------------------------------------------------------
    // Master volume
    // ------------------------------------------------------------------

    pub fn set_master_volume(&mut self, vol: u32) {
        self.master_volume = vol.min(100);
    }

    /// Effective volume for a device: device_vol * master_vol / 100.
    pub fn effective_volume(&self, device_id: DeviceId) -> u32 {
        if self.master_muted {
            return 0;
        }
        let dev_vol = self
            .get_device(device_id)
            .map_or(0, |d| if d.muted { 0 } else { d.volume });
        dev_vol.saturating_mul(self.master_volume) / 100
    }

    // ------------------------------------------------------------------
    // Per-app volume
    // ------------------------------------------------------------------

    pub fn set_app_volume(&mut self, app_id: &str, display_name: &str, volume: u32) {
        if let Some(entry) = self.app_volumes.iter_mut().find(|e| e.app_id == app_id) {
            entry.set_volume(volume);
        } else {
            let mut e = AppVolumeEntry::new(app_id, display_name);
            e.set_volume(volume);
            self.app_volumes.push(e);
        }
    }

    pub fn set_app_muted(&mut self, app_id: &str, muted: bool) {
        if let Some(entry) = self.app_volumes.iter_mut().find(|e| e.app_id == app_id) {
            entry.muted = muted;
        }
    }

    pub fn set_app_device(&mut self, app_id: &str, device_id: Option<DeviceId>) {
        if let Some(entry) = self.app_volumes.iter_mut().find(|e| e.app_id == app_id) {
            entry.output_device_id = device_id;
        }
    }

    pub fn remove_app_volume(&mut self, app_id: &str) -> bool {
        let before = self.app_volumes.len();
        self.app_volumes.retain(|e| e.app_id != app_id);
        self.app_volumes.len() < before
    }

    pub fn app_volumes(&self) -> &[AppVolumeEntry] {
        &self.app_volumes
    }

    pub fn effective_app_volume(&self, app_id: &str) -> u32 {
        if self.master_muted {
            return 0;
        }
        let entry = self.app_volumes.iter().find(|e| e.app_id == app_id);
        let app_vol = entry.map_or(100, |e| if e.muted { 0 } else { e.volume });
        app_vol.saturating_mul(self.master_volume) / 100
    }

    // ------------------------------------------------------------------
    // System sounds
    // ------------------------------------------------------------------

    pub fn get_system_sound(&self, event: SystemSoundEvent) -> Option<&SystemSoundConfig> {
        self.system_sounds.iter().find(|s| s.event == event)
    }

    pub fn set_system_sound_enabled(&mut self, event: SystemSoundEvent, enabled: bool) {
        if let Some(s) = self.system_sounds.iter_mut().find(|s| s.event == event) {
            s.enabled = enabled;
        }
    }

    pub fn set_system_sound_custom(&mut self, event: SystemSoundEvent, path: Option<String>) {
        if let Some(s) = self.system_sounds.iter_mut().find(|s| s.event == event) {
            s.custom_sound = path;
        }
    }

    pub fn set_system_sound_volume(&mut self, event: SystemSoundEvent, vol: Option<u32>) {
        if let Some(s) = self.system_sounds.iter_mut().find(|s| s.event == event) {
            s.volume_override = vol.map(|v| v.min(100));
        }
    }

    pub fn should_play_sound(&self, event: SystemSoundEvent) -> bool {
        if !self.system_sounds_enabled || self.master_muted {
            return false;
        }
        self.system_sounds
            .iter()
            .find(|s| s.event == event)
            .is_some_and(|s| s.enabled)
    }

    pub fn system_sounds_list(&self) -> &[SystemSoundConfig] {
        &self.system_sounds
    }

    // ------------------------------------------------------------------
    // Spatial audio
    // ------------------------------------------------------------------

    pub fn set_spatial_mode(&mut self, mode: SpatialAudioMode) {
        self.spatial_mode = mode;
    }
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Settings panel rendering
// ============================================================================

/// Render state for the sound settings panel.
pub struct SoundSettingsUI {
    settings: SoundSettings,
    /// Active tab: 0=Output, 1=Input, 2=App Volumes, 3=System Sounds, 4=Spatial.
    active_tab: usize,
}

impl SoundSettingsUI {
    pub fn new() -> Self {
        Self {
            settings: SoundSettings::with_defaults(),
            active_tab: 0,
        }
    }

    pub fn with_settings(settings: SoundSettings) -> Self {
        Self {
            settings,
            active_tab: 0,
        }
    }

    pub fn settings(&self) -> &SoundSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut SoundSettings {
        &mut self.settings
    }

    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub fn set_active_tab(&mut self, tab: usize) {
        if tab <= 4 {
            self.active_tab = tab;
        }
    }

    const TAB_LABELS: [&'static str; 5] = [
        "Output",
        "Input",
        "App Volumes",
        "System Sounds",
        "Spatial Audio",
    ];

    /// Render the sound settings panel.
    pub fn render(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let mut cy = y;

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: 800.0,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "Sound Settings".into(),
            font_size: 20.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 32.0;

        // Master volume bar
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: format!(
                "Master Volume: {}%{}",
                self.settings.master_volume,
                if self.settings.master_muted {
                    " (Muted)"
                } else {
                    ""
                }
            ),
            font_size: 14.0,
            color: if self.settings.master_muted {
                p.red
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 22.0;
        cy = Self::render_volume_bar(
            p,
            &mut cmds,
            x + pad,
            cy,
            inner,
            self.settings.master_volume,
            self.settings.master_muted,
        );
        cy += 12.0;

        // Tab bar
        let tab_w = inner / Self::TAB_LABELS.len() as f32;
        for (i, label) in Self::TAB_LABELS.iter().enumerate() {
            let tx = x + pad + tab_w * i as f32;
            let active = self.active_tab == i;
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: cy,
                width: tab_w - 2.0,
                height: 32.0,
                color: if active { p.surface0 } else { p.mantle },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: cy + 8.0,
                text: (*label).into(),
                font_size: 12.0,
                color: if active { p.accent } else { p.subtext0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        cy += 40.0;

        // Tab content
        match self.active_tab {
            0 => cy = self.render_output_tab(p, &mut cmds, x + pad, cy, inner),
            1 => cy = self.render_input_tab(p, &mut cmds, x + pad, cy, inner),
            2 => cy = self.render_app_volumes_tab(p, &mut cmds, x + pad, cy, inner),
            3 => cy = self.render_system_sounds_tab(p, &mut cmds, x + pad, cy, inner),
            4 => cy = self.render_spatial_tab(p, &mut cmds, x + pad, cy, inner),
            _ => {}
        }

        let _ = cy;
        cmds
    }

    // ------------------------------------------------------------------
    // Tab renderers
    // ------------------------------------------------------------------

    fn render_output_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let devices = self.settings.output_devices();
        if devices.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "No output devices detected.".into(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            return y + 24.0;
        }

        for dev in &devices {
            let bg = if dev.is_default { p.surface0 } else { p.mantle };
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 64.0,
                color: bg,
                corner_radii: CornerRadii::all(6.0),
            });

            let name_suffix = if dev.is_default { " ✓ Default" } else { "" };
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 8.0,
                text: format!("{}{}", dev.name, name_suffix),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 28.0,
                text: dev.format_string(),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Volume bar
            Self::render_volume_bar(
                p,
                cmds,
                x + 12.0,
                y + 46.0,
                width - 24.0,
                dev.volume,
                dev.muted,
            );

            y += 72.0;
        }
        y
    }

    fn render_input_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let devices = self.settings.input_devices();
        if devices.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "No input devices detected.".into(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            return y + 24.0;
        }

        for dev in &devices {
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 48.0,
                color: if dev.is_default { p.surface0 } else { p.mantle },
                corner_radii: CornerRadii::all(6.0),
            });
            let def_txt = if dev.is_default { " ✓ Default" } else { "" };
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 8.0,
                text: format!("{}{}", dev.name, def_txt),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 28.0,
                text: dev.format_string(),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 56.0;
        }

        // Mic settings
        y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Microphone Settings".into(),
            font_size: 14.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        let mic = &self.settings.mic;
        y = Self::render_label_val(p, cmds, x, y, width, "Gain", &format!("{}%", mic.gain));
        y = Self::render_toggle_row(
            p,
            cmds,
            x,
            y,
            width,
            "Noise suppression",
            mic.noise_suppression,
        );
        y = Self::render_toggle_row(
            p,
            cmds,
            x,
            y,
            width,
            "Echo cancellation",
            mic.echo_cancellation,
        );
        y = Self::render_toggle_row(p, cmds, x, y, width, "Automatic gain", mic.auto_gain);
        y = Self::render_toggle_row(p, cmds, x, y, width, "Monitor (loopback)", mic.monitor);
        if mic.monitor {
            y = Self::render_label_val(
                p,
                cmds,
                x,
                y,
                width,
                "Monitor volume",
                &format!("{}%", mic.monitor_volume),
            );
        }

        y
    }

    fn render_app_volumes_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        if self.settings.app_volumes.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "No applications are currently producing audio.".into(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            return y + 24.0;
        }

        for entry in &self.settings.app_volumes {
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 48.0,
                color: p.mantle,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 6.0,
                text: entry.display_name.clone(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.5),
                overflow: TextOverflow::Ellipsis,
            });
            let muted_txt = if entry.muted { " (Muted)" } else { "" };
            cmds.push(RenderCommand::Text {
                x: x + width * 0.55,
                y: y + 6.0,
                text: format!("{}%{}", entry.volume, muted_txt),
                font_size: 13.0,
                color: if entry.muted { p.red } else { p.subtext0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.4),
                overflow: TextOverflow::Ellipsis,
            });
            Self::render_volume_bar(
                p,
                cmds,
                x + 12.0,
                y + 30.0,
                width - 24.0,
                entry.volume,
                entry.muted,
            );
            y += 56.0;
        }
        y
    }

    fn render_system_sounds_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        // Global toggle
        y = Self::render_toggle_row(
            p,
            cmds,
            x,
            y,
            width,
            "Enable system sounds",
            self.settings.system_sounds_enabled,
        );
        y += 4.0;

        for sc in &self.settings.system_sounds {
            let label = sc.event.label();
            let status = if sc.enabled { "On" } else { "Off" };
            let custom = sc.custom_sound.as_deref().unwrap_or("Default");
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 28.0,
                color: p.mantle,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 6.0,
                text: label.into(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.35),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + width * 0.4,
                y: y + 6.0,
                text: status.into(),
                font_size: 12.0,
                color: if sc.enabled { p.green } else { p.overlay0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.15),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + width * 0.6,
                y: y + 6.0,
                text: custom.into(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.35),
                overflow: TextOverflow::Ellipsis,
            });
            y += 32.0;
        }
        y
    }

    fn render_spatial_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Spatial Audio".into(),
            font_size: 14.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        for mode in SpatialAudioMode::ALL {
            let active = self.settings.spatial_mode == mode;
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 32.0,
                color: if active { p.surface0 } else { p.mantle },
                corner_radii: CornerRadii::all(6.0),
            });
            let indicator = if active { "● " } else { "○ " };
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 8.0,
                text: format!("{}{}", indicator, mode.label()),
                font_size: 13.0,
                color: if active { p.accent } else { p.text },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 36.0;
        }
        y
    }

    // ------------------------------------------------------------------
    // Shared rendering helpers
    // ------------------------------------------------------------------

    fn render_volume_bar(
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        volume: u32,
        muted: bool,
    ) -> f32 {
        let bar_h = 6.0_f32;
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: bar_h,
            color: p.surface1,
            corner_radii: CornerRadii::all(3.0),
        });
        let frac = volume as f32 / 100.0;
        let fill_color = if muted { p.red } else { p.accent };
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: width * frac,
            height: bar_h,
            color: fill_color,
            corner_radii: CornerRadii::all(3.0),
        });
        y + bar_h + 4.0
    }

    fn render_label_val(
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) -> f32 {
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y,
            text: label.into(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });
        y + 22.0
    }

    fn render_toggle_row(
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        on: bool,
    ) -> f32 {
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y,
            text: label.into(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.6),
            overflow: TextOverflow::Ellipsis,
        });
        let tx = x + width - 48.0;
        let bg = if on { p.green } else { p.surface1 };
        cmds.extend(crate::switch::switch(tx, y, 40.0, 20.0, on, bg));
        y + 26.0
    }

    /// Hit-test for tab selection. Returns tab index or None.
    pub fn hit_tab(&self, rel_x: f32, width: f32) -> Option<usize> {
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let tab_w = inner / Self::TAB_LABELS.len() as f32;
        let offset = rel_x - pad;
        if offset < 0.0 || offset >= inner {
            return None;
        }
        let idx = (offset / tab_w) as usize;
        if idx < Self::TAB_LABELS.len() {
            Some(idx)
        } else {
            None
        }
    }
}

impl Default for SoundSettingsUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use guitk::color::Color;

    #[test]
    fn audio_device_format_string() {
        let d = AudioDevice::new(1, "Test", DeviceKind::Output);
        assert!(d.format_string().contains("48000"));
        assert!(d.format_string().contains("Stereo"));
    }

    #[test]
    fn audio_device_format_surround() {
        let mut d = AudioDevice::new(1, "Test", DeviceKind::Output);
        d.channels = 6;
        assert!(d.format_string().contains("5.1"));
        d.channels = 8;
        assert!(d.format_string().contains("7.1"));
    }

    #[test]
    fn audio_device_format_mono() {
        let mut d = AudioDevice::new(1, "Test", DeviceKind::Input);
        d.channels = 1;
        assert!(d.format_string().contains("Mono"));
    }

    #[test]
    fn audio_device_format_unusual_channels() {
        let mut d = AudioDevice::new(1, "Test", DeviceKind::Output);
        d.channels = 4;
        assert!(d.format_string().contains("4 ch"));
    }

    #[test]
    fn device_volume_clamped() {
        let mut d = AudioDevice::new(1, "T", DeviceKind::Output);
        d.set_volume(200);
        assert_eq!(d.volume, 100);
    }

    #[test]
    fn sound_settings_defaults() {
        let s = SoundSettings::with_defaults();
        assert!(s.default_output().is_some());
        assert!(s.default_input().is_some());
        assert_eq!(s.master_volume, 80);
        assert!(!s.master_muted);
    }

    #[test]
    fn add_remove_device() {
        let mut s = SoundSettings::new();
        let id = s.add_device("Speakers", DeviceKind::Output);
        assert!(s.get_device(id).is_some());
        assert!(s.remove_device(id));
        assert!(s.get_device(id).is_none());
    }

    #[test]
    fn set_default_device() {
        let mut s = SoundSettings::new();
        let a = s.add_device("A", DeviceKind::Output);
        let b = s.add_device("B", DeviceKind::Output);
        s.set_default_device(a);
        assert!(s.get_device(a).unwrap().is_default);
        assert!(!s.get_device(b).unwrap().is_default);
        s.set_default_device(b);
        assert!(!s.get_device(a).unwrap().is_default);
        assert!(s.get_device(b).unwrap().is_default);
    }

    #[test]
    fn effective_volume() {
        let mut s = SoundSettings::new();
        let id = s.add_device("Out", DeviceKind::Output);
        s.set_device_volume(id, 50);
        s.set_master_volume(80);
        assert_eq!(s.effective_volume(id), 40); // 50 * 80 / 100
    }

    #[test]
    fn effective_volume_muted() {
        let mut s = SoundSettings::new();
        let id = s.add_device("Out", DeviceKind::Output);
        s.set_device_volume(id, 50);
        s.master_muted = true;
        assert_eq!(s.effective_volume(id), 0);
    }

    #[test]
    fn effective_volume_device_muted() {
        let mut s = SoundSettings::new();
        let id = s.add_device("Out", DeviceKind::Output);
        s.set_device_muted(id, true);
        assert_eq!(s.effective_volume(id), 0);
    }

    #[test]
    fn device_format() {
        let mut s = SoundSettings::new();
        let id = s.add_device("Out", DeviceKind::Output);
        s.set_device_format(id, 96000, 24, 6);
        let d = s.get_device(id).unwrap();
        assert_eq!(d.sample_rate, 96000);
        assert_eq!(d.bit_depth, 24);
        assert_eq!(d.channels, 6);
    }

    #[test]
    fn app_volume() {
        let mut s = SoundSettings::new();
        s.set_app_volume("music", "Music Player", 70);
        assert_eq!(s.app_volumes().len(), 1);
        assert_eq!(s.app_volumes()[0].volume, 70);
    }

    #[test]
    fn app_volume_update() {
        let mut s = SoundSettings::new();
        s.set_app_volume("music", "Music", 70);
        s.set_app_volume("music", "Music", 30);
        assert_eq!(s.app_volumes().len(), 1);
        assert_eq!(s.app_volumes()[0].volume, 30);
    }

    #[test]
    fn app_volume_muted() {
        let mut s = SoundSettings::new();
        s.set_app_volume("vid", "Video", 100);
        s.set_app_muted("vid", true);
        assert_eq!(s.effective_app_volume("vid"), 0);
    }

    #[test]
    fn app_volume_respects_master() {
        let mut s = SoundSettings::new();
        s.set_master_volume(50);
        s.set_app_volume("a", "A", 80);
        assert_eq!(s.effective_app_volume("a"), 40); // 80*50/100
    }

    #[test]
    fn remove_app_volume() {
        let mut s = SoundSettings::new();
        s.set_app_volume("a", "A", 50);
        assert!(s.remove_app_volume("a"));
        assert!(!s.remove_app_volume("a"));
        assert!(s.app_volumes().is_empty());
    }

    #[test]
    fn system_sound_enabled() {
        let s = SoundSettings::new();
        assert!(s.should_play_sound(SystemSoundEvent::Notification));
    }

    #[test]
    fn system_sound_disabled() {
        let mut s = SoundSettings::new();
        s.set_system_sound_enabled(SystemSoundEvent::Notification, false);
        assert!(!s.should_play_sound(SystemSoundEvent::Notification));
    }

    #[test]
    fn system_sound_globally_off() {
        let mut s = SoundSettings::new();
        s.system_sounds_enabled = false;
        assert!(!s.should_play_sound(SystemSoundEvent::Error));
    }

    #[test]
    fn system_sound_custom() {
        let mut s = SoundSettings::new();
        s.set_system_sound_custom(SystemSoundEvent::Error, Some("/sounds/boom.wav".into()));
        let sc = s.get_system_sound(SystemSoundEvent::Error).unwrap();
        assert_eq!(sc.custom_sound.as_deref(), Some("/sounds/boom.wav"));
    }

    #[test]
    fn system_sound_volume_override() {
        let mut s = SoundSettings::new();
        s.set_system_sound_volume(SystemSoundEvent::Warning, Some(200));
        let sc = s.get_system_sound(SystemSoundEvent::Warning).unwrap();
        assert_eq!(sc.volume_override, Some(100)); // clamped
    }

    #[test]
    fn spatial_audio_mode() {
        let mut s = SoundSettings::new();
        assert_eq!(s.spatial_mode, SpatialAudioMode::Off);
        s.set_spatial_mode(SpatialAudioMode::HeadphoneHrtf);
        assert_eq!(s.spatial_mode, SpatialAudioMode::HeadphoneHrtf);
    }

    #[test]
    fn spatial_mode_labels() {
        for m in SpatialAudioMode::ALL {
            assert!(!m.label().is_empty());
        }
    }

    #[test]
    fn mic_config_defaults() {
        let m = MicConfig::default();
        assert_eq!(m.gain, 80);
        assert!(m.noise_suppression);
        assert!(m.echo_cancellation);
        assert!(m.auto_gain);
        assert!(!m.monitor);
    }

    #[test]
    fn mic_gain_clamped() {
        let mut m = MicConfig::default();
        m.set_gain(200);
        assert_eq!(m.gain, 100);
    }

    #[test]
    fn mic_monitor_volume_clamped() {
        let mut m = MicConfig::default();
        m.set_monitor_volume(200);
        assert_eq!(m.monitor_volume, 100);
    }

    #[test]
    fn master_volume_clamped() {
        let mut s = SoundSettings::new();
        s.set_master_volume(200);
        assert_eq!(s.master_volume, 100);
    }

    #[test]
    fn output_and_input_device_lists() {
        let s = SoundSettings::with_defaults();
        assert_eq!(s.output_devices().len(), 2); // Speakers + HDMI
        assert_eq!(s.input_devices().len(), 1); // Built-in Microphone
    }

    #[test]
    fn system_sounds_count() {
        let s = SoundSettings::new();
        assert_eq!(s.system_sounds_list().len(), 12);
    }

    #[test]
    fn system_sound_event_labels() {
        for e in SystemSoundEvent::ALL {
            assert!(!e.label().is_empty());
        }
    }

    #[test]
    fn ui_render_produces_commands() {
        let ui = SoundSettingsUI::new();
        let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_set_tab() {
        let mut ui = SoundSettingsUI::new();
        assert_eq!(ui.active_tab(), 0);
        ui.set_active_tab(3);
        assert_eq!(ui.active_tab(), 3);
        ui.set_active_tab(99);
        assert_eq!(ui.active_tab(), 3); // out of range ignored
    }

    #[test]
    fn ui_render_each_tab() {
        let mut ui = SoundSettingsUI::new();
        ui.settings_mut().set_app_volume("test", "Test App", 50);
        for i in 0..5 {
            ui.set_active_tab(i);
            let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 500.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_hit_tab() {
        let ui = SoundSettingsUI::new();
        // Tabs start at x=16, each (500-32)/5 = 93.6 wide.
        assert!(ui.hit_tab(10.0, 500.0).is_none()); // before tabs
        let hit = ui.hit_tab(20.0, 500.0);
        assert_eq!(hit, Some(0));
    }

    #[test]
    fn ui_hit_tab_last() {
        let ui = SoundSettingsUI::new();
        let inner = 500.0 - 32.0;
        // Last tab starts at pad + 4*(inner/5).
        let last_start = 16.0 + 4.0 * (inner / 5.0);
        let hit = ui.hit_tab(last_start + 5.0, 500.0);
        assert_eq!(hit, Some(4));
    }

    #[test]
    fn app_volume_entry_set_volume() {
        let mut e = AppVolumeEntry::new("app", "App");
        e.set_volume(200);
        assert_eq!(e.volume, 100);
    }

    #[test]
    fn app_device_routing() {
        let mut s = SoundSettings::new();
        let id = s.add_device("Headphones", DeviceKind::Output);
        s.set_app_volume("game", "Game", 100);
        s.set_app_device("game", Some(id));
        assert_eq!(s.app_volumes()[0].output_device_id, Some(id));
    }

    #[test]
    fn ui_with_settings() {
        let mut settings = SoundSettings::new();
        settings.set_master_volume(42);
        let ui = SoundSettingsUI::with_settings(settings);
        assert_eq!(ui.settings().master_volume, 42);
    }

    #[test]
    fn monitor_loopback_renders_volume() {
        let mut ui = SoundSettingsUI::new();
        ui.settings_mut().mic.monitor = true;
        ui.set_active_tab(1);
        let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 500.0);
        let has_mon = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Monitor volume")),
        );
        assert!(has_mon);
    }

    // ------------------------------------------------------------------
    // Palette conversion
    //
    // Part 2 of TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-
    // PALETTE. The eleven `const … : Color` this module used to hold were
    // Catppuccin Mocha values; the sweep below renders in *light* mode, where
    // a surviving one is a colour the palette does not contain and names
    // itself. What the sweep cannot see is a colour put in the *wrong* role —
    // a role is a member of both palettes — so every colour that must not
    // follow the mode or the accent gets its own test underneath.
    // ------------------------------------------------------------------

    /// Accents that are none of the colours this panel freezes.
    ///
    /// `red` is mute, `green` is "on" and `lavender` is a section heading; an
    /// accent equal to any of them would make "this did not follow the accent"
    /// true by coincidence, in the one run where a real failure would be
    /// hardest to notice. `blue` is excluded too, because blue is what the
    /// three accent sites were *before* the conversion.
    const SAFE_ACCENTS: [Color; 4] = [
        appearance::MAUVE,
        appearance::TEAL,
        appearance::SAPPHIRE,
        appearance::PINK,
    ];

    /// The panel is always rendered at this origin and width, so every width
    /// and height a fixture asserts on is a constant rather than a guess.
    const W: f32 = 500.0;
    /// `W - 2 * pad`, the content width every tab is given.
    const INNER: f32 = 468.0;

    fn rgb(c: Color) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

    /// A panel with every two-way branch in the renderer taken both ways.
    ///
    /// Two output devices (one default, one not), two input devices (likewise),
    /// two app-volume rows (one muted, one not), one disabled system sound
    /// among eleven enabled, one custom sound among eleven defaults, mic
    /// toggles both on and off, monitor on so its extra row is drawn, and a
    /// spatial mode selected so the radio group has both an active and an
    /// inactive member. Volumes and formats are all distinct so that no two
    /// texts collide and hide each other.
    fn full_ui(tab: usize) -> SoundSettingsUI {
        let mut s = SoundSettings::new();
        s.set_master_volume(60);

        let speakers = s.add_device("Speakers", DeviceKind::Output);
        s.set_default_device(speakers);
        s.set_device_volume(speakers, 70);
        let hdmi = s.add_device("HDMI Audio", DeviceKind::Output);
        s.set_device_volume(hdmi, 40);
        s.set_device_format(hdmi, 48000, 16, 6);

        let builtin = s.add_device("Built-in Microphone", DeviceKind::Input);
        s.set_default_device(builtin);
        s.set_device_format(builtin, 48000, 16, 1);
        let usb = s.add_device("USB Headset Mic", DeviceKind::Input);
        s.set_device_format(usb, 44100, 16, 2);

        s.set_app_volume("player", "Media Player", 55);
        s.set_app_volume("game", "Game", 30);
        s.set_app_muted("game", true);

        s.set_system_sound_enabled(SystemSoundEvent::Error, false);
        s.set_system_sound_custom(SystemSoundEvent::Notification, Some("chime.wav".into()));

        s.mic.gain = 45;
        s.mic.noise_suppression = true;
        s.mic.echo_cancellation = false;
        s.mic.auto_gain = true;
        s.mic.monitor = true;
        s.mic.monitor_volume = 25;

        s.spatial_mode = SpatialAudioMode::HeadphoneHrtf;

        let mut ui = SoundSettingsUI::with_settings(s);
        ui.set_active_tab(tab);
        ui
    }

    /// [`full_ui`] with the master muted — the other arm of the two sites that
    /// change when it is.
    fn muted_ui(tab: usize) -> SoundSettingsUI {
        let mut ui = full_ui(tab);
        ui.settings_mut().master_muted = true;
        ui
    }

    /// No devices and no app audio: the three "nothing here" arms, which no
    /// populated fixture can reach.
    fn empty_ui(tab: usize) -> SoundSettingsUI {
        let mut ui = SoundSettingsUI::with_settings(SoundSettings::new());
        ui.set_active_tab(tab);
        ui
    }

    fn draw(ui: &SoundSettingsUI, p: &Palette) -> Vec<RenderCommand> {
        ui.render(p, 0.0, 0.0, W)
    }

    /// Every command all five tabs draw, for the sweep — which is only ever as
    /// wide as the render it is handed.
    fn draw_all_tabs(make: impl Fn(usize) -> SoundSettingsUI, p: &Palette) -> Vec<RenderCommand> {
        let mut all = Vec::new();
        for tab in 0..5 {
            all.extend(draw(&make(tab), p));
        }
        all
    }

    /// Colours of every `Text` saying exactly `want` at exactly `size`.
    ///
    /// The font size is part of the key because this panel says "Spatial
    /// Audio" twice — once as a 12px tab label and once as a 14px section
    /// heading — in two different roles.
    fn texts_saying(cmds: &[RenderCommand], want: &str, size: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    font_size,
                    color,
                    ..
                } if text == want && (*font_size - size).abs() < f32::EPSILON => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Colours of every `FillRect` of exactly this size, in draw order.
    fn fills(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if (*width - w).abs() < 0.01 && (*height - h).abs() < 0.01 => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Colours of every volume-bar rectangle, in draw order: track, fill,
    /// track, fill, … Keyed on the 6px bar height, which nothing else uses.
    fn bars(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { height, color, .. }
                    if (*height - 6.0).abs() < f32::EPSILON =>
                {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_colour_the_sound_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                for (what, make) in [
                    ("populated", full_ui as fn(usize) -> SoundSettingsUI),
                    ("muted", muted_ui as fn(usize) -> SoundSettingsUI),
                    ("empty", empty_ui as fn(usize) -> SoundSettingsUI),
                ] {
                    let cmds = draw_all_tabs(make, &p);
                    // A switch knob is `readable_on` its own track — one of the
                    // two extremes, not a role. The tracks are named rather
                    // than the extremes, so the exemption stays tied to the
                    // fill it sits on.
                    assert_drawn_from(
                        &p,
                        &cmds,
                        &[
                            appearance::readable_on(p.green),
                            appearance::readable_on(p.surface1),
                        ],
                        &format!("sound settings ({what})"),
                    );
                }
            }
        }
    }

    /// The sweep is only as wide as the render it is given, so this holds the
    /// fixtures to actually reaching every arm. Each assertion names the `if`
    /// or `match` in the renderer it stands for; a branch that stops being
    /// drawn stops being checked by everything above, silently, unless this
    /// fails first.
    #[test]
    fn the_fixtures_take_every_branch_the_sound_panel_has() {
        let p = Palette::for_mode(false);

        // render(): master muted / not muted.
        assert_eq!(
            texts_saying(&draw(&full_ui(0), &p), "Master Volume: 60%", 14.0).len(),
            1,
            "the unmuted master label is not drawn"
        );
        assert_eq!(
            texts_saying(&draw(&muted_ui(0), &p), "Master Volume: 60% (Muted)", 14.0).len(),
            1,
            "the muted master label is not drawn"
        );

        // render(): active / inactive tab, and all five match arms.
        for tab in 0..5 {
            let cmds = draw(&full_ui(tab), &p);
            let tabs = fills(&cmds, INNER / 5.0 - 2.0, 32.0);
            assert_eq!(tabs.len(), 5, "the tab bar is not five tabs wide");
            assert_eq!(
                tabs.iter().filter(|c| **c == p.surface0).count(),
                1,
                "tab {tab} is not the only active one"
            );
        }

        // render_output_tab / render_input_tab / render_app_volumes_tab: the
        // empty arm and the populated one.
        for (tab, msg) in [
            (0, "No output devices detected."),
            (1, "No input devices detected."),
            (2, "No applications are currently producing audio."),
        ] {
            assert_eq!(
                texts_saying(&draw(&empty_ui(tab), &p), msg, 13.0).len(),
                1,
                "tab {tab} does not say it is empty"
            );
            assert!(
                texts_saying(&draw(&full_ui(tab), &p), msg, 13.0).is_empty(),
                "tab {tab} claims to be empty when it is not"
            );
        }

        // render_output_tab / render_input_tab: default and non-default rows.
        assert_eq!(
            fills(&draw(&full_ui(0), &p), INNER, 64.0),
            vec![p.surface0, p.mantle],
            "the output tab does not draw one default row and one other"
        );
        assert_eq!(
            fills(&draw(&full_ui(1), &p), INNER, 48.0),
            vec![p.surface0, p.mantle],
            "the input tab does not draw one default row and one other"
        );

        // render_input_tab: mic.monitor on, so the extra row exists.
        assert_eq!(
            texts_saying(&draw(&full_ui(1), &p), "Monitor volume", 13.0).len(),
            1,
            "the monitor-volume row is not drawn"
        );

        // render_toggle_row: on and off.
        let mic = draw(&full_ui(1), &p);
        let pills = fills(&mic, 40.0, 20.0);
        assert!(
            pills.contains(&p.green) && pills.contains(&p.surface1),
            "the mic tab does not show a toggle both on and off"
        );

        // render_app_volumes_tab: a muted row and an unmuted one.
        let apps = draw(&full_ui(2), &p);
        assert_eq!(texts_saying(&apps, "55%", 13.0).len(), 1);
        assert_eq!(texts_saying(&apps, "30% (Muted)", 13.0).len(), 1);

        // render_system_sounds_tab: an enabled sound and a disabled one, and a
        // custom sound among the defaults.
        let sounds = draw(&full_ui(3), &p);
        assert_eq!(texts_saying(&sounds, "On", 12.0).len(), 11);
        assert_eq!(texts_saying(&sounds, "Off", 12.0).len(), 1);
        assert_eq!(texts_saying(&sounds, "chime.wav", 12.0).len(), 1);
        assert_eq!(texts_saying(&sounds, "Default", 12.0).len(), 11);

        // render_spatial_tab: the selected mode and the two others.
        let spatial = draw(&full_ui(4), &p);
        assert_eq!(
            fills(&spatial, INNER, 32.0),
            vec![p.mantle, p.surface0, p.mantle],
            "the spatial tab does not draw exactly one selected mode"
        );

        // render_volume_bar: a muted bar and an unmuted one.
        let muted_bars = bars(&draw(&muted_ui(0), &p));
        assert!(
            muted_bars.contains(&p.red),
            "no muted volume bar is ever drawn"
        );
        assert!(
            bars(&draw(&full_ui(0), &p)).contains(&p.accent),
            "no unmuted volume bar is ever drawn"
        );
    }

    /// Every text this panel draws, one assertion per *source* site.
    ///
    /// This table has one entry per `color:` expression in the source, not one
    /// per kind of text, and must not be shortened to a representative sample:
    /// a table that lists one entry per kind leaves every unlisted site
    /// checked by nothing, which is exactly how two defects escaped the
    /// proof run for `widgets.rs`. n source sites, n assertions.
    #[test]
    fn every_text_the_sound_panel_draws_is_in_the_role_it_claims() {
        for light in [false, true] {
            let p = Palette::for_mode(light);

            // render(): title, and the unmuted master label.
            let head = draw(&full_ui(0), &p);
            for (glyph, size, role, what) in [
                ("Sound Settings", 20.0, p.text, "the title"),
                (
                    "Master Volume: 60%",
                    14.0,
                    p.text,
                    "an unmuted master label",
                ),
                ("Output", 12.0, p.accent, "the active tab's label"),
                ("Input", 12.0, p.subtext0, "an inactive tab's label"),
            ] {
                let t = texts_saying(&head, glyph, size);
                assert_eq!(t.len(), 1, "{what} is not drawn once (light={light})");
                assert_eq!(rgb(t[0]), rgb(role), "{what} is in the wrong role");
            }

            // render(): the muted master label.
            let m = texts_saying(&draw(&muted_ui(0), &p), "Master Volume: 60% (Muted)", 14.0);
            assert_eq!(m.len(), 1);
            assert_eq!(rgb(m[0]), rgb(p.red), "a muted master label is not red");

            // render_output_tab.
            let out = draw(&full_ui(0), &p);
            for (glyph, size, role, what) in [
                (
                    "Speakers ✓ Default",
                    14.0,
                    p.text,
                    "a default output's name",
                ),
                ("HDMI Audio", 14.0, p.text, "a non-default output's name"),
                (
                    "48000 Hz / 16-bit / Stereo",
                    11.0,
                    p.subtext0,
                    "an output's format line",
                ),
                (
                    "48000 Hz / 16-bit / 5.1 Surround",
                    11.0,
                    p.subtext0,
                    "the other output's format line",
                ),
            ] {
                let t = texts_saying(&out, glyph, size);
                assert_eq!(t.len(), 1, "{what} is not drawn once (light={light})");
                assert_eq!(rgb(t[0]), rgb(role), "{what} is in the wrong role");
            }

            // render_input_tab, render_label_val and render_toggle_row.
            let inp = draw(&full_ui(1), &p);
            for (glyph, size, role, what) in [
                (
                    "Built-in Microphone ✓ Default",
                    14.0,
                    p.text,
                    "a default input's name",
                ),
                (
                    "USB Headset Mic",
                    14.0,
                    p.text,
                    "a non-default input's name",
                ),
                (
                    "48000 Hz / 16-bit / Mono",
                    11.0,
                    p.subtext0,
                    "an input's format line",
                ),
                (
                    "44100 Hz / 16-bit / Stereo",
                    11.0,
                    p.subtext0,
                    "the other input's format line",
                ),
                (
                    "Microphone Settings",
                    14.0,
                    p.lavender,
                    "the mic section heading",
                ),
                ("Gain", 13.0, p.subtext0, "a label-value row's label"),
                ("45%", 13.0, p.text, "a label-value row's value"),
                (
                    "Monitor volume",
                    13.0,
                    p.subtext0,
                    "the monitor row's label",
                ),
                ("25%", 13.0, p.text, "the monitor row's value"),
                (
                    "Noise suppression",
                    13.0,
                    p.subtext0,
                    "a toggle row's label",
                ),
                (
                    "Echo cancellation",
                    13.0,
                    p.subtext0,
                    "an off toggle row's label",
                ),
            ] {
                let t = texts_saying(&inp, glyph, size);
                assert_eq!(t.len(), 1, "{what} is not drawn once (light={light})");
                assert_eq!(rgb(t[0]), rgb(role), "{what} is in the wrong role");
            }

            // render_app_volumes_tab.
            let apps = draw(&full_ui(2), &p);
            for (glyph, size, role, what) in [
                ("Media Player", 13.0, p.text, "an app's name"),
                ("55%", 13.0, p.subtext0, "an unmuted app's volume"),
                ("Game", 13.0, p.text, "the muted app's name"),
                ("30% (Muted)", 13.0, p.red, "a muted app's volume"),
            ] {
                let t = texts_saying(&apps, glyph, size);
                assert_eq!(t.len(), 1, "{what} is not drawn once (light={light})");
                assert_eq!(rgb(t[0]), rgb(role), "{what} is in the wrong role");
            }

            // render_system_sounds_tab.
            let sounds = draw(&full_ui(3), &p);
            for (glyph, size, role, want, what) in [
                (
                    "Enable system sounds",
                    13.0,
                    p.subtext0,
                    1,
                    "the global toggle's label",
                ),
                ("Notification", 12.0, p.text, 1, "a system sound's label"),
                ("On", 12.0, p.green, 11, "an enabled sound's status"),
                ("Off", 12.0, p.overlay0, 1, "a disabled sound's status"),
                ("chime.wav", 12.0, p.subtext0, 1, "a custom sound's name"),
                ("Default", 12.0, p.subtext0, 11, "an unset sound's name"),
            ] {
                let t = texts_saying(&sounds, glyph, size);
                assert_eq!(t.len(), want, "{what} is not drawn {want}× (light={light})");
                for c in t {
                    assert_eq!(rgb(c), rgb(role), "{what} is in the wrong role");
                }
            }

            // render_spatial_tab.
            let spatial = draw(&full_ui(4), &p);
            for (glyph, size, role, what) in [
                (
                    "Spatial Audio",
                    14.0,
                    p.lavender,
                    "the spatial section heading",
                ),
                (
                    "● Headphone HRTF",
                    13.0,
                    p.accent,
                    "the selected mode's label",
                ),
                ("○ Off", 13.0, p.text, "an unselected mode's label"),
                (
                    "○ Virtual Surround",
                    13.0,
                    p.text,
                    "the other unselected mode's label",
                ),
            ] {
                let t = texts_saying(&spatial, glyph, size);
                assert_eq!(t.len(), 1, "{what} is not drawn once (light={light})");
                assert_eq!(rgb(t[0]), rgb(role), "{what} is in the wrong role");
            }

            // The three "nothing here" lines, which only the empty fixture
            // reaches.
            for (tab, msg) in [
                (0, "No output devices detected."),
                (1, "No input devices detected."),
                (2, "No applications are currently producing audio."),
            ] {
                let t = texts_saying(&draw(&empty_ui(tab), &p), msg, 13.0);
                assert_eq!(t.len(), 1, "tab {tab} does not say it is empty");
                assert_eq!(
                    rgb(t[0]),
                    rgb(p.overlay0),
                    "an empty-list line is not in the faintest role (light={light})"
                );
            }
        }
    }

    /// Every rectangle this panel draws, again one assertion per source site.
    #[test]
    fn every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let head = draw(&full_ui(0), &p);

            assert_eq!(
                fills(&head, W, 800.0),
                vec![p.base],
                "the panel background is not `base` (light={light})"
            );
            assert_eq!(
                fills(&head, INNER / 5.0 - 2.0, 32.0),
                vec![p.surface0, p.mantle, p.mantle, p.mantle, p.mantle],
                "the tab bar is not one raised tab among four recessed"
            );
            assert_eq!(
                fills(&head, INNER, 64.0),
                vec![p.surface0, p.mantle],
                "an output row is in the wrong role"
            );
            assert_eq!(
                fills(&draw(&full_ui(1), &p), INNER, 48.0),
                vec![p.surface0, p.mantle],
                "an input row is in the wrong role"
            );
            assert_eq!(
                fills(&draw(&full_ui(2), &p), INNER, 48.0),
                vec![p.mantle, p.mantle],
                "an app row is in the wrong role"
            );
            assert_eq!(
                fills(&draw(&full_ui(3), &p), INNER, 28.0),
                vec![p.mantle; 12],
                "a system-sound row is in the wrong role"
            );
            assert_eq!(
                fills(&draw(&full_ui(4), &p), INNER, 32.0),
                vec![p.mantle, p.surface0, p.mantle],
                "a spatial-mode row is in the wrong role"
            );

            // render_toggle_row: pill on, pill off, and the knob — which is
            // `readable_on` whichever pill it sits on, so the on and off knobs
            // are deliberately *different* inks. See
            // `switch::the_knob_is_legible_on_every_track_a_panel_can_choose`.
            let inp = draw(&full_ui(1), &p);
            let pills = vec![p.green, p.surface1, p.green, p.green];
            assert_eq!(
                fills(&inp, 40.0, 20.0),
                pills,
                "the mic toggles' pills are in the wrong roles (light={light})"
            );
            assert_eq!(
                fills(&inp, 16.0, 16.0),
                pills
                    .iter()
                    .map(|c| appearance::readable_on(*c))
                    .collect::<Vec<_>>(),
                "a toggle knob is not derived from its own pill (light={light})"
            );

            // render_volume_bar: the track is a surface in every state.
            for (what, cmds) in [
                ("unmuted", draw(&full_ui(0), &p)),
                ("muted", draw(&muted_ui(0), &p)),
            ] {
                let b = bars(&cmds);
                assert_eq!(b.len(), 6, "the output tab does not draw three bars");
                for (i, track) in b.iter().step_by(2).enumerate() {
                    assert_eq!(
                        *track, p.surface1,
                        "the {what} bar {i}'s track is not `surface1` (light={light})"
                    );
                }
            }
        }
    }

    /// Three sites follow the accent, and all three say "you are here" or
    /// "drag me": the active tab's label, the selected spatial mode's label,
    /// and the filled part of a volume bar.
    ///
    /// Checked as equality with `p.accent` rather than inequality with the blue
    /// they used to be — a site frozen to some *other* literal would pass an
    /// inequality test and still fail the user.
    #[test]
    fn the_three_accent_sites_follow_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                let head = draw(&full_ui(0), &p);
                let tab = texts_saying(&head, "Output", 12.0);
                assert_eq!(tab.len(), 1);
                assert_eq!(
                    rgb(tab[0]),
                    rgb(accent),
                    "the active tab's label does not follow the accent (light={light})"
                );

                // Three bars on the output tab: master, Speakers, HDMI. Every
                // fill is the accent, so this is per-site rather than a sample.
                let b = bars(&head);
                assert_eq!(b.len(), 6);
                for (i, fill) in b.iter().skip(1).step_by(2).enumerate() {
                    assert_eq!(
                        rgb(*fill),
                        rgb(accent),
                        "volume bar {i}'s fill does not follow the accent (light={light})"
                    );
                }

                let mode = texts_saying(&draw(&full_ui(4), &p), "● Headphone HRTF", 13.0);
                assert_eq!(mode.len(), 1);
                assert_eq!(
                    rgb(mode[0]),
                    rgb(accent),
                    "the selected spatial mode does not follow the accent (light={light})"
                );
            }
        }
    }

    /// Mute is the one state that overrides the slider rule.
    ///
    /// A filled bar normally says "this is how far you dragged it", which is
    /// the accent's job. A muted one says "no sound is coming out of this",
    /// which is a reading — and a reading is never the accent's, or a
    /// red-accented desktop would draw every unmuted bar in the mute colour.
    #[test]
    fn a_muted_volume_bar_never_looks_like_an_unmuted_one() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                let muted = bars(&draw(&muted_ui(0), &p));
                assert_eq!(muted.len(), 6);
                // Only the master bar is muted here; the two device bars are
                // not, which is why this checks index 1 and not all three.
                assert_eq!(
                    rgb(muted[1]),
                    rgb(p.red),
                    "the master bar does not go red when muted (light={light})"
                );
                assert_ne!(
                    rgb(muted[1]),
                    rgb(accent),
                    "the mute colour followed the accent (light={light})"
                );

                // The muted app row's bar, which is a different call site.
                let apps = bars(&draw(&full_ui(2), &p));
                assert_eq!(apps.len(), 6);
                assert_eq!(
                    rgb(apps[3]),
                    rgb(p.accent),
                    "the unmuted app's bar does not follow the accent (light={light})"
                );
                assert_eq!(
                    rgb(apps[5]),
                    rgb(p.red),
                    "the muted app's bar is not red (light={light})"
                );
            }
        }
    }

    /// Everything this panel says about a *state* or a *category* is frozen.
    ///
    /// On/off is green-against-neutral in every accent because green means
    /// enabled here rather than decorating it, and a section heading names a
    /// category, which the accent never marks. One assertion per source site:
    /// the two status arms, the two pill arms, and the two headings.
    #[test]
    fn nothing_that_reports_a_state_follows_the_accent() {
        for light in [false, true] {
            let plain = Palette::for_mode(light);
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                let sounds = draw(&full_ui(3), &p);
                for c in texts_saying(&sounds, "On", 12.0) {
                    assert_eq!(
                        rgb(c),
                        rgb(plain.green),
                        "an enabled sound's status moved with the accent (light={light})"
                    );
                }
                for c in texts_saying(&sounds, "Off", 12.0) {
                    assert_eq!(
                        rgb(c),
                        rgb(plain.overlay0),
                        "a disabled sound's status moved with the accent (light={light})"
                    );
                }

                let inp = draw(&full_ui(1), &p);
                let pills = fills(&inp, 40.0, 20.0);
                assert_eq!(pills.len(), 4);
                for (i, pill) in pills.iter().enumerate() {
                    let want = if i == 1 { plain.surface1 } else { plain.green };
                    assert_eq!(
                        rgb(*pill),
                        rgb(want),
                        "toggle pill {i} moved with the accent (light={light})"
                    );
                }

                for (cmds, glyph, what) in [
                    (&inp, "Microphone Settings", "the mic heading"),
                    (
                        &draw(&full_ui(4), &p),
                        "Spatial Audio",
                        "the spatial heading",
                    ),
                ] {
                    let t = texts_saying(cmds, glyph, 14.0);
                    assert_eq!(t.len(), 1);
                    assert_eq!(
                        rgb(t[0]),
                        rgb(plain.lavender),
                        "{what} moved with the accent (light={light})"
                    );
                }
            }
        }
    }

    /// The three pairs this panel draws to tell two things apart must stay
    /// apart in both modes: the default device row against the others, the
    /// active tab against the inactive ones, and a bar's track against its
    /// fill. A conversion that collapsed either half of a pair onto the same
    /// role would still pass the sweep — both halves are roles.
    #[test]
    fn every_pair_this_panel_uses_to_tell_things_apart_stays_apart() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                assert_ne!(
                    rgb(p.surface0),
                    rgb(p.mantle),
                    "a default row is indistinguishable from the others (light={light})"
                );

                let head = draw(&full_ui(0), &p);
                let tabs = texts_saying(&head, "Output", 12.0);
                let other = texts_saying(&head, "Input", 12.0);
                assert_ne!(
                    rgb(tabs[0]),
                    rgb(other[0]),
                    "the active tab reads like an inactive one (light={light})"
                );

                let b = bars(&head);
                for i in (0..b.len()).step_by(2) {
                    assert_ne!(
                        rgb(b[i]),
                        rgb(b[i + 1]),
                        "bar {i}'s track reads like its fill (light={light})"
                    );
                }

                let sounds = draw(&full_ui(3), &p);
                assert_ne!(
                    rgb(texts_saying(&sounds, "On", 12.0)[0]),
                    rgb(texts_saying(&sounds, "Off", 12.0)[0]),
                    "an enabled sound reads like a disabled one (light={light})"
                );
            }
        }
    }
}
