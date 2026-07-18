//! Webcam — camera device management and privacy controls.
//!
//! Tracks connected camera devices, manages per-app access permissions,
//! provides a privacy indicator (camera-in-use LED), and supports
//! capture resolution/framerate configuration.
//!
//! ## Architecture
//!
//! ```text
//! Camera hardware detected
//!   → webcam::register_camera(name, resolutions)
//!
//! App requests camera access
//!   → webcam::open_stream(camera_id, app_pid, resolution)
//!     → checks privacy settings (blocked apps)
//!     → returns stream ID
//!
//! Privacy indicator
//!   → webcam::cameras_in_use() → list of active cameras
//!   → system tray shows camera icon when any camera active
//!
//! Integration:
//!   → appsandbox (camera permission checks)
//!   → notifcenter (camera access notifications)
//!   → systray (camera-in-use indicator)
//!   → devicemgr (camera hotplug events)
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

/// Camera connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraConnection {
    Usb,
    Builtin,
    Network,
    Virtual,
}

impl CameraConnection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Builtin => "Built-in",
            Self::Network => "Network",
            Self::Virtual => "Virtual",
        }
    }
}

/// Camera resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub max_fps: u32,
}

/// Privacy setting for a camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacySetting {
    /// Camera available to all apps.
    Open,
    /// Camera requires per-app approval.
    PromptRequired,
    /// Camera hardware disabled (privacy shutter).
    Disabled,
}

impl PrivacySetting {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::PromptRequired => "Prompt Required",
            Self::Disabled => "Disabled",
        }
    }
}

/// A registered camera device.
#[derive(Debug, Clone)]
pub struct Camera {
    pub id: u32,
    pub name: String,
    pub connection: CameraConnection,
    pub resolutions: Vec<Resolution>,
    pub privacy: PrivacySetting,
    pub is_default: bool,
    pub registered_ns: u64,
}

/// An active camera stream.
#[derive(Debug, Clone)]
pub struct CameraStream {
    pub stream_id: u32,
    pub camera_id: u32,
    pub app_name: String,
    pub app_pid: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub started_ns: u64,
}

/// A blocked app entry.
#[derive(Debug, Clone)]
pub struct BlockedApp {
    pub app_name: String,
    pub blocked_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CAMERAS: usize = 16;
const MAX_STREAMS: usize = 64;
const MAX_BLOCKED: usize = 100;

struct State {
    cameras: Vec<Camera>,
    streams: Vec<CameraStream>,
    blocked_apps: Vec<BlockedApp>,
    next_camera_id: u32,
    next_stream_id: u32,
    default_camera_id: u32,
    total_streams: u64,
    total_denied: u64,
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
    // Start with NO cameras. A webcam is enumerated hardware, not a configurable
    // default. Seeding a phantom "Integrated Webcam" (Builtin, 640/720/1080p)
    // would surface a fabricated capture device through /proc/webcam and the
    // `webcam` shell command as if a real camera had been detected — and a
    // desktop may have no camera at all. Worse, a fabricated device with a
    // PromptRequired privacy setting implies a capture surface that does not
    // exist. Real cameras appear only when a UVC/USB or platform driver calls
    // register_camera() on hotplug/enumeration.
    //
    // DEFERRED PROPER FIX: wire register_camera()/unregister_camera() to a real
    // UVC webcam driver once one exists; until then this stays empty so
    // /proc/webcam reports "camera_count: 0" rather than inventing a device.
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    *guard = Some(State {
        cameras: Vec::new(),
        streams: Vec::new(),
        blocked_apps: Vec::new(),
        next_camera_id: 1,
        next_stream_id: 1,
        default_camera_id: 0,
        total_streams: 0,
        total_denied: 0,
        ops: 0,
    });
}

/// Register a new camera device.
pub fn register_camera(name: &str, connection: CameraConnection) -> KernelResult<u32> {
    with_state(|state| {
        if state.cameras.len() >= MAX_CAMERAS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_camera_id;
        state.next_camera_id += 1;

        let is_first = state.cameras.is_empty();
        state.cameras.push(Camera {
            id,
            name: String::from(name),
            connection,
            resolutions: alloc::vec![
                Resolution { width: 640, height: 480, max_fps: 30 },
                Resolution { width: 1280, height: 720, max_fps: 30 },
            ],
            privacy: PrivacySetting::PromptRequired,
            is_default: is_first,
            registered_ns: crate::hpet::elapsed_ns(),
        });
        if is_first { state.default_camera_id = id; }
        Ok(id)
    })
}

/// Unregister a camera device (also closes all its streams).
pub fn unregister_camera(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.cameras.iter().position(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        state.cameras.remove(pos);
        // Close all streams on this camera.
        state.streams.retain(|s| s.camera_id != id);
        // Update default if needed.
        if state.default_camera_id == id {
            state.default_camera_id = state.cameras.first().map(|c| c.id).unwrap_or(0);
            if let Some(c) = state.cameras.first_mut() {
                c.is_default = true;
            }
        }
        Ok(())
    })
}

/// Open a camera stream for an app.
pub fn open_stream(camera_id: u32, app_name: &str, app_pid: u32, width: u32, height: u32, fps: u32) -> KernelResult<u32> {
    with_state(|state| {
        // Check blocked apps.
        if state.blocked_apps.iter().any(|b| b.app_name == app_name) {
            state.total_denied += 1;
            return Err(KernelError::PermissionDenied);
        }

        let camera = state.cameras.iter().find(|c| c.id == camera_id)
            .ok_or(KernelError::NotFound)?;

        // Check privacy setting.
        if camera.privacy == PrivacySetting::Disabled {
            state.total_denied += 1;
            return Err(KernelError::PermissionDenied);
        }

        if state.streams.len() >= MAX_STREAMS {
            return Err(KernelError::ResourceExhausted);
        }

        let sid = state.next_stream_id;
        state.next_stream_id += 1;
        state.total_streams += 1;

        state.streams.push(CameraStream {
            stream_id: sid,
            camera_id,
            app_name: String::from(app_name),
            app_pid,
            width,
            height,
            fps,
            started_ns: crate::hpet::elapsed_ns(),
        });
        Ok(sid)
    })
}

/// Close a camera stream.
pub fn close_stream(stream_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.streams.iter().position(|s| s.stream_id == stream_id)
            .ok_or(KernelError::NotFound)?;
        state.streams.remove(pos);
        Ok(())
    })
}

/// Set camera privacy setting.
pub fn set_privacy(camera_id: u32, privacy: PrivacySetting) -> KernelResult<()> {
    with_state(|state| {
        let camera = state.cameras.iter_mut().find(|c| c.id == camera_id)
            .ok_or(KernelError::NotFound)?;
        camera.privacy = privacy;
        // If disabling, close all streams.
        if privacy == PrivacySetting::Disabled {
            state.streams.retain(|s| s.camera_id != camera_id);
        }
        Ok(())
    })
}

/// Block an app from accessing any camera.
pub fn block_app(app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.blocked_apps.iter().any(|b| b.app_name == app_name) {
            return Err(KernelError::AlreadyExists);
        }
        if state.blocked_apps.len() >= MAX_BLOCKED {
            return Err(KernelError::ResourceExhausted);
        }
        state.blocked_apps.push(BlockedApp {
            app_name: String::from(app_name),
            blocked_ns: crate::hpet::elapsed_ns(),
        });
        // Close any existing streams from this app.
        state.streams.retain(|s| s.app_name != app_name);
        Ok(())
    })
}

/// Unblock an app.
pub fn unblock_app(app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.blocked_apps.iter().position(|b| b.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        state.blocked_apps.remove(pos);
        Ok(())
    })
}

/// Set default camera.
pub fn set_default(camera_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.cameras.iter().any(|c| c.id == camera_id) {
            return Err(KernelError::NotFound);
        }
        for c in state.cameras.iter_mut() {
            c.is_default = c.id == camera_id;
        }
        state.default_camera_id = camera_id;
        Ok(())
    })
}

/// List all cameras.
pub fn list_cameras() -> Vec<Camera> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cameras.clone())
}

/// List all active streams.
pub fn list_streams() -> Vec<CameraStream> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.streams.clone())
}

/// List blocked apps.
pub fn list_blocked() -> Vec<BlockedApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.blocked_apps.clone())
}

/// Get cameras currently in use (have active streams).
pub fn cameras_in_use() -> Vec<u32> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut ids: Vec<u32> = s.streams.iter().map(|st| st.camera_id).collect();
        ids.sort();
        ids.dedup();
        ids
    })
}

/// Statistics: (camera_count, stream_count, total_streams, total_denied, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cameras.len(), s.streams.len(), s.total_streams, s.total_denied, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("webcam::self_test() — running tests...");

    // Residue-free: start from a clean, controlled State so assertions hold
    // regardless of prior kshell/procfs activity (init_defaults early-returns
    // when STATE is already populated), and build every camera through the real
    // register_camera() hotplug API rather than a seeded phantom device.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no cameras until a driver enumerates one.
    assert_eq!(list_cameras().len(), 0);
    crate::serial_println!("  [1/10] empty defaults: OK");

    // Build a fixture camera the way a UVC driver would on hotplug; the first
    // registered camera becomes the default.
    let cam_id = register_camera("Integrated Webcam", CameraConnection::Builtin).expect("register first");
    assert!(list_cameras()[0].is_default);

    // 2: Register another camera.
    let cam2 = register_camera("USB Webcam", CameraConnection::Usb).expect("register");
    assert_eq!(list_cameras().len(), 2);
    crate::serial_println!("  [2/10] register camera: OK");

    // 3: Open stream.
    let sid = open_stream(cam_id, "video_call", 100, 1280, 720, 30).expect("open");
    assert!(sid > 0);
    assert_eq!(list_streams().len(), 1);
    crate::serial_println!("  [3/10] open stream: OK");

    // 4: Camera in use.
    let in_use = cameras_in_use();
    assert!(in_use.contains(&cam_id));
    crate::serial_println!("  [4/10] cameras in use: OK");

    // 5: Block app.
    block_app("malware_app").expect("block");
    let blocked_result = open_stream(cam_id, "malware_app", 999, 640, 480, 30);
    assert!(blocked_result.is_err());
    crate::serial_println!("  [5/10] block app: OK");

    // 6: Unblock app.
    unblock_app("malware_app").expect("unblock");
    let sid2 = open_stream(cam_id, "malware_app", 999, 640, 480, 30).expect("now allowed");
    assert!(sid2 > 0);
    crate::serial_println!("  [6/10] unblock app: OK");

    // 7: Close stream.
    close_stream(sid).expect("close");
    assert_eq!(list_streams().len(), 1); // sid2 still open
    crate::serial_println!("  [7/10] close stream: OK");

    // 8: Privacy disable closes streams.
    set_privacy(cam_id, PrivacySetting::Disabled).expect("disable");
    let open_result = open_stream(cam_id, "test", 101, 640, 480, 30);
    assert!(open_result.is_err());
    crate::serial_println!("  [8/10] privacy disable: OK");

    // 9: Set default camera.
    set_default(cam2).expect("default");
    let cams = list_cameras();
    let c2 = cams.iter().find(|c| c.id == cam2).expect("find");
    assert!(c2.is_default);
    crate::serial_println!("  [9/10] set default: OK");

    // 10: Unregister camera.
    unregister_camera(cam2).expect("unreg");
    assert_eq!(list_cameras().len(), 1);
    crate::serial_println!("  [10/10] unregister: OK");

    let (cam_count, _, total, denied, ops) = stats();
    assert_eq!(cam_count, 1);
    assert!(total >= 3);
    assert!(denied >= 1);
    assert!(ops > 0);

    // Leave no residue for later callers / the live /proc/webcam view: the test
    // registered cameras and opened streams, none of which represents real
    // hardware. Reset to None so production reads report camera_count: 0.
    *STATE.lock() = None;

    crate::serial_println!("webcam::self_test() — all 10 tests passed");
}
