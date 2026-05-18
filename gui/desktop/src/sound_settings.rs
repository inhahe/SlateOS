//! Sound settings panel.
//!
//! Configures audio output/input devices, master volume, per-app volume,
//! system sounds, spatial audio, and microphone settings. Renders as a
//! sub-page of the desktop's Settings application.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Audio device
// ============================================================================

/// Kind of audio device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Output,
    Input,
}

/// Audio endpoint device (speaker, headphones, microphone, etc.).
#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub id: u32,
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
    pub fn new(id: u32, name: &str, kind: DeviceKind) -> Self {
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
            n => return format!("{} Hz / {}-bit / {} ch", self.sample_rate, self.bit_depth, n),
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
    pub output_device_id: Option<u32>,
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
    /// Next device ID.
    next_id: u32,
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
            next_id: 1,
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

    pub fn add_device(&mut self, name: &str, kind: DeviceKind) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.devices.push(AudioDevice::new(id, name, kind));
        id
    }

    pub fn remove_device(&mut self, id: u32) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != id);
        self.devices.len() < before
    }

    pub fn get_device(&self, id: u32) -> Option<&AudioDevice> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn get_device_mut(&mut self, id: u32) -> Option<&mut AudioDevice> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    pub fn output_devices(&self) -> Vec<&AudioDevice> {
        self.devices.iter().filter(|d| d.kind == DeviceKind::Output).collect()
    }

    pub fn input_devices(&self) -> Vec<&AudioDevice> {
        self.devices.iter().filter(|d| d.kind == DeviceKind::Input).collect()
    }

    pub fn set_default_device(&mut self, id: u32) {
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
        self.devices.iter().find(|d| d.kind == DeviceKind::Output && d.is_default)
    }

    pub fn default_input(&self) -> Option<&AudioDevice> {
        self.devices.iter().find(|d| d.kind == DeviceKind::Input && d.is_default)
    }

    pub fn set_device_volume(&mut self, id: u32, vol: u32) {
        if let Some(d) = self.get_device_mut(id) {
            d.set_volume(vol);
        }
    }

    pub fn set_device_muted(&mut self, id: u32, muted: bool) {
        if let Some(d) = self.get_device_mut(id) {
            d.muted = muted;
        }
    }

    pub fn set_device_format(&mut self, id: u32, sample_rate: u32, bit_depth: u32, channels: u32) {
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
    pub fn effective_volume(&self, device_id: u32) -> u32 {
        if self.master_muted {
            return 0;
        }
        let dev_vol = self.get_device(device_id).map_or(0, |d| {
            if d.muted { 0 } else { d.volume }
        });
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

    pub fn set_app_device(&mut self, app_id: &str, device_id: Option<u32>) {
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

    pub fn set_system_sound_volume(
        &mut self,
        event: SystemSoundEvent,
        vol: Option<u32>,
    ) {
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
            .map_or(false, |s| s.enabled)
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
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
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
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "Sound Settings".into(),
            font_size: 20.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
        });
        cy += 32.0;

        // Master volume bar
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: format!(
                "Master Volume: {}%{}",
                self.settings.master_volume,
                if self.settings.master_muted { " (Muted)" } else { "" }
            ),
            font_size: 14.0,
            color: if self.settings.master_muted { RED } else { TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner),
        });
        cy += 22.0;
        cy = Self::render_volume_bar(&mut cmds, x + pad, cy, inner, self.settings.master_volume, self.settings.master_muted);
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
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: cy + 8.0,
                text: (*label).into(),
                font_size: 12.0,
                color: if active { BLUE } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_w - 16.0),
            });
        }
        cy += 40.0;

        // Tab content
        match self.active_tab {
            0 => cy = self.render_output_tab(&mut cmds, x + pad, cy, inner),
            1 => cy = self.render_input_tab(&mut cmds, x + pad, cy, inner),
            2 => cy = self.render_app_volumes_tab(&mut cmds, x + pad, cy, inner),
            3 => cy = self.render_system_sounds_tab(&mut cmds, x + pad, cy, inner),
            4 => cy = self.render_spatial_tab(&mut cmds, x + pad, cy, inner),
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
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return y + 24.0;
        }

        for dev in &devices {
            let bg = if dev.is_default { SURFACE0 } else { MANTLE };
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
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 28.0,
                text: dev.format_string(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
            });

            // Volume bar
            Self::render_volume_bar(cmds, x + 12.0, y + 46.0, width - 24.0, dev.volume, dev.muted);

            y += 72.0;
        }
        y
    }

    fn render_input_tab(
        &self,
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
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return y + 24.0;
        }

        for dev in &devices {
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 48.0,
                color: if dev.is_default { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });
            let def_txt = if dev.is_default { " ✓ Default" } else { "" };
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 8.0,
                text: format!("{}{}", dev.name, def_txt),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 28.0,
                text: dev.format_string(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
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
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        let mic = &self.settings.mic;
        y = Self::render_label_val(cmds, x, y, width, "Gain", &format!("{}%", mic.gain));
        y = Self::render_toggle_row(cmds, x, y, width, "Noise suppression", mic.noise_suppression);
        y = Self::render_toggle_row(cmds, x, y, width, "Echo cancellation", mic.echo_cancellation);
        y = Self::render_toggle_row(cmds, x, y, width, "Automatic gain", mic.auto_gain);
        y = Self::render_toggle_row(cmds, x, y, width, "Monitor (loopback)", mic.monitor);
        if mic.monitor {
            y = Self::render_label_val(cmds, x, y, width, "Monitor volume", &format!("{}%", mic.monitor_volume));
        }

        y
    }

    fn render_app_volumes_tab(
        &self,
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
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return y + 24.0;
        }

        for entry in &self.settings.app_volumes {
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 48.0,
                color: MANTLE,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 6.0,
                text: entry.display_name.clone(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.5),
            });
            let muted_txt = if entry.muted { " (Muted)" } else { "" };
            cmds.push(RenderCommand::Text {
                x: x + width * 0.55,
                y: y + 6.0,
                text: format!("{}%{}", entry.volume, muted_txt),
                font_size: 13.0,
                color: if entry.muted { RED } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.4),
            });
            Self::render_volume_bar(cmds, x + 12.0, y + 30.0, width - 24.0, entry.volume, entry.muted);
            y += 56.0;
        }
        y
    }

    fn render_system_sounds_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        // Global toggle
        y = Self::render_toggle_row(
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
                color: MANTLE,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 6.0,
                text: label.into(),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.35),
            });
            cmds.push(RenderCommand::Text {
                x: x + width * 0.4,
                y: y + 6.0,
                text: status.into(),
                font_size: 12.0,
                color: if sc.enabled { GREEN } else { OVERLAY0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.15),
            });
            cmds.push(RenderCommand::Text {
                x: x + width * 0.6,
                y: y + 6.0,
                text: custom.into(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.35),
            });
            y += 32.0;
        }
        y
    }

    fn render_spatial_tab(
        &self,
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
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        for mode in SpatialAudioMode::ALL {
            let active = self.settings.spatial_mode == mode;
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 32.0,
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });
            let indicator = if active { "● " } else { "○ " };
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 8.0,
                text: format!("{}{}", indicator, mode.label()),
                font_size: 13.0,
                color: if active { BLUE } else { TEXT },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(width - 24.0),
            });
            y += 36.0;
        }
        y
    }

    // ------------------------------------------------------------------
    // Shared rendering helpers
    // ------------------------------------------------------------------

    fn render_volume_bar(
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
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        let frac = volume as f32 / 100.0;
        let fill_color = if muted { RED } else { BLUE };
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
        y + 22.0
    }

    fn render_toggle_row(
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.6),
        });
        let tx = x + width - 48.0;
        let bg = if on { GREEN } else { SURFACE1 };
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y,
            width: 40.0,
            height: 20.0,
            color: bg,
            corner_radii: CornerRadii::all(10.0),
        });
        let knob_x = if on { tx + 22.0 } else { tx + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 2.0,
            width: 16.0,
            height: 16.0,
            color: TEXT,
            corner_radii: CornerRadii::all(8.0),
        });
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        let cmds = ui.render(0.0, 0.0, 500.0);
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
            let cmds = ui.render(0.0, 0.0, 500.0);
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
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_mon = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Monitor volume")));
        assert!(has_mon);
    }
}
