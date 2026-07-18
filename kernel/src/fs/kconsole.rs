//! Kernel Console — virtual console management.
//!
//! Manages kernel virtual consoles (VTs), tracks active console,
//! console dimensions, scrollback, and input/output routing.
//! Supports multiple consoles for serial, VGA, and framebuffer.
//!
//! ## Architecture
//!
//! ```text
//! Console management
//!   → kconsole::switch(n) → switch active console
//!   → kconsole::write(n, data) → write to console
//!   → kconsole::list() → list consoles
//!   → kconsole::resize(n, cols, rows) → resize console
//!
//! Integration:
//!   → kernlog (kernel log)
//!   → kshell (kernel shell)
//!   → display (display settings)
//!   → serial (serial port)
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

/// Console type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleType {
    Serial,
    Vga,
    Framebuffer,
    Virtual,
    Network,
}

impl ConsoleType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::Vga => "vga",
            Self::Framebuffer => "fb",
            Self::Virtual => "virtual",
            Self::Network => "network",
        }
    }
}

/// A virtual console.
#[derive(Debug, Clone)]
pub struct Console {
    pub id: u32,
    pub name: String,
    pub console_type: ConsoleType,
    pub cols: u32,
    pub rows: u32,
    pub active: bool,
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub scrollback_lines: u32,
    pub max_scrollback: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CONSOLES: usize = 16;

struct State {
    consoles: Vec<Console>,
    active_id: u32,
    next_id: u32,
    total_switches: u64,
    total_writes: u64,
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
    *guard = Some(State {
        consoles: alloc::vec![
            Console { id: 1, name: String::from("ttyS0"), console_type: ConsoleType::Serial, cols: 80, rows: 25, active: true, bytes_written: 4096, bytes_read: 512, scrollback_lines: 100, max_scrollback: 1000 },
            Console { id: 2, name: String::from("tty1"), console_type: ConsoleType::Framebuffer, cols: 120, rows: 40, active: false, bytes_written: 0, bytes_read: 0, scrollback_lines: 0, max_scrollback: 5000 },
            Console { id: 3, name: String::from("tty2"), console_type: ConsoleType::Framebuffer, cols: 120, rows: 40, active: false, bytes_written: 0, bytes_read: 0, scrollback_lines: 0, max_scrollback: 5000 },
        ],
        active_id: 1,
        next_id: 4,
        total_switches: 0,
        total_writes: 1,
        ops: 0,
    });
}

/// Switch active console.
pub fn switch(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.consoles.iter().any(|c| c.id == id) { return Err(KernelError::NotFound); }
        for c in &mut state.consoles { c.active = c.id == id; }
        state.active_id = id;
        state.total_switches += 1;
        Ok(())
    })
}

/// Write data to console.
pub fn write(id: u32, data_len: u64) -> KernelResult<()> {
    with_state(|state| {
        let c = state.consoles.iter_mut().find(|c| c.id == id).ok_or(KernelError::NotFound)?;
        c.bytes_written += data_len;
        state.total_writes += 1;
        Ok(())
    })
}

/// Create a new console.
pub fn create(name: &str, console_type: ConsoleType, cols: u32, rows: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.consoles.len() >= MAX_CONSOLES { return Err(KernelError::ResourceExhausted); }
        if state.consoles.iter().any(|c| c.name == name) { return Err(KernelError::AlreadyExists); }
        let id = state.next_id;
        state.next_id += 1;
        state.consoles.push(Console {
            id, name: String::from(name), console_type, cols, rows,
            active: false, bytes_written: 0, bytes_read: 0,
            scrollback_lines: 0, max_scrollback: 5000,
        });
        Ok(id)
    })
}

/// Resize a console.
pub fn resize(id: u32, cols: u32, rows: u32) -> KernelResult<()> {
    with_state(|state| {
        let c = state.consoles.iter_mut().find(|c| c.id == id).ok_or(KernelError::NotFound)?;
        c.cols = cols;
        c.rows = rows;
        Ok(())
    })
}

/// Get active console.
pub fn active() -> Option<Console> {
    STATE.lock().as_ref().and_then(|s| s.consoles.iter().find(|c| c.active).cloned())
}

/// List consoles.
pub fn list() -> Vec<Console> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.consoles.clone())
}

/// Get console by ID.
pub fn get(id: u32) -> Option<Console> {
    STATE.lock().as_ref().and_then(|s| s.consoles.iter().find(|c| c.id == id).cloned())
}

/// Statistics: (console_count, active_id, total_switches, total_writes, ops).
pub fn stats() -> (usize, u32, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.consoles.len(), s.active_id, s.total_switches, s.total_writes, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("kconsole::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Active.
    let a = active().expect("active");
    assert_eq!(a.id, 1);
    assert_eq!(a.name, "ttyS0");
    crate::serial_println!("  [2/8] active: OK");

    // 3: Switch.
    switch(2).expect("switch");
    let a = active().expect("active2");
    assert_eq!(a.id, 2);
    crate::serial_println!("  [3/8] switch: OK");

    // 4: Write.
    write(2, 1024).expect("write");
    let c = get(2).expect("get");
    assert_eq!(c.bytes_written, 1024);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Create.
    let id = create("tty3", ConsoleType::Virtual, 80, 24).expect("create");
    assert!(id >= 4);
    assert!(create("tty3", ConsoleType::Virtual, 80, 24).is_err());
    crate::serial_println!("  [5/8] create: OK");

    // 6: Resize.
    resize(id, 132, 50).expect("resize");
    let c = get(id).expect("get2");
    assert_eq!(c.cols, 132);
    assert_eq!(c.rows, 50);
    crate::serial_println!("  [6/8] resize: OK");

    // 7: Switch error.
    assert!(switch(999).is_err());
    crate::serial_println!("  [7/8] switch error: OK");

    // 8: Stats.
    let (count, active_id, switches, writes, ops) = stats();
    assert_eq!(count, 4);
    assert_eq!(active_id, 2);
    assert!(switches >= 1);
    assert!(writes >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kconsole::self_test() — all 8 tests passed");
}
