//! Screen Saver — screen saver management and configuration.
//!
//! Manages screen saver selection, idle timeout detection, preview,
//! and password-on-wake settings.
//!
//! ## Architecture
//!
//! ```text
//! User idle for timeout
//!   → screensaver::activate() → starts saver
//!   → screensaver::deactivate() → returns to desktop
//!
//! Configuration
//!   → screensaver::set_timeout(seconds)
//!   → screensaver::set_saver(name)
//!   → screensaver::set_password_required(bool)
//!
//! Integration:
//!   → screenlock (password-on-wake)
//!   → power (display sleep after saver)
//!   → brightness (dim before saver)
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

/// Built-in screen saver type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaverType {
    Blank,
    Starfield,
    Matrix,
    Bouncing,
    Clock,
    Slideshow,
    Bubbles,
    Custom,
}

impl SaverType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Blank => "Blank Screen",
            Self::Starfield => "Starfield",
            Self::Matrix => "Matrix Rain",
            Self::Bouncing => "Bouncing Logo",
            Self::Clock => "Clock",
            Self::Slideshow => "Photo Slideshow",
            Self::Bubbles => "Bubbles",
            Self::Custom => "Custom",
        }
    }
}

/// Screen saver state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaverState {
    Inactive,
    Active,
    Preview,
}

/// Screen saver configuration.
#[derive(Debug, Clone)]
pub struct SaverConfig {
    pub id: u32,
    pub name: String,
    pub saver_type: SaverType,
    pub timeout_secs: u32,
    pub password_required: bool,
    pub enabled: bool,
    pub state: SaverState,
    pub activated_ns: u64,
    /// Slideshow directory path.
    pub slideshow_dir: String,
    /// Custom saver command.
    pub custom_cmd: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CONFIGS: usize = 8;

struct State {
    configs: Vec<SaverConfig>,
    next_id: u32,
    active_config_id: u32,
    total_activations: u64,
    total_deactivations: u64,
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

    let configs = alloc::vec![
        SaverConfig {
            id: 1, name: String::from("Blank"),
            saver_type: SaverType::Blank,
            timeout_secs: 300, password_required: true,
            enabled: true, state: SaverState::Inactive,
            activated_ns: 0,
            slideshow_dir: String::new(), custom_cmd: String::new(),
        },
        SaverConfig {
            id: 2, name: String::from("Starfield"),
            saver_type: SaverType::Starfield,
            timeout_secs: 600, password_required: true,
            enabled: true, state: SaverState::Inactive,
            activated_ns: 0,
            slideshow_dir: String::new(), custom_cmd: String::new(),
        },
    ];

    *guard = Some(State {
        configs,
        next_id: 3,
        active_config_id: 1,
        total_activations: 0,
        total_deactivations: 0,
        ops: 0,
    });
}

/// Set the active screen saver.
pub fn set_active(config_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.configs.iter().any(|c| c.id == config_id) {
            return Err(KernelError::NotFound);
        }
        state.active_config_id = config_id;
        Ok(())
    })
}

/// Activate the screen saver.
pub fn activate() -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == state.active_config_id)
            .ok_or(KernelError::NotFound)?;
        if !cfg.enabled {
            return Err(KernelError::InvalidArgument);
        }
        cfg.state = SaverState::Active;
        cfg.activated_ns = crate::hpet::elapsed_ns();
        state.total_activations += 1;
        Ok(())
    })
}

/// Deactivate (dismiss) the screen saver.
pub fn deactivate() -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == state.active_config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.state = SaverState::Inactive;
        state.total_deactivations += 1;
        Ok(())
    })
}

/// Preview the screen saver (activates in preview mode).
pub fn preview(config_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.state = SaverState::Preview;
        Ok(())
    })
}

/// Stop previewing.
pub fn stop_preview(config_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.state = SaverState::Inactive;
        Ok(())
    })
}

/// Set idle timeout in seconds.
pub fn set_timeout(config_id: u32, secs: u32) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.timeout_secs = secs.clamp(30, 7200);
        Ok(())
    })
}

/// Set password-on-wake requirement.
pub fn set_password_required(config_id: u32, required: bool) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.password_required = required;
        Ok(())
    })
}

/// Enable/disable a saver.
pub fn set_enabled(config_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.enabled = enabled;
        Ok(())
    })
}

/// Register a custom screen saver.
pub fn register_saver(name: &str, saver_type: SaverType) -> KernelResult<u32> {
    with_state(|state| {
        if state.configs.len() >= MAX_CONFIGS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.configs.push(SaverConfig {
            id, name: String::from(name),
            saver_type,
            timeout_secs: 300, password_required: true,
            enabled: true, state: SaverState::Inactive,
            activated_ns: 0,
            slideshow_dir: String::new(), custom_cmd: String::new(),
        });
        Ok(id)
    })
}

/// List all savers.
pub fn list_savers() -> Vec<SaverConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.configs.clone())
}

/// Get a saver config.
pub fn get_saver(id: u32) -> KernelResult<SaverConfig> {
    with_state(|state| {
        state.configs.iter().find(|c| c.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Get active saver ID.
pub fn active_saver_id() -> u32 {
    STATE.lock().as_ref().map_or(0, |s| s.active_config_id)
}

/// Statistics: (saver_count, total_activations, total_deactivations, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.configs.len(), s.total_activations, s.total_deactivations, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("screensaver::self_test() — running tests...");
    init_defaults();

    // 1: Default savers.
    let savers = list_savers();
    assert_eq!(savers.len(), 2);
    assert_eq!(savers[0].saver_type, SaverType::Blank);
    assert_eq!(savers[1].saver_type, SaverType::Starfield);
    crate::serial_println!("  [1/8] default savers: OK");

    // 2: Activate.
    activate().expect("activate");
    let s = get_saver(1).expect("get");
    assert_eq!(s.state, SaverState::Active);
    crate::serial_println!("  [2/8] activate: OK");

    // 3: Deactivate.
    deactivate().expect("deactivate");
    let s = get_saver(1).expect("get2");
    assert_eq!(s.state, SaverState::Inactive);
    crate::serial_println!("  [3/8] deactivate: OK");

    // 4: Preview.
    preview(2).expect("preview");
    let s = get_saver(2).expect("get3");
    assert_eq!(s.state, SaverState::Preview);
    stop_preview(2).expect("stop");
    crate::serial_println!("  [4/8] preview: OK");

    // 5: Timeout.
    set_timeout(1, 120).expect("timeout");
    let s = get_saver(1).expect("get4");
    assert_eq!(s.timeout_secs, 120);
    crate::serial_println!("  [5/8] timeout: OK");

    // 6: Password setting.
    set_password_required(1, false).expect("pw");
    let s = get_saver(1).expect("get5");
    assert!(!s.password_required);
    crate::serial_println!("  [6/8] password: OK");

    // 7: Register custom.
    let id = register_saver("Matrix Rain", SaverType::Matrix).expect("reg");
    assert_eq!(list_savers().len(), 3);
    set_active(id).expect("set_active");
    assert_eq!(active_saver_id(), id);
    crate::serial_println!("  [7/8] register: OK");

    // 8: Stats.
    let (count, acts, deacts, ops) = stats();
    assert_eq!(count, 3);
    assert_eq!(acts, 1);
    assert_eq!(deacts, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("screensaver::self_test() — all 8 tests passed");
}
