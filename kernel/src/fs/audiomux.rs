//! Audio multiplexer — per-application audio routing.
//!
//! Routes audio streams from applications to specific output devices,
//! supports per-app volume, audio device grouping, and output
//! destination selection (speakers vs headphones vs HDMI).
//!
//! ## Architecture
//!
//! ```text
//! Application plays audio
//!   → audiomux::create_stream(app_id, output_id) → routed stream
//!
//! Settings panel → Sound → App Volume
//!   → audiomux::list_streams() → active audio streams
//!   → audiomux::set_volume(stream_id, volume) → per-app volume
//!
//! Device change (headphones plugged in)
//!   → audiomux::set_default_output(device_id) → reroute streams
//!
//! Integration:
//!   → soundmixer (master volume / global mix)
//!   → audiodevice (device enumeration)
//!   → notifcenter (device change notifications)
//!   → focusassist (mute non-focused apps)
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

/// Audio endpoint type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointType {
    Speakers,
    Headphones,
    Hdmi,
    Bluetooth,
    Usb,
    LineOut,
    Monitor,
}

impl EndpointType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Speakers => "Speakers",
            Self::Headphones => "Headphones",
            Self::Hdmi => "HDMI",
            Self::Bluetooth => "Bluetooth",
            Self::Usb => "USB Audio",
            Self::LineOut => "Line Out",
            Self::Monitor => "Monitor",
        }
    }
}

/// Stream state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Playing,
    Paused,
    Stopped,
    Muted,
}

impl StreamState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
            Self::Muted => "Muted",
        }
    }
}

/// An audio output device.
#[derive(Debug, Clone)]
pub struct AudioOutput {
    /// Device ID.
    pub id: u32,
    /// Name.
    pub name: String,
    /// Endpoint type.
    pub endpoint: EndpointType,
    /// Volume (0-100).
    pub volume: u32,
    /// Whether this is the default output.
    pub is_default: bool,
    /// Whether muted.
    pub muted: bool,
    /// Active stream count.
    pub active_streams: u32,
}

/// An audio stream from an application.
#[derive(Debug, Clone)]
pub struct AudioStream {
    /// Stream ID.
    pub id: u32,
    /// Application name.
    pub app_name: String,
    /// PID of the application.
    pub app_pid: u32,
    /// Output device ID.
    pub output_id: u32,
    /// Per-stream volume (0-100).
    pub volume: u32,
    /// State.
    pub state: StreamState,
    /// Whether muted.
    pub muted: bool,
    /// Created timestamp (ns).
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_OUTPUTS: usize = 20;
const MAX_STREAMS: usize = 100;

struct State {
    outputs: Vec<AudioOutput>,
    streams: Vec<AudioStream>,
    next_output_id: u32,
    next_stream_id: u32,
    default_output: u32,
    total_streams_created: u64,
    total_reroutes: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let outputs = alloc::vec![
        AudioOutput {
            id: 1, name: String::from("Built-in Speakers"),
            endpoint: EndpointType::Speakers, volume: 80,
            is_default: true, muted: false, active_streams: 0,
        },
        AudioOutput {
            id: 2, name: String::from("HDMI Audio"),
            endpoint: EndpointType::Hdmi, volume: 100,
            is_default: false, muted: false, active_streams: 0,
        },
    ];
    *guard = Some(State {
        outputs, streams: Vec::new(),
        next_output_id: 3, next_stream_id: 1,
        default_output: 1,
        total_streams_created: 0,
        total_reroutes: 0,
        ops: 0,
    });
}

/// Add an audio output device.
pub fn add_output(name: &str, endpoint: EndpointType) -> KernelResult<u32> {
    with_state(|state| {
        if state.outputs.len() >= MAX_OUTPUTS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_output_id;
        state.next_output_id += 1;
        state.outputs.push(AudioOutput {
            id, name: String::from(name), endpoint,
            volume: 100, is_default: false, muted: false, active_streams: 0,
        });
        Ok(id)
    })
}

/// Remove an output device.
pub fn remove_output(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.outputs.iter().position(|o| o.id == id)
            .ok_or(KernelError::NotFound)?;
        // Reroute any streams on this output to default.
        for stream in state.streams.iter_mut() {
            if stream.output_id == id {
                stream.output_id = state.default_output;
                state.total_reroutes += 1;
            }
        }
        state.outputs.remove(pos);
        Ok(())
    })
}

/// Set default output device.
pub fn set_default_output(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.outputs.iter().any(|o| o.id == id) {
            return Err(KernelError::NotFound);
        }
        for o in state.outputs.iter_mut() {
            o.is_default = o.id == id;
        }
        state.default_output = id;
        Ok(())
    })
}

/// Create an audio stream for an application.
pub fn create_stream(app_name: &str, app_pid: u32, output_id: Option<u32>) -> KernelResult<u32> {
    with_state(|state| {
        if state.streams.len() >= MAX_STREAMS {
            return Err(KernelError::ResourceExhausted);
        }
        let out_id = output_id.unwrap_or(state.default_output);
        if !state.outputs.iter().any(|o| o.id == out_id) {
            return Err(KernelError::NotFound);
        }

        let id = state.next_stream_id;
        state.next_stream_id += 1;
        state.total_streams_created += 1;

        state.streams.push(AudioStream {
            id, app_name: String::from(app_name), app_pid,
            output_id: out_id, volume: 100,
            state: StreamState::Playing, muted: false,
            created_ns: crate::hpet::elapsed_ns(),
        });

        // Update active stream count.
        if let Some(out) = state.outputs.iter_mut().find(|o| o.id == out_id) {
            out.active_streams += 1;
        }
        Ok(id)
    })
}

/// Destroy a stream.
pub fn destroy_stream(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.streams.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        let out_id = state.streams[pos].output_id;
        state.streams.remove(pos);

        // Update active stream count.
        if let Some(out) = state.outputs.iter_mut().find(|o| o.id == out_id) {
            out.active_streams = out.active_streams.saturating_sub(1);
        }
        Ok(())
    })
}

/// Set stream volume (0-100).
pub fn set_stream_volume(id: u32, volume: u32) -> KernelResult<()> {
    with_state(|state| {
        let stream = state.streams.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        stream.volume = volume.min(100);
        Ok(())
    })
}

/// Mute/unmute a stream.
pub fn set_stream_muted(id: u32, muted: bool) -> KernelResult<()> {
    with_state(|state| {
        let stream = state.streams.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        stream.muted = muted;
        stream.state = if muted { StreamState::Muted } else { StreamState::Playing };
        Ok(())
    })
}

/// Reroute a stream to a different output.
pub fn reroute_stream(stream_id: u32, new_output_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.outputs.iter().any(|o| o.id == new_output_id) {
            return Err(KernelError::NotFound);
        }
        let stream = state.streams.iter_mut().find(|s| s.id == stream_id)
            .ok_or(KernelError::NotFound)?;
        let old_out = stream.output_id;
        stream.output_id = new_output_id;
        state.total_reroutes += 1;

        // Update counts.
        if let Some(out) = state.outputs.iter_mut().find(|o| o.id == old_out) {
            out.active_streams = out.active_streams.saturating_sub(1);
        }
        if let Some(out) = state.outputs.iter_mut().find(|o| o.id == new_output_id) {
            out.active_streams += 1;
        }
        Ok(())
    })
}

/// Set output device volume.
pub fn set_output_volume(id: u32, volume: u32) -> KernelResult<()> {
    with_state(|state| {
        let out = state.outputs.iter_mut().find(|o| o.id == id)
            .ok_or(KernelError::NotFound)?;
        out.volume = volume.min(100);
        Ok(())
    })
}

/// List all outputs.
pub fn list_outputs() -> Vec<AudioOutput> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.outputs.clone())
}

/// List all streams.
pub fn list_streams() -> Vec<AudioStream> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.streams.clone())
}

/// Statistics: (output_count, stream_count, total_created, total_reroutes, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.outputs.len(), s.streams.len(), s.total_streams_created, s.total_reroutes, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("audiomux::self_test() — running tests...");
    init_defaults();

    // 1: Default outputs.
    let outputs = list_outputs();
    assert_eq!(outputs.len(), 2);
    crate::serial_println!("  [1/11] default outputs: OK");

    // 2: Create stream.
    let sid = create_stream("music_player", 100, None).expect("create");
    assert!(sid > 0);
    crate::serial_println!("  [2/11] create stream: OK");

    // 3: Stream on default output.
    let streams = list_streams();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].output_id, 1); // Default output.
    crate::serial_println!("  [3/11] default routing: OK");

    // 4: Set stream volume.
    set_stream_volume(sid, 50).expect("vol");
    let streams = list_streams();
    assert_eq!(streams[0].volume, 50);
    crate::serial_println!("  [4/11] stream volume: OK");

    // 5: Mute stream.
    set_stream_muted(sid, true).expect("mute");
    let streams = list_streams();
    assert!(streams[0].muted);
    assert_eq!(streams[0].state, StreamState::Muted);
    crate::serial_println!("  [5/11] mute stream: OK");

    // 6: Reroute stream.
    reroute_stream(sid, 2).expect("reroute");
    let streams = list_streams();
    assert_eq!(streams[0].output_id, 2);
    crate::serial_println!("  [6/11] reroute stream: OK");

    // 7: Add output.
    let oid = add_output("Bluetooth Headphones", EndpointType::Bluetooth).expect("add out");
    assert_eq!(list_outputs().len(), 3);
    crate::serial_println!("  [7/11] add output: OK");

    // 8: Set default.
    set_default_output(oid).expect("set default");
    let outputs = list_outputs();
    let def = outputs.iter().find(|o| o.is_default).expect("find default");
    assert_eq!(def.id, oid);
    crate::serial_println!("  [8/11] set default: OK");

    // 9: Destroy stream.
    destroy_stream(sid).expect("destroy");
    assert!(list_streams().is_empty());
    crate::serial_println!("  [9/11] destroy stream: OK");

    // 10: Remove output.
    remove_output(oid).expect("remove");
    assert_eq!(list_outputs().len(), 2);
    crate::serial_println!("  [10/11] remove output: OK");

    // 11: Stats.
    let (outs, streams, created, reroutes, ops) = stats();
    assert_eq!(outs, 2);
    assert_eq!(streams, 0);
    assert_eq!(created, 1);
    assert!(reroutes >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("audiomux::self_test() — all 11 tests passed");
}
