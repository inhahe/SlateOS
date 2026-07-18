//! Clipboard Actions — quick actions on clipboard content.
//!
//! Provides context-sensitive actions based on clipboard content
//! type (URL, email, phone, code, etc.) with customizable handlers.
//!
//! ## Architecture
//!
//! ```text
//! Clipboard paste
//!   → clipaction::detect_type(content) → content type
//!   → clipaction::get_actions(type) → available actions
//!   → clipaction::execute(action_id) → perform action
//!
//! Integration:
//!   → clipboard (local clipboard)
//!   → cliphistory (clipboard history)
//!   → contextmenu (context menu)
//!   → openwith (open with)
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

/// Detected content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Url,
    Email,
    PhoneNumber,
    FilePath,
    Color,
    Code,
    Json,
    PlainText,
    Number,
    Date,
}

impl ContentType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Url => "URL",
            Self::Email => "Email",
            Self::PhoneNumber => "Phone",
            Self::FilePath => "File Path",
            Self::Color => "Color",
            Self::Code => "Code",
            Self::Json => "JSON",
            Self::PlainText => "Text",
            Self::Number => "Number",
            Self::Date => "Date",
        }
    }
}

/// An action that can be performed on content.
#[derive(Debug, Clone)]
pub struct ClipAction {
    pub id: u32,
    pub name: String,
    pub content_type: ContentType,
    pub command: String,
    pub enabled: bool,
    pub use_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ACTIONS: usize = 200;

struct State {
    actions: Vec<ClipAction>,
    next_id: u32,
    total_detections: u64,
    total_executions: u64,
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
        actions: alloc::vec![
            ClipAction { id: 1, name: String::from("Open URL"), content_type: ContentType::Url, command: String::from("browser:open"), enabled: true, use_count: 0 },
            ClipAction { id: 2, name: String::from("Copy URL"), content_type: ContentType::Url, command: String::from("clipboard:copy"), enabled: true, use_count: 0 },
            ClipAction { id: 3, name: String::from("Send Email"), content_type: ContentType::Email, command: String::from("email:compose"), enabled: true, use_count: 0 },
            ClipAction { id: 4, name: String::from("Open File"), content_type: ContentType::FilePath, command: String::from("files:open"), enabled: true, use_count: 0 },
            ClipAction { id: 5, name: String::from("Format JSON"), content_type: ContentType::Json, command: String::from("editor:format_json"), enabled: true, use_count: 0 },
            ClipAction { id: 6, name: String::from("Preview Color"), content_type: ContentType::Color, command: String::from("colorpicker:preview"), enabled: true, use_count: 0 },
        ],
        next_id: 7,
        total_detections: 0,
        total_executions: 0,
        ops: 0,
    });
}

/// Detect content type from text.
pub fn detect_type(content: &str) -> ContentType {
    let trimmed = content.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        ContentType::Url
    } else if trimmed.contains('@') && trimmed.contains('.') && !trimmed.contains(' ') {
        ContentType::Email
    } else if trimmed.starts_with('/') || trimmed.contains(":\\") {
        ContentType::FilePath
    } else if trimmed.starts_with('#') && trimmed.len() <= 9 {
        ContentType::Color
    } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
        ContentType::Json
    } else if trimmed.chars().all(|c| c.is_ascii_digit() || c == '+' || c == '-' || c == '(' || c == ')' || c == ' ') && trimmed.len() >= 7 {
        ContentType::PhoneNumber
    } else if trimmed.parse::<f64>().is_ok() {
        ContentType::Number
    } else {
        ContentType::PlainText
    }
}

/// Get available actions for a content type.
pub fn get_actions(content_type: ContentType) -> Vec<ClipAction> {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        state.ops += 1;
        state.total_detections += 1;
        state.actions.iter()
            .filter(|a| a.content_type == content_type && a.enabled)
            .cloned()
            .collect()
    } else {
        Vec::new()
    }
}

/// Execute an action.
pub fn execute_action(id: u32) -> KernelResult<String> {
    with_state(|state| {
        let action = state.actions.iter_mut().find(|a| a.id == id)
            .ok_or(KernelError::NotFound)?;
        action.use_count += 1;
        state.total_executions += 1;
        Ok(action.command.clone())
    })
}

/// Add a custom action.
pub fn add_action(name: &str, content_type: ContentType, command: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.actions.len() >= MAX_ACTIONS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.actions.push(ClipAction {
            id, name: String::from(name), content_type,
            command: String::from(command), enabled: true, use_count: 0,
        });
        Ok(id)
    })
}

/// Remove an action.
pub fn remove_action(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.actions.len();
        state.actions.retain(|a| a.id != id);
        if state.actions.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List all actions.
pub fn list_actions() -> Vec<ClipAction> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.actions.clone())
}

/// Statistics: (action_count, total_detections, total_executions, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.actions.len(), s.total_detections, s.total_executions, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("clipaction::self_test() — running tests...");
    init_defaults();

    // 1: Default actions.
    assert_eq!(list_actions().len(), 6);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Detect URL.
    assert_eq!(detect_type("https://example.com"), ContentType::Url);
    crate::serial_println!("  [2/8] detect URL: OK");

    // 3: Detect email.
    assert_eq!(detect_type("user@example.com"), ContentType::Email);
    crate::serial_println!("  [3/8] detect email: OK");

    // 4: Detect file path.
    assert_eq!(detect_type("/usr/bin/app"), ContentType::FilePath);
    crate::serial_println!("  [4/8] detect path: OK");

    // 5: Get actions for URL.
    let actions = get_actions(ContentType::Url);
    assert_eq!(actions.len(), 2); // Open URL + Copy URL.
    crate::serial_println!("  [5/8] get actions: OK");

    // 6: Execute action.
    let cmd = execute_action(1).expect("exec"); // Open URL.
    assert_eq!(cmd, "browser:open");
    crate::serial_println!("  [6/8] execute: OK");

    // 7: Add custom action.
    let _aid = add_action("Shorten URL", ContentType::Url, "url:shorten").expect("add");
    let actions = get_actions(ContentType::Url);
    assert_eq!(actions.len(), 3);
    crate::serial_println!("  [7/8] add action: OK");

    // 8: Stats.
    let (actions_count, detections, executions, ops) = stats();
    assert!(actions_count >= 7);
    assert!(detections >= 2);
    assert_eq!(executions, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("clipaction::self_test() — all 8 tests passed");
}
