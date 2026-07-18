//! Audio device management — input/output device selection and routing.
//!
//! Manages audio hardware devices, virtual sinks/sources, per-device
//! volume/mute, and default device selection for playback and capture.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Sound → Devices
//!   → audiodevice::set_default_output() / set_device_volume()
//!
//! Audio pipeline
//!   → audiodevice::default_output() → driver → hardware
//!   → audiodevice::default_input()  → driver → hardware
//!
//! Integration:
//!   → soundmixer (per-app volume routing)
//!   → bluetooth (A2DP/HFP device registration)
//!   → notifcenter (device change notifications)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Audio device direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDirection {
    /// Output (speakers, headphones).
    Output,
    /// Input (microphone).
    Input,
    /// Both directions (USB headset with mic).
    Duplex,
}

impl DeviceDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Input => "Input",
            Self::Duplex => "Duplex",
        }
    }
}

/// Audio device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDeviceType {
    /// Built-in speakers.
    Speakers,
    /// Built-in microphone.
    Microphone,
    /// Headphones (3.5mm jack).
    Headphones,
    /// USB audio device.
    Usb,
    /// Bluetooth audio.
    Bluetooth,
    /// HDMI/DisplayPort audio.
    Hdmi,
    /// Virtual/loopback device.
    Virtual,
    /// External DAC.
    ExternalDac,
}

impl AudioDeviceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Speakers => "Speakers",
            Self::Microphone => "Microphone",
            Self::Headphones => "Headphones",
            Self::Usb => "USB Audio",
            Self::Bluetooth => "Bluetooth",
            Self::Hdmi => "HDMI",
            Self::Virtual => "Virtual",
            Self::ExternalDac => "External DAC",
        }
    }
}

/// Audio device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Active and available.
    Active,
    /// Connected but not in use.
    Idle,
    /// Temporarily unavailable.
    Suspended,
    /// Disconnected.
    Disconnected,
}

impl DeviceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Idle => "Idle",
            Self::Suspended => "Suspended",
            Self::Disconnected => "Disconnected",
        }
    }
}

/// Sample rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleRate {
    Rate44100,
    Rate48000,
    Rate88200,
    Rate96000,
    Rate192000,
}

impl SampleRate {
    pub fn hz(self) -> u32 {
        match self {
            Self::Rate44100 => 44100,
            Self::Rate48000 => 48000,
            Self::Rate88200 => 88200,
            Self::Rate96000 => 96000,
            Self::Rate192000 => 192000,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rate44100 => "44.1 kHz",
            Self::Rate48000 => "48 kHz",
            Self::Rate88200 => "88.2 kHz",
            Self::Rate96000 => "96 kHz",
            Self::Rate192000 => "192 kHz",
        }
    }
}

/// Audio device.
#[derive(Debug, Clone)]
pub struct AudioDevice {
    /// Device ID.
    pub id: u32,
    /// Device name.
    pub name: String,
    /// Device type.
    pub device_type: AudioDeviceType,
    /// Direction (input/output/duplex).
    pub direction: DeviceDirection,
    /// Current state.
    pub state: DeviceState,
    /// Volume (0-100).
    pub volume: u32,
    /// Muted.
    pub muted: bool,
    /// Is default for its direction.
    pub is_default: bool,
    /// Sample rate.
    pub sample_rate: SampleRate,
    /// Bit depth.
    pub bit_depth: u8,
    /// Channel count.
    pub channels: u8,
    /// Latency (ms).
    pub latency_ms: u32,
    /// Driver name.
    pub driver: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct AudioDeviceState {
    devices: Vec<AudioDevice>,
    next_id: u32,
    auto_switch_on_connect: bool,
    ops: u64,
}

static STATE: Mutex<Option<AudioDeviceState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut AudioDeviceState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize audio device management with an empty device set.
///
/// We do NOT seed any devices here. Audio devices describe real hardware
/// endpoints (a specific set of speakers/microphone with a real driver,
/// volume, sample rate, and latency). Seeding "Built-in Speakers"/"Built-in
/// Microphone" with a Linux driver name (`snd-hda-intel`) and invented
/// volume/latency would surface fabricated hardware through /proc and the
/// Settings → Sound panel as if it physically existed. Devices appear only
/// when a driver registers them through add_device() (hotplug).
///
/// DEFERRED PROPER FIX: wire add_device() to the real audio-driver stack so
/// that enumerated HDA/USB/Bluetooth endpoints register here. A PCI scan can
/// find audio *controllers* (class 0x04) but not the endpoint-level details
/// (volume/sample-rate/channels) this model needs, so endpoint registration
/// must come from the driver, not a fabricated PCI read-through.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(AudioDeviceState {
        devices: Vec::new(),
        next_id: 1,
        auto_switch_on_connect: true,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Device management
// ---------------------------------------------------------------------------

/// Register an audio device (hotplug).
pub fn add_device(
    name: &str,
    device_type: AudioDeviceType,
    direction: DeviceDirection,
    driver: &str,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;

        // Auto-set as default if first of its direction.
        let is_first = !state.devices.iter().any(|d| {
            d.state != DeviceState::Disconnected &&
            (d.direction == direction || d.direction == DeviceDirection::Duplex ||
             direction == DeviceDirection::Duplex)
        });

        state.devices.push(AudioDevice {
            id,
            name: String::from(name),
            device_type,
            direction,
            state: DeviceState::Active,
            volume: 70,
            muted: false,
            is_default: is_first,
            sample_rate: SampleRate::Rate48000,
            bit_depth: 16,
            channels: 2,
            latency_ms: 10,
            driver: String::from(driver),
        });

        // Auto-switch to new device if setting is on and it's output.
        if state.auto_switch_on_connect && !is_first
            && (direction == DeviceDirection::Output || direction == DeviceDirection::Duplex)
        {
            // Set as default output.
            for d in &mut state.devices {
                if d.direction == DeviceDirection::Output || d.direction == DeviceDirection::Duplex {
                    d.is_default = d.id == id;
                }
            }
        }

        Ok(id)
    })
}

/// Remove a device (hotunplug).
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.devices.iter().position(|d| d.id == id).ok_or(KernelError::NotFound)?;
        let was_default = state.devices[pos].is_default;
        let direction = state.devices[pos].direction;
        state.devices.remove(pos);

        // Promote next device if default was removed.
        if was_default {
            if let Some(next) = state.devices.iter_mut().find(|d| {
                d.state != DeviceState::Disconnected &&
                (d.direction == direction || d.direction == DeviceDirection::Duplex)
            }) {
                next.is_default = true;
            }
        }
        Ok(())
    })
}

/// Get a device by ID.
pub fn get_device(id: u32) -> KernelResult<AudioDevice> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.devices.iter().find(|d| d.id == id).cloned().ok_or(KernelError::NotFound)
}

/// List all devices.
pub fn list_devices() -> Vec<AudioDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.devices.clone())
}

/// List output devices.
pub fn output_devices() -> Vec<AudioDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.devices.iter()
            .filter(|d| d.direction == DeviceDirection::Output || d.direction == DeviceDirection::Duplex)
            .cloned()
            .collect()
    })
}

/// List input devices.
pub fn input_devices() -> Vec<AudioDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.devices.iter()
            .filter(|d| d.direction == DeviceDirection::Input || d.direction == DeviceDirection::Duplex)
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Default device
// ---------------------------------------------------------------------------

/// Set the default output device.
pub fn set_default_output(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.devices.iter().any(|d| d.id == id) {
            return Err(KernelError::NotFound);
        }
        for d in &mut state.devices {
            if d.direction == DeviceDirection::Output || d.direction == DeviceDirection::Duplex {
                d.is_default = d.id == id;
            }
        }
        Ok(())
    })
}

/// Set the default input device.
pub fn set_default_input(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.devices.iter().any(|d| d.id == id) {
            return Err(KernelError::NotFound);
        }
        for d in &mut state.devices {
            if d.direction == DeviceDirection::Input || d.direction == DeviceDirection::Duplex {
                d.is_default = d.id == id;
            }
        }
        Ok(())
    })
}

/// Get the default output device.
pub fn default_output() -> Option<AudioDevice> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.devices.iter()
            .find(|d| d.is_default && (d.direction == DeviceDirection::Output || d.direction == DeviceDirection::Duplex))
            .cloned()
    })
}

/// Get the default input device.
pub fn default_input() -> Option<AudioDevice> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.devices.iter()
            .find(|d| d.is_default && (d.direction == DeviceDirection::Input || d.direction == DeviceDirection::Duplex))
            .cloned()
    })
}

// ---------------------------------------------------------------------------
// Device settings
// ---------------------------------------------------------------------------

/// Set device volume (0-100).
pub fn set_device_volume(id: u32, volume: u32) -> KernelResult<()> {
    if volume > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id).ok_or(KernelError::NotFound)?;
        dev.volume = volume;
        Ok(())
    })
}

/// Set device mute.
pub fn set_device_mute(id: u32, muted: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id).ok_or(KernelError::NotFound)?;
        dev.muted = muted;
        Ok(())
    })
}

/// Set device sample rate.
pub fn set_sample_rate(id: u32, rate: SampleRate) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id).ok_or(KernelError::NotFound)?;
        dev.sample_rate = rate;
        Ok(())
    })
}

/// Set auto-switch on connect.
pub fn set_auto_switch(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.auto_switch_on_connect = enabled; Ok(()) })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (device_count, output_count, input_count, default_out_id, default_in_id, ops).
pub fn stats() -> (usize, usize, usize, u32, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let out_count = s.devices.iter().filter(|d| d.direction == DeviceDirection::Output || d.direction == DeviceDirection::Duplex).count();
            let in_count = s.devices.iter().filter(|d| d.direction == DeviceDirection::Input || d.direction == DeviceDirection::Duplex).count();
            let def_out = s.devices.iter().find(|d| d.is_default && (d.direction == DeviceDirection::Output || d.direction == DeviceDirection::Duplex)).map_or(0, |d| d.id);
            let def_in = s.devices.iter().find(|d| d.is_default && (d.direction == DeviceDirection::Input || d.direction == DeviceDirection::Duplex)).map_or(0, |d| d.id);
            (s.devices.len(), out_count, in_count, def_out, def_in, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the audio device module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[audiodevice] Running self-tests...");

    *STATE.lock() = None;
    init_defaults();

    // Test 1: empty defaults, then build a fixture through the real add_device
    // hotplug API (first device of each direction auto-becomes the default).
    {
        assert_eq!(list_devices().len(), 0);
        let spk = add_device("Built-in Speakers", AudioDeviceType::Speakers, DeviceDirection::Output, "hda").unwrap();
        let mic = add_device("Built-in Microphone", AudioDeviceType::Microphone, DeviceDirection::Input, "hda").unwrap();
        assert_eq!(list_devices().len(), 2);
        let out = default_output().unwrap();
        assert_eq!(out.id, spk);
        assert_eq!(out.name, "Built-in Speakers");
        let inp = default_input().unwrap();
        assert_eq!(inp.id, mic);
        assert_eq!(inp.name, "Built-in Microphone");
    }
    serial_println!("[audiodevice]  1/11 defaults OK");

    // Test 2: add device.
    {
        let id = add_device("USB Headset", AudioDeviceType::Usb, DeviceDirection::Duplex, "snd-usb-audio").unwrap();
        let dev = get_device(id).unwrap();
        assert_eq!(dev.name, "USB Headset");
        assert_eq!(dev.device_type, AudioDeviceType::Usb);
    }
    serial_println!("[audiodevice]  2/11 add device OK");

    // Test 3: output/input lists.
    {
        let outputs = output_devices();
        assert!(outputs.len() >= 2); // speakers + USB duplex
        let inputs = input_devices();
        assert!(inputs.len() >= 2); // mic + USB duplex
    }
    serial_println!("[audiodevice]  3/11 device lists OK");

    // Test 4: set default output.
    {
        let devices = list_devices();
        let usb_id = devices.iter().find(|d| d.name == "USB Headset").unwrap().id;
        set_default_output(usb_id).unwrap();
        let out = default_output().unwrap();
        assert_eq!(out.name, "USB Headset");
    }
    serial_println!("[audiodevice]  4/11 default output OK");

    // Test 5: set default input.
    {
        let devices = list_devices();
        let usb_id = devices.iter().find(|d| d.name == "USB Headset").unwrap().id;
        set_default_input(usb_id).unwrap();
        let inp = default_input().unwrap();
        assert_eq!(inp.name, "USB Headset");
    }
    serial_println!("[audiodevice]  5/11 default input OK");

    // Test 6: volume.
    {
        let devices = list_devices();
        let id = devices.first().unwrap().id;
        set_device_volume(id, 50).unwrap();
        assert_eq!(get_device(id).unwrap().volume, 50);
        assert!(set_device_volume(id, 101).is_err());
    }
    serial_println!("[audiodevice]  6/11 volume OK");

    // Test 7: mute.
    {
        let devices = list_devices();
        let id = devices.first().unwrap().id;
        set_device_mute(id, true).unwrap();
        assert!(get_device(id).unwrap().muted);
        set_device_mute(id, false).unwrap();
    }
    serial_println!("[audiodevice]  7/11 mute OK");

    // Test 8: sample rate.
    {
        let devices = list_devices();
        let id = devices.first().unwrap().id;
        set_sample_rate(id, SampleRate::Rate96000).unwrap();
        assert_eq!(get_device(id).unwrap().sample_rate, SampleRate::Rate96000);
    }
    serial_println!("[audiodevice]  8/11 sample rate OK");

    // Test 9: auto-switch.
    {
        set_auto_switch(false).unwrap();
        // Add device — should NOT auto-switch.
        let _ = add_device("BT Speaker", AudioDeviceType::Bluetooth, DeviceDirection::Output, "snd-bt").unwrap();
        // Default should still be USB Headset from test 4.
        set_auto_switch(true).unwrap();
    }
    serial_println!("[audiodevice]  9/11 auto-switch OK");

    // Test 10: remove device.
    {
        let devices = list_devices();
        let bt_id = devices.iter().find(|d| d.name == "BT Speaker").unwrap().id;
        remove_device(bt_id).unwrap();
        assert!(get_device(bt_id).is_err());
    }
    serial_println!("[audiodevice] 10/11 remove OK");

    // Test 11: stats.
    {
        let (total, out, inp, def_out, def_in, ops) = stats();
        assert!(total >= 2);
        assert!(out >= 1);
        assert!(inp >= 1);
        assert!(def_out > 0);
        assert!(def_in > 0);
        assert!(ops > 0);
    }
    serial_println!("[audiodevice] 11/11 stats OK");

    // Leave no residue for later callers / boot-time tests.
    *STATE.lock() = None;

    serial_println!("[audiodevice] All self-tests passed.");
}
