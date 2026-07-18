//! Sound mixer — per-application volume control and audio routing.
//!
//! Manages audio volume at the system level and per application,
//! analogous to the Windows Volume Mixer.  The mixer tracks which
//! apps are currently producing sound and provides a unified API
//! for the system tray volume icon and settings panel.
//!
//! ## Design Reference
//!
//! design.txt line 711: "volume, can open sound mixer that includes
//! volume for any running program (shows programs currently playing
//! sound first)"
//!
//! design.txt line 1120: "can always view a history of which programs
//! recently either played a system sound or emitted any sound"
//!
//! ## Architecture
//!
//! ```text
//! System tray volume icon
//!   → soundmixer::master_volume() / set_master_volume()
//!   → soundmixer::app_entries()  (per-app volumes for mixer UI)
//!
//! Audio driver / mixer daemon
//!   → soundmixer::register_stream() (when app starts producing audio)
//!   → soundmixer::unregister_stream()
//!   → soundmixer::report_activity() (marks app as actively playing)
//!
//! Settings / history view
//!   → soundmixer::sound_history()
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum concurrent audio streams.
const MAX_STREAMS: usize = 256;

/// Maximum per-app entries.
const MAX_APPS: usize = 128;

/// Maximum sound history entries.
const MAX_HISTORY: usize = 512;

/// Maximum audio output devices.
const MAX_DEVICES: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Audio output device.
#[derive(Debug, Clone)]
pub struct AudioDevice {
    /// Device identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Whether this is the default output device.
    pub is_default: bool,
    /// Device volume (0-100).
    pub volume: u8,
    /// Whether the device is muted.
    pub muted: bool,
}

/// An application's audio state in the mixer.
#[derive(Debug, Clone)]
pub struct AppAudioEntry {
    /// Application ID (e.g., "com.mint.mediaplayer").
    pub app_id: String,
    /// Display name.
    pub app_name: String,
    /// Per-app volume (0-100).
    pub volume: u8,
    /// Whether this app is muted.
    pub muted: bool,
    /// Number of active streams from this app.
    pub stream_count: u32,
    /// Whether the app is currently producing sound.
    pub playing: bool,
    /// Last time this app produced sound (nanoseconds).
    pub last_sound_ns: u64,
    /// Target output device (None = default device).
    pub output_device: Option<String>,
}

/// An active audio stream.
#[derive(Debug, Clone)]
pub struct AudioStream {
    /// Unique stream ID.
    pub id: u64,
    /// Application that owns this stream.
    pub app_id: String,
    /// Stream label (e.g., "Music Playback", "Notification").
    pub label: String,
    /// Stream-level volume multiplier (0-100).
    pub volume: u8,
    /// Whether this stream is muted.
    pub muted: bool,
    /// Stream category.
    pub category: StreamCategory,
    /// Whether this stream is actively playing.
    pub active: bool,
    /// Target output device override.
    pub output_device: Option<String>,
}

/// Audio stream category for priority and routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamCategory {
    /// System sounds (notifications, alerts).
    System,
    /// Voice communication (calls, VoIP).
    Communication,
    /// Media playback (music, video).
    Media,
    /// Game audio.
    Game,
    /// General application audio.
    Application,
}

impl StreamCategory {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Communication => "Communication",
            Self::Media => "Media",
            Self::Game => "Game",
            Self::Application => "Application",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "system" | "sys" => Some(Self::System),
            "comm" | "communication" | "voice" => Some(Self::Communication),
            "media" | "music" | "video" => Some(Self::Media),
            "game" => Some(Self::Game),
            "app" | "application" => Some(Self::Application),
            _ => None,
        }
    }
}

/// Sound history entry — records when an app made sound.
#[derive(Debug, Clone)]
pub struct SoundHistoryEntry {
    /// Application ID.
    pub app_id: String,
    /// Application name.
    pub app_name: String,
    /// What kind of sound (category label or stream label).
    pub description: String,
    /// Timestamp (nanoseconds).
    pub timestamp_ns: u64,
}

/// Ducking policy — how to handle audio when multiple apps play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuckingPolicy {
    /// No automatic ducking.
    None,
    /// Duck other audio when communication audio plays.
    DuckOnCommunication,
    /// Duck all non-foreground audio.
    DuckBackground,
}

impl DuckingPolicy {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::DuckOnCommunication => "Duck on Communication",
            Self::DuckBackground => "Duck Background",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "none" | "off" => Some(Self::None),
            "comm" | "communication" => Some(Self::DuckOnCommunication),
            "bg" | "background" => Some(Self::DuckBackground),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct MixerState {
    /// Master volume (0-100).
    master_volume: u8,
    /// Master mute.
    master_muted: bool,
    /// Output devices.
    devices: BTreeMap<String, AudioDevice>,
    /// Default device ID.
    default_device: String,
    /// Per-app audio entries.
    apps: BTreeMap<String, AppAudioEntry>,
    /// Active streams.
    streams: BTreeMap<u64, AudioStream>,
    /// Sound history (ring buffer).
    history: Vec<SoundHistoryEntry>,
    /// History write index (wraps at MAX_HISTORY).
    history_idx: usize,
    /// Audio ducking policy.
    ducking: DuckingPolicy,
    /// Next stream ID.
    next_stream_id: u64,
}

impl MixerState {
    const fn new() -> Self {
        Self {
            master_volume: 75,
            master_muted: false,
            devices: BTreeMap::new(),
            default_device: String::new(),
            apps: BTreeMap::new(),
            streams: BTreeMap::new(),
            history: Vec::new(),
            history_idx: 0,
            ducking: DuckingPolicy::DuckOnCommunication,
            next_stream_id: 1,
        }
    }

    fn add_history(&mut self, app_id: &str, app_name: &str, desc: &str) {
        let now = crate::timekeeping::clock_monotonic();
        let entry = SoundHistoryEntry {
            app_id: String::from(app_id),
            app_name: String::from(app_name),
            description: String::from(desc),
            timestamp_ns: now,
        };
        if self.history.len() < MAX_HISTORY {
            self.history.push(entry);
        } else {
            let idx = self.history_idx % MAX_HISTORY;
            if let Some(slot) = self.history.get_mut(idx) {
                *slot = entry;
            }
        }
        self.history_idx = self.history_idx.wrapping_add(1);
    }
}

static MIXER: Mutex<MixerState> = Mutex::new(MixerState::new());
static VOLUME_CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);
static STREAM_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Master volume
// ---------------------------------------------------------------------------

/// Get current master volume (0-100).
pub fn master_volume() -> u8 {
    MIXER.lock().master_volume
}

/// Set master volume (0-100).
pub fn set_master_volume(vol: u8) {
    VOLUME_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    MIXER.lock().master_volume = vol.min(100);
}

/// Get master mute state.
pub fn master_muted() -> bool {
    MIXER.lock().master_muted
}

/// Set master mute.
pub fn set_master_muted(muted: bool) {
    VOLUME_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    MIXER.lock().master_muted = muted;
}

/// Toggle master mute.
pub fn toggle_master_mute() -> bool {
    VOLUME_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = MIXER.lock();
    state.master_muted = !state.master_muted;
    state.master_muted
}

// ---------------------------------------------------------------------------
// Device management
// ---------------------------------------------------------------------------

/// Add an audio output device.
pub fn add_device(id: &str, name: &str) -> KernelResult<()> {
    if id.is_empty() || name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = MIXER.lock();
    if state.devices.len() >= MAX_DEVICES && !state.devices.contains_key(id) {
        return Err(KernelError::ResourceExhausted);
    }
    let is_first = state.devices.is_empty();
    state.devices.insert(String::from(id), AudioDevice {
        id: String::from(id),
        name: String::from(name),
        is_default: is_first,
        volume: 100,
        muted: false,
    });
    if is_first {
        state.default_device = String::from(id);
    }
    Ok(())
}

/// Remove an audio device.
pub fn remove_device(id: &str) -> KernelResult<()> {
    let mut state = MIXER.lock();
    state.devices.remove(id).ok_or(KernelError::NotFound)?;
    // If we removed the default, pick another.
    if state.default_device == id {
        let new_default = state.devices.keys()
            .next()
            .cloned()
            .unwrap_or_default();
        state.default_device = new_default.clone();
        if let Some(dev) = state.devices.get_mut(&new_default) {
            dev.is_default = true;
        }
    }
    Ok(())
}

/// Set the default output device.
pub fn set_default_device(id: &str) -> KernelResult<()> {
    let mut state = MIXER.lock();
    if !state.devices.contains_key(id) {
        return Err(KernelError::NotFound);
    }
    // Clear old default.
    let old_default = state.default_device.clone();
    if let Some(old) = state.devices.get_mut(&old_default) {
        old.is_default = false;
    }
    state.default_device = String::from(id);
    if let Some(dev) = state.devices.get_mut(id) {
        dev.is_default = true;
    }
    Ok(())
}

/// Set device volume.
pub fn set_device_volume(id: &str, vol: u8) -> KernelResult<()> {
    VOLUME_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = MIXER.lock();
    let dev = state.devices.get_mut(id).ok_or(KernelError::NotFound)?;
    dev.volume = vol.min(100);
    Ok(())
}

/// Set device mute.
pub fn set_device_muted(id: &str, muted: bool) -> KernelResult<()> {
    let mut state = MIXER.lock();
    let dev = state.devices.get_mut(id).ok_or(KernelError::NotFound)?;
    dev.muted = muted;
    Ok(())
}

/// List all output devices.
pub fn list_devices() -> Vec<AudioDevice> {
    let state = MIXER.lock();
    state.devices.values().cloned().collect()
}

// ---------------------------------------------------------------------------
// Per-app volume
// ---------------------------------------------------------------------------

/// Set per-app volume (0-100). Creates entry if needed.
pub fn set_app_volume(app_id: &str, vol: u8) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    VOLUME_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = MIXER.lock();
    if let Some(entry) = state.apps.get_mut(app_id) {
        entry.volume = vol.min(100);
    } else {
        if state.apps.len() >= MAX_APPS {
            return Err(KernelError::ResourceExhausted);
        }
        state.apps.insert(String::from(app_id), AppAudioEntry {
            app_id: String::from(app_id),
            app_name: String::from(app_id),
            volume: vol.min(100),
            muted: false,
            stream_count: 0,
            playing: false,
            last_sound_ns: 0,
            output_device: None,
        });
    }
    Ok(())
}

/// Set per-app mute.
pub fn set_app_muted(app_id: &str, muted: bool) -> KernelResult<()> {
    let mut state = MIXER.lock();
    let entry = state.apps.get_mut(app_id).ok_or(KernelError::NotFound)?;
    entry.muted = muted;
    Ok(())
}

/// Set per-app output device routing.
pub fn set_app_output(app_id: &str, device_id: Option<&str>) -> KernelResult<()> {
    let mut state = MIXER.lock();
    // Validate device first, before taking mutable ref to apps.
    if let Some(did) = device_id {
        if !state.devices.contains_key(did) {
            return Err(KernelError::NotFound);
        }
    }
    let entry = state.apps.get_mut(app_id).ok_or(KernelError::NotFound)?;
    entry.output_device = device_id.map(String::from);
    Ok(())
}

/// Get all app mixer entries. Apps currently playing appear first.
pub fn app_entries() -> Vec<AppAudioEntry> {
    let state = MIXER.lock();
    let mut entries: Vec<AppAudioEntry> = state.apps.values().cloned().collect();
    // Sort: playing first, then by last_sound_ns descending.
    entries.sort_by(|a, b| {
        b.playing.cmp(&a.playing)
            .then(b.last_sound_ns.cmp(&a.last_sound_ns))
    });
    entries
}

/// Get a specific app's audio entry.
pub fn get_app_entry(app_id: &str) -> Option<AppAudioEntry> {
    MIXER.lock().apps.get(app_id).cloned()
}

/// Compute effective volume for an app (master × device × app).
pub fn effective_volume(app_id: &str) -> u8 {
    let state = MIXER.lock();
    if state.master_muted {
        return 0;
    }
    let master = state.master_volume as u32;
    let device_vol = if let Some(entry) = state.apps.get(app_id) {
        let dev_id = entry.output_device.as_deref()
            .unwrap_or(&state.default_device);
        if let Some(dev) = state.devices.get(dev_id) {
            if dev.muted { return 0; }
            dev.volume as u32
        } else {
            100u32
        }
    } else {
        100u32
    };
    let app_vol = state.apps.get(app_id)
        .map(|e| {
            if e.muted { 0u32 } else { e.volume as u32 }
        })
        .unwrap_or(100u32);

    // Multiply all three: master × device × app, each in 0-100.
    let result = master.saturating_mul(device_vol).saturating_mul(app_vol) / 10000;
    result.min(100) as u8
}

// ---------------------------------------------------------------------------
// Stream management
// ---------------------------------------------------------------------------

/// Register an audio stream for an application.
pub fn register_stream(app_id: &str, app_name: &str, label: &str,
                       category: StreamCategory) -> KernelResult<u64> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    STREAM_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = MIXER.lock();
    if state.streams.len() >= MAX_STREAMS {
        return Err(KernelError::ResourceExhausted);
    }

    let id = state.next_stream_id;
    state.next_stream_id = state.next_stream_id.saturating_add(1);

    state.streams.insert(id, AudioStream {
        id,
        app_id: String::from(app_id),
        label: String::from(label),
        volume: 100,
        muted: false,
        category,
        active: false,
        output_device: None,
    });

    // Ensure app entry exists.
    if !state.apps.contains_key(app_id) {
        if state.apps.len() < MAX_APPS {
            state.apps.insert(String::from(app_id), AppAudioEntry {
                app_id: String::from(app_id),
                app_name: String::from(app_name),
                volume: 100,
                muted: false,
                stream_count: 0,
                playing: false,
                last_sound_ns: 0,
                output_device: None,
            });
        }
    }

    // Increment stream count for this app.
    if let Some(entry) = state.apps.get_mut(app_id) {
        entry.stream_count = entry.stream_count.saturating_add(1);
        // Update name if it was just the app_id.
        if entry.app_name == entry.app_id && !app_name.is_empty() {
            entry.app_name = String::from(app_name);
        }
    }

    Ok(id)
}

/// Unregister an audio stream.
pub fn unregister_stream(stream_id: u64) -> KernelResult<()> {
    let mut state = MIXER.lock();
    let stream = state.streams.remove(&stream_id)
        .ok_or(KernelError::NotFound)?;

    // Decrement app stream count.
    if let Some(entry) = state.apps.get_mut(&stream.app_id) {
        entry.stream_count = entry.stream_count.saturating_sub(1);
        // If no more streams, mark as not playing.
        if entry.stream_count == 0 {
            entry.playing = false;
        }
    }

    Ok(())
}

/// Report that a stream is actively producing audio.
pub fn report_activity(stream_id: u64) -> KernelResult<()> {
    let mut state = MIXER.lock();
    let stream = state.streams.get_mut(&stream_id)
        .ok_or(KernelError::NotFound)?;
    stream.active = true;
    let app_id = stream.app_id.clone();
    let label = stream.label.clone();
    let category = stream.category;

    let now = crate::timekeeping::clock_monotonic();
    if let Some(entry) = state.apps.get_mut(&app_id) {
        let was_playing = entry.playing;
        entry.playing = true;
        entry.last_sound_ns = now;

        // Record to history if newly started playing.
        if !was_playing {
            let name = entry.app_name.clone();
            let desc = if label.is_empty() {
                String::from(category.label())
            } else {
                label
            };
            state.add_history(&app_id, &name, &desc);
        }
    }

    Ok(())
}

/// Mark a stream as no longer active (paused/stopped but not unregistered).
pub fn report_inactive(stream_id: u64) -> KernelResult<()> {
    let mut state = MIXER.lock();
    let stream = state.streams.get_mut(&stream_id)
        .ok_or(KernelError::NotFound)?;
    stream.active = false;
    let app_id = stream.app_id.clone();

    // Check if any other streams for this app are still active.
    let any_active = state.streams.values()
        .any(|s| s.app_id == app_id && s.active);
    if !any_active {
        if let Some(entry) = state.apps.get_mut(&app_id) {
            entry.playing = false;
        }
    }

    Ok(())
}

/// Set volume on a specific stream (0-100).
pub fn set_stream_volume(stream_id: u64, vol: u8) -> KernelResult<()> {
    VOLUME_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = MIXER.lock();
    let stream = state.streams.get_mut(&stream_id)
        .ok_or(KernelError::NotFound)?;
    stream.volume = vol.min(100);
    Ok(())
}

/// List all active streams.
pub fn list_streams() -> Vec<AudioStream> {
    MIXER.lock().streams.values().cloned().collect()
}

/// List streams for a specific app.
pub fn app_streams(app_id: &str) -> Vec<AudioStream> {
    MIXER.lock().streams.values()
        .filter(|s| s.app_id == app_id)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Sound history (design.txt line 1120)
// ---------------------------------------------------------------------------

/// Get sound history (most recent first).
pub fn sound_history() -> Vec<SoundHistoryEntry> {
    let state = MIXER.lock();
    let mut entries = state.history.clone();
    entries.sort_by_key(|e| core::cmp::Reverse(e.timestamp_ns));
    entries
}

/// Get history entries for a specific app.
pub fn app_history(app_id: &str) -> Vec<SoundHistoryEntry> {
    let state = MIXER.lock();
    state.history.iter()
        .filter(|e| e.app_id == app_id)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Ducking
// ---------------------------------------------------------------------------

/// Get current ducking policy.
pub fn ducking_policy() -> DuckingPolicy {
    MIXER.lock().ducking
}

/// Set ducking policy.
pub fn set_ducking_policy(policy: DuckingPolicy) {
    MIXER.lock().ducking = policy;
}

/// Check if any communication streams are active (triggers ducking).
pub fn communication_active() -> bool {
    MIXER.lock().streams.values()
        .any(|s| s.active && s.category == StreamCategory::Communication)
}

/// Compute ducking factor for a given category (0-100, where 100 = no duck).
pub fn ducking_factor(category: StreamCategory) -> u8 {
    let state = MIXER.lock();
    match state.ducking {
        DuckingPolicy::None => 100,
        DuckingPolicy::DuckOnCommunication => {
            if category == StreamCategory::Communication {
                return 100;
            }
            let has_comm = state.streams.values()
                .any(|s| s.active && s.category == StreamCategory::Communication);
            if has_comm { 30 } else { 100 }
        }
        DuckingPolicy::DuckBackground => {
            // For this simplified model, System and Communication are never ducked.
            match category {
                StreamCategory::System | StreamCategory::Communication => 100,
                _ => {
                    let has_comm = state.streams.values()
                        .any(|s| s.active && s.category == StreamCategory::Communication);
                    if has_comm { 20 } else { 100 }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Initialize with default audio device.
pub fn init_defaults() {
    let mut state = MIXER.lock();
    if state.devices.is_empty() {
        state.devices.insert(String::from("default"), AudioDevice {
            id: String::from("default"),
            name: String::from("System Audio Output"),
            is_default: true,
            volume: 100,
            muted: false,
        });
        state.default_device = String::from("default");
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (stream_count, app_count, device_count, volume_changes, total_streams_created).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let state = MIXER.lock();
    (
        state.streams.len(),
        state.apps.len(),
        state.devices.len(),
        VOLUME_CHANGE_COUNT.load(Ordering::Relaxed),
        STREAM_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset counters.
pub fn reset_stats() {
    VOLUME_CHANGE_COUNT.store(0, Ordering::Relaxed);
    STREAM_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut state = MIXER.lock();
    state.master_volume = 75;
    state.master_muted = false;
    state.devices.clear();
    state.default_device = String::new();
    state.apps.clear();
    state.streams.clear();
    state.history.clear();
    state.history_idx = 0;
    state.ducking = DuckingPolicy::DuckOnCommunication;
    state.next_stream_id = 1;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the sound mixer.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Save state.
    let saved_master = master_volume();
    let saved_muted = master_muted();
    clear_all();
    reset_stats();

    // Test 1: Master volume.
    serial_println!("  soundmixer::test 1: master volume");
    set_master_volume(80);
    assert_eq!(master_volume(), 80);
    set_master_volume(150); // Clamped to 100.
    assert_eq!(master_volume(), 100);
    set_master_muted(true);
    assert!(master_muted());
    let new_mute = toggle_master_mute();
    assert!(!new_mute);

    // Test 2: Device management.
    serial_println!("  soundmixer::test 2: devices");
    add_device("hdmi0", "HDMI Output")?;
    add_device("speakers", "Built-in Speakers")?;
    let devs = list_devices();
    assert_eq!(devs.len(), 2);
    // First device should be default.
    set_default_device("speakers")?;
    let devs2 = list_devices();
    let speakers = devs2.iter().find(|d| d.id == "speakers");
    assert!(speakers.is_some());
    assert!(speakers.is_some_and(|d| d.is_default));
    set_device_volume("hdmi0", 50)?;
    remove_device("hdmi0")?;
    assert_eq!(list_devices().len(), 1);

    // Test 3: Stream registration.
    serial_println!("  soundmixer::test 3: streams");
    let s1 = register_stream("player", "Media Player", "Music", StreamCategory::Media)?;
    let s2 = register_stream("player", "Media Player", "Effects", StreamCategory::Application)?;
    let s3 = register_stream("browser", "Web Browser", "Tab Audio", StreamCategory::Media)?;
    assert_eq!(list_streams().len(), 3);

    // Test 4: Activity reporting and history.
    serial_println!("  soundmixer::test 4: activity and history");
    report_activity(s1)?;
    let entry = get_app_entry("player");
    assert!(entry.is_some());
    assert!(entry.is_some_and(|e| e.playing));
    report_inactive(s1)?;
    // s2 is for same app — still has streams but s2 isn't active either.
    let entry2 = get_app_entry("player");
    assert!(entry2.is_none_or(|e| !e.playing));
    let hist = sound_history();
    assert!(!hist.is_empty());

    // Test 5: Per-app volume and effective volume.
    serial_println!("  soundmixer::test 5: per-app volume");
    set_master_volume(50);
    set_app_volume("player", 80)?;
    // Effective: 50 * 100 * 80 / 10000 = 40.
    let eff = effective_volume("player");
    assert_eq!(eff, 40);
    set_master_muted(true);
    assert_eq!(effective_volume("player"), 0);
    set_master_muted(false);

    // Test 6: App entries sorted (playing first).
    serial_println!("  soundmixer::test 6: app entry ordering");
    report_activity(s3)?;
    let entries = app_entries();
    // "browser" should be first (playing), "player" second (not playing).
    assert!(entries.len() >= 2);
    if let Some(first) = entries.first() {
        assert_eq!(first.app_id, "browser");
        assert!(first.playing);
    }

    // Test 7: Ducking.
    serial_println!("  soundmixer::test 7: ducking");
    set_ducking_policy(DuckingPolicy::DuckOnCommunication);
    let comm = register_stream("voip", "VoIP App", "Call", StreamCategory::Communication)?;
    report_activity(comm)?;
    assert!(communication_active());
    // Media should be ducked.
    assert_eq!(ducking_factor(StreamCategory::Media), 30);
    // Communication itself should not be ducked.
    assert_eq!(ducking_factor(StreamCategory::Communication), 100);

    // Cleanup.
    let _ = unregister_stream(s1);
    let _ = unregister_stream(s2);
    let _ = unregister_stream(s3);
    let _ = unregister_stream(comm);
    clear_all();
    reset_stats();
    set_master_volume(saved_master);
    set_master_muted(saved_muted);

    serial_println!("  soundmixer: all tests passed");
    Ok(())
}
