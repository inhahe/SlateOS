//! Wake sensor settings — webcam/microphone-based screen wake.
//!
//! Opt-in feature that wakes the screen when motion is detected via
//! the webcam or sound change via the microphone.  Requires explicit
//! user consent with a privacy warning.
//!
//! ## Design Reference
//!
//! design.txt line 1305: "Can wake up as soon as it detects enough
//!   change via the webcam or enough sound (or change in sound, or
//!   voice) from the mic? So in most cases you could just step into
//!   the room and the computer could wake up."
//!
//! design.txt line 1309 (PUSHBACK): "This requires the webcam and
//!   microphone to be always on, which is a privacy concern and power
//!   drain. Make this opt-in with a clear privacy warning.  The
//!   standard approach (wake on keyboard/mouse/touchpad input) is
//!   sufficient for most users."
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Power → Wake Sensors
//!   → wakesensor::config() → current settings
//!   → wakesensor::set_camera_enabled(true) → requires consent
//!   → wakesensor::set_mic_enabled(true) → requires consent
//!
//! Power daemon (idle monitor)
//!   → wakesensor::should_wake(motion_level, sound_level) → bool
//!   → if true → power::wake_screen()
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Sensor type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorType {
    /// Webcam motion detection.
    Camera,
    /// Microphone sound detection.
    Microphone,
}

/// Sensitivity level for wake detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    /// Low — requires significant motion or loud sound.
    Low,
    /// Medium — reasonable threshold.
    Medium,
    /// High — slight movement or quiet sound triggers wake.
    High,
    /// Custom threshold value.
    Custom,
}

/// Privacy consent state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentState {
    /// User has not been asked.
    NotAsked,
    /// User explicitly consented (with timestamp).
    Granted,
    /// User explicitly denied.
    Denied,
    /// Previously granted, now revoked.
    Revoked,
}

/// Wake event record.
#[derive(Debug, Clone)]
pub struct WakeEvent {
    /// Event ID.
    pub id: u64,
    /// Which sensor triggered the wake.
    pub sensor: SensorType,
    /// Detection level that triggered wake (0-100).
    pub level: u32,
    /// Threshold that was set at the time.
    pub threshold: u32,
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// Whether this was a false positive (user went back to sleep).
    pub false_positive: bool,
}

/// Per-sensor configuration.
#[derive(Debug, Clone)]
pub struct SensorConfig {
    /// Sensor type.
    pub sensor: SensorType,
    /// Whether this sensor is enabled for wake detection.
    pub enabled: bool,
    /// Sensitivity preset.
    pub sensitivity: Sensitivity,
    /// Custom threshold (0-100, used when sensitivity is Custom).
    pub custom_threshold: u32,
    /// Effective threshold based on sensitivity.
    pub effective_threshold: u32,
    /// Privacy consent state.
    pub consent: ConsentState,
    /// Consent timestamp (ns, when granted).
    pub consent_ns: u64,
    /// Power draw estimate (mW) when active.
    pub power_draw_mw: u32,
    /// Whether to show LED indicator when sensor is active.
    pub show_indicator: bool,
    /// Schedule: only active during these hours (None = always).
    pub active_hours_start: Option<u8>,
    /// Schedule: end hour.
    pub active_hours_end: Option<u8>,
    /// Number of wake events.
    pub wake_count: u64,
    /// Number of false positives.
    pub false_positive_count: u64,
}

/// Overall wake sensor configuration.
#[derive(Debug, Clone)]
pub struct WakeSensorConfig {
    /// Camera sensor config.
    pub camera: SensorConfig,
    /// Microphone sensor config.
    pub mic: SensorConfig,
    /// Global enable (master switch).
    pub globally_enabled: bool,
    /// Cooldown between wake events in seconds (avoid rapid re-triggers).
    pub cooldown_secs: u32,
    /// Whether to log all wake events.
    pub log_events: bool,
    /// Maximum stored events.
    pub max_events: usize,
    /// Whether to auto-disable on battery power.
    pub disable_on_battery: bool,
    /// Privacy warning text shown to user.
    pub privacy_warning: String,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_EVENTS: usize = 256;

const PRIVACY_WARNING: &str = "\
This feature keeps the webcam and/or microphone active while the screen \
is off, which uses additional power and may raise privacy concerns. \
No audio or video data is recorded or transmitted — only motion/sound \
levels are monitored. An LED indicator will show when sensors are active. \
You can revoke consent at any time in Settings → Power → Wake Sensors.";

fn threshold_for(sensitivity: Sensitivity, custom: u32) -> u32 {
    match sensitivity {
        Sensitivity::Low => 70,
        Sensitivity::Medium => 40,
        Sensitivity::High => 15,
        Sensitivity::Custom => custom.min(100),
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: WakeSensorConfig,
    events: Vec<WakeEvent>,
    next_event_id: u64,
    changes: u64,
}

fn default_sensor(sensor: SensorType) -> SensorConfig {
    SensorConfig {
        sensor,
        enabled: false,
        sensitivity: Sensitivity::Medium,
        custom_threshold: 40,
        effective_threshold: 40,
        consent: ConsentState::NotAsked,
        consent_ns: 0,
        power_draw_mw: match sensor {
            SensorType::Camera => 200,
            SensorType::Microphone => 50,
        },
        show_indicator: true,
        active_hours_start: None,
        active_hours_end: None,
        wake_count: 0,
        false_positive_count: 0,
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    config: WakeSensorConfig {
        camera: SensorConfig {
            sensor: SensorType::Camera,
            enabled: false,
            sensitivity: Sensitivity::Medium,
            custom_threshold: 40,
            effective_threshold: 40,
            consent: ConsentState::NotAsked,
            consent_ns: 0,
            power_draw_mw: 200,
            show_indicator: true,
            active_hours_start: None,
            active_hours_end: None,
            wake_count: 0,
            false_positive_count: 0,
        },
        mic: SensorConfig {
            sensor: SensorType::Microphone,
            enabled: false,
            sensitivity: Sensitivity::Medium,
            custom_threshold: 40,
            effective_threshold: 40,
            consent: ConsentState::NotAsked,
            consent_ns: 0,
            power_draw_mw: 50,
            show_indicator: true,
            active_hours_start: None,
            active_hours_end: None,
            wake_count: 0,
            false_positive_count: 0,
        },
        globally_enabled: false,
        cooldown_secs: 10,
        log_events: true,
        max_events: MAX_EVENTS,
        disable_on_battery: true,
        privacy_warning: String::new(),
    },
    events: Vec::new(),
    next_event_id: 1,
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current configuration.
pub fn config() -> WakeSensorConfig {
    let state = STATE.lock();
    let mut cfg = state.config.clone();
    if cfg.privacy_warning.is_empty() {
        cfg.privacy_warning = String::from(PRIVACY_WARNING);
    }
    cfg
}

/// Grant privacy consent for a sensor.
pub fn grant_consent(sensor: SensorType) -> KernelResult<()> {
    let timestamp = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();
    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    sc.consent = ConsentState::Granted;
    sc.consent_ns = timestamp;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Revoke consent for a sensor (also disables it).
pub fn revoke_consent(sensor: SensorType) -> KernelResult<()> {
    let mut state = STATE.lock();
    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    sc.consent = ConsentState::Revoked;
    sc.enabled = false;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Enable a sensor (requires consent).
pub fn set_sensor_enabled(sensor: SensorType, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    if enabled && sc.consent != ConsentState::Granted {
        return Err(KernelError::PermissionDenied);
    }
    sc.enabled = enabled;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set sensitivity for a sensor.
pub fn set_sensitivity(sensor: SensorType, sensitivity: Sensitivity) -> KernelResult<()> {
    let mut state = STATE.lock();
    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    sc.sensitivity = sensitivity;
    sc.effective_threshold = threshold_for(sensitivity, sc.custom_threshold);
    state.changes += 1;
    Ok(())
}

/// Set custom threshold (0-100).
pub fn set_custom_threshold(sensor: SensorType, threshold: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    sc.custom_threshold = threshold.min(100);
    if sc.sensitivity == Sensitivity::Custom {
        sc.effective_threshold = sc.custom_threshold;
    }
    state.changes += 1;
    Ok(())
}

/// Set LED indicator visibility.
pub fn set_show_indicator(sensor: SensorType, show: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    sc.show_indicator = show;
    state.changes += 1;
    Ok(())
}

/// Set active hours schedule (None = always active).
pub fn set_active_hours(
    sensor: SensorType,
    start: Option<u8>,
    end: Option<u8>,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    // Validate hours.
    if let Some(h) = start {
        if h > 23 { return Err(KernelError::InvalidArgument); }
    }
    if let Some(h) = end {
        if h > 23 { return Err(KernelError::InvalidArgument); }
    }
    sc.active_hours_start = start;
    sc.active_hours_end = end;
    state.changes += 1;
    Ok(())
}

/// Set global enable.
pub fn set_globally_enabled(enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.globally_enabled = enabled;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set cooldown seconds.
pub fn set_cooldown(seconds: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.cooldown_secs = seconds.clamp(1, 300);
    state.changes += 1;
    Ok(())
}

/// Set log events flag.
pub fn set_log_events(log: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.log_events = log;
    state.changes += 1;
    Ok(())
}

/// Set battery policy.
pub fn set_disable_on_battery(disable: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.disable_on_battery = disable;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Wake decision
// ---------------------------------------------------------------------------

/// Determine if sensors should trigger a wake.
///
/// Called by the power daemon's idle monitor with current sensor readings.
/// Returns true if wake should occur.
pub fn should_wake(motion_level: u32, sound_level: u32) -> bool {
    let state = STATE.lock();
    if !state.config.globally_enabled {
        return false;
    }

    let cam = &state.config.camera;
    if cam.enabled && cam.consent == ConsentState::Granted
        && motion_level >= cam.effective_threshold
    {
        return true;
    }

    let mic = &state.config.mic;
    if mic.enabled && mic.consent == ConsentState::Granted
        && sound_level >= mic.effective_threshold
    {
        return true;
    }

    false
}

/// Record a wake event from a sensor trigger.
pub fn record_wake(sensor: SensorType, level: u32) {
    let timestamp = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    let sc = match sensor {
        SensorType::Camera => &mut state.config.camera,
        SensorType::Microphone => &mut state.config.mic,
    };
    sc.wake_count += 1;
    let threshold = sc.effective_threshold;

    if state.config.log_events {
        let id = state.next_event_id;
        state.next_event_id += 1;
        if state.events.len() >= state.config.max_events {
            state.events.remove(0);
        }
        state.events.push(WakeEvent {
            id,
            sensor,
            level,
            threshold,
            timestamp_ns: timestamp,
            false_positive: false,
        });
    }

    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Mark a wake event as false positive.
pub fn mark_false_positive(event_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let event = state.events.iter_mut().find(|e| e.id == event_id)
        .ok_or(KernelError::NotFound)?;
    if !event.false_positive {
        event.false_positive = true;
        let sensor = event.sensor;
        let sc = match sensor {
            SensorType::Camera => &mut state.config.camera,
            SensorType::Microphone => &mut state.config.mic,
        };
        sc.false_positive_count += 1;
    }
    state.changes += 1;
    Ok(())
}

/// Get wake event history.
pub fn wake_events() -> Vec<WakeEvent> {
    STATE.lock().events.clone()
}

/// Clear wake event history.
pub fn clear_events() {
    let mut state = STATE.lock();
    state.events.clear();
    state.changes += 1;
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with conservative defaults (everything disabled).
pub fn init_defaults() {
    let mut state = STATE.lock();
    state.config = WakeSensorConfig {
        camera: default_sensor(SensorType::Camera),
        mic: default_sensor(SensorType::Microphone),
        globally_enabled: false,
        cooldown_secs: 10,
        log_events: true,
        max_events: MAX_EVENTS,
        disable_on_battery: true,
        privacy_warning: String::from(PRIVACY_WARNING),
    };
    state.events.clear();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Return (globally_enabled, camera_enabled, mic_enabled, event_count, ops).
pub fn stats() -> (bool, bool, bool, usize, u64) {
    let state = STATE.lock();
    (state.config.globally_enabled,
     state.config.camera.enabled,
     state.config.mic.enabled,
     state.events.len(),
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.config.camera = default_sensor(SensorType::Camera);
    state.config.mic = default_sensor(SensorType::Microphone);
    state.config.globally_enabled = false;
    state.events.clear();
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: default state — everything disabled.
    serial_println!("wakesensor::self_test 1: defaults");
    let cfg = config();
    assert!(!cfg.globally_enabled);
    assert!(!cfg.camera.enabled);
    assert!(!cfg.mic.enabled);
    assert_eq!(cfg.camera.consent, ConsentState::NotAsked);

    // Test 2: enable without consent fails.
    serial_println!("wakesensor::self_test 2: consent required");
    assert!(set_sensor_enabled(SensorType::Camera, true).is_err());

    // Test 3: grant consent then enable.
    serial_println!("wakesensor::self_test 3: consent + enable");
    grant_consent(SensorType::Camera)?;
    set_sensor_enabled(SensorType::Camera, true)?;
    let cfg = config();
    assert_eq!(cfg.camera.consent, ConsentState::Granted);
    assert!(cfg.camera.enabled);

    // Test 4: revoke consent disables.
    serial_println!("wakesensor::self_test 4: revoke");
    revoke_consent(SensorType::Camera)?;
    let cfg = config();
    assert_eq!(cfg.camera.consent, ConsentState::Revoked);
    assert!(!cfg.camera.enabled);

    // Test 5: sensitivity.
    serial_println!("wakesensor::self_test 5: sensitivity");
    grant_consent(SensorType::Camera)?;
    set_sensitivity(SensorType::Camera, Sensitivity::High)?;
    let cfg = config();
    assert_eq!(cfg.camera.effective_threshold, 15);
    set_sensitivity(SensorType::Camera, Sensitivity::Low)?;
    let cfg = config();
    assert_eq!(cfg.camera.effective_threshold, 70);

    // Test 6: custom threshold.
    serial_println!("wakesensor::self_test 6: custom threshold");
    set_sensitivity(SensorType::Camera, Sensitivity::Custom)?;
    set_custom_threshold(SensorType::Camera, 55)?;
    let cfg = config();
    assert_eq!(cfg.camera.effective_threshold, 55);

    // Test 7: should_wake logic.
    serial_println!("wakesensor::self_test 7: should_wake");
    set_sensor_enabled(SensorType::Camera, true)?;
    set_globally_enabled(true)?;
    // Below threshold.
    assert!(!should_wake(50, 0));
    // At threshold.
    assert!(should_wake(55, 0));
    // Above threshold.
    assert!(should_wake(80, 0));

    // Test 8: mic sensor.
    serial_println!("wakesensor::self_test 8: mic");
    grant_consent(SensorType::Microphone)?;
    set_sensor_enabled(SensorType::Microphone, true)?;
    set_sensitivity(SensorType::Microphone, Sensitivity::Medium)?;
    assert!(should_wake(0, 50));
    assert!(!should_wake(0, 30));

    // Test 9: wake events.
    serial_println!("wakesensor::self_test 9: events");
    record_wake(SensorType::Camera, 60);
    record_wake(SensorType::Microphone, 50);
    let events = wake_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].sensor, SensorType::Camera);

    // Test 10: false positive.
    serial_println!("wakesensor::self_test 10: false positive");
    let eid = events[0].id;
    mark_false_positive(eid)?;
    let cfg = config();
    assert_eq!(cfg.camera.false_positive_count, 1);

    // Test 11: active hours.
    serial_println!("wakesensor::self_test 11: active hours");
    set_active_hours(SensorType::Camera, Some(22), Some(7))?;
    let cfg = config();
    assert_eq!(cfg.camera.active_hours_start, Some(22));
    assert_eq!(cfg.camera.active_hours_end, Some(7));
    // Invalid hour.
    assert!(set_active_hours(SensorType::Camera, Some(25), None).is_err());

    clear_all();
    serial_println!("wakesensor::self_test: all 11 tests passed");
    Ok(())
}
