//! OOBE — Out-of-Box Experience / Setup Wizard.
//!
//! Manages the initial setup flow for new installations: language
//! selection, user account creation, privacy settings, network
//! configuration, and theme preferences.
//!
//! ## Architecture
//!
//! ```text
//! First boot detected
//!   → oobe::start() → begins setup wizard
//!   → oobe::advance() → next step
//!   → oobe::complete() → marks setup done
//!
//! Steps:
//!   1. Language/Region
//!   2. Keyboard Layout
//!   3. Network Setup
//!   4. User Account
//!   5. Privacy Settings
//!   6. Theme/Appearance
//!   7. Summary/Complete
//!
//! Integration:
//!   → locale (language/region)
//!   → keylayout (keyboard)
//!   → netsettings (WiFi)
//!   → useracct (account creation)
//!   → theme (appearance)
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

/// Setup wizard step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Welcome,
    Language,
    Keyboard,
    Network,
    UserAccount,
    Privacy,
    Theme,
    Summary,
    Complete,
}

impl SetupStep {
    pub fn label(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Language => "Language & Region",
            Self::Keyboard => "Keyboard Layout",
            Self::Network => "Network Setup",
            Self::UserAccount => "User Account",
            Self::Privacy => "Privacy Settings",
            Self::Theme => "Theme & Appearance",
            Self::Summary => "Summary",
            Self::Complete => "Complete",
        }
    }

    pub fn step_number(self) -> u8 {
        match self {
            Self::Welcome => 0,
            Self::Language => 1,
            Self::Keyboard => 2,
            Self::Network => 3,
            Self::UserAccount => 4,
            Self::Privacy => 5,
            Self::Theme => 6,
            Self::Summary => 7,
            Self::Complete => 8,
        }
    }

    pub fn next(self) -> Option<SetupStep> {
        match self {
            Self::Welcome => Some(Self::Language),
            Self::Language => Some(Self::Keyboard),
            Self::Keyboard => Some(Self::Network),
            Self::Network => Some(Self::UserAccount),
            Self::UserAccount => Some(Self::Privacy),
            Self::Privacy => Some(Self::Theme),
            Self::Theme => Some(Self::Summary),
            Self::Summary => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    pub fn prev(self) -> Option<SetupStep> {
        match self {
            Self::Welcome => None,
            Self::Language => Some(Self::Welcome),
            Self::Keyboard => Some(Self::Language),
            Self::Network => Some(Self::Keyboard),
            Self::UserAccount => Some(Self::Network),
            Self::Privacy => Some(Self::UserAccount),
            Self::Theme => Some(Self::Privacy),
            Self::Summary => Some(Self::Theme),
            Self::Complete => None,
        }
    }
}

/// Setup choices collected during OOBE.
#[derive(Debug, Clone)]
pub struct SetupChoices {
    pub language: String,
    pub region: String,
    pub keyboard_layout: String,
    pub username: String,
    pub hostname: String,
    pub wifi_ssid: String,
    pub send_diagnostics: bool,
    pub location_services: bool,
    pub auto_updates: bool,
    pub theme: String,
}

impl SetupChoices {
    fn new() -> Self {
        Self {
            language: String::from("en-US"),
            region: String::from("US"),
            keyboard_layout: String::from("us"),
            username: String::new(),
            hostname: String::from("mintos-pc"),
            wifi_ssid: String::new(),
            send_diagnostics: false,
            location_services: false,
            auto_updates: true,
            theme: String::from("Default"),
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    current_step: SetupStep,
    choices: SetupChoices,
    completed: bool,
    skipped_steps: Vec<SetupStep>,
    started_ns: u64,
    completed_ns: u64,
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
        current_step: SetupStep::Welcome,
        choices: SetupChoices::new(),
        completed: false,
        skipped_steps: Vec::new(),
        started_ns: crate::hpet::elapsed_ns(),
        completed_ns: 0,
        ops: 0,
    });
}

/// Get current step.
pub fn current_step() -> SetupStep {
    STATE.lock().as_ref().map_or(SetupStep::Complete, |s| s.current_step)
}

/// Advance to next step.
pub fn advance() -> KernelResult<SetupStep> {
    with_state(|state| {
        if state.completed {
            return Err(KernelError::InvalidArgument);
        }
        match state.current_step.next() {
            Some(next) => {
                state.current_step = next;
                if next == SetupStep::Complete {
                    state.completed = true;
                    state.completed_ns = crate::hpet::elapsed_ns();
                }
                Ok(next)
            }
            None => Err(KernelError::InvalidArgument),
        }
    })
}

/// Go back to previous step.
pub fn go_back() -> KernelResult<SetupStep> {
    with_state(|state| {
        match state.current_step.prev() {
            Some(prev) => {
                state.current_step = prev;
                Ok(prev)
            }
            None => Err(KernelError::InvalidArgument),
        }
    })
}

/// Skip current step.
pub fn skip() -> KernelResult<SetupStep> {
    with_state(|state| {
        let skipped = state.current_step;
        state.skipped_steps.push(skipped);
        match state.current_step.next() {
            Some(next) => {
                state.current_step = next;
                Ok(next)
            }
            None => Err(KernelError::InvalidArgument),
        }
    })
}

/// Set language choice.
pub fn set_language(language: &str, region: &str) -> KernelResult<()> {
    with_state(|state| {
        state.choices.language = String::from(language);
        state.choices.region = String::from(region);
        Ok(())
    })
}

/// Set keyboard layout.
pub fn set_keyboard(layout: &str) -> KernelResult<()> {
    with_state(|state| {
        state.choices.keyboard_layout = String::from(layout);
        Ok(())
    })
}

/// Set user account info.
pub fn set_user(username: &str, hostname: &str) -> KernelResult<()> {
    with_state(|state| {
        state.choices.username = String::from(username);
        if !hostname.is_empty() {
            state.choices.hostname = String::from(hostname);
        }
        Ok(())
    })
}

/// Set network (WiFi SSID).
pub fn set_network(ssid: &str) -> KernelResult<()> {
    with_state(|state| {
        state.choices.wifi_ssid = String::from(ssid);
        Ok(())
    })
}

/// Set privacy choices.
pub fn set_privacy(diagnostics: bool, location: bool, auto_updates: bool) -> KernelResult<()> {
    with_state(|state| {
        state.choices.send_diagnostics = diagnostics;
        state.choices.location_services = location;
        state.choices.auto_updates = auto_updates;
        Ok(())
    })
}

/// Set theme choice.
pub fn set_theme(theme: &str) -> KernelResult<()> {
    with_state(|state| {
        state.choices.theme = String::from(theme);
        Ok(())
    })
}

/// Get all choices.
pub fn get_choices() -> Option<SetupChoices> {
    STATE.lock().as_ref().map(|s| s.choices.clone())
}

/// Check if OOBE is completed.
pub fn is_completed() -> bool {
    STATE.lock().as_ref().is_none_or(|s| s.completed)
}

/// Force complete (skip remaining).
pub fn force_complete() -> KernelResult<()> {
    with_state(|state| {
        state.current_step = SetupStep::Complete;
        state.completed = true;
        state.completed_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Get skipped steps.
pub fn skipped_steps() -> Vec<SetupStep> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.skipped_steps.clone())
}

/// Statistics: (current_step_number, completed, skipped_count, ops).
pub fn stats() -> (u8, bool, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.current_step.step_number(), s.completed, s.skipped_steps.len(), s.ops),
        None => (8, true, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("oobe::self_test() — running tests...");
    init_defaults();

    // 1: Starts at Welcome.
    assert_eq!(current_step(), SetupStep::Welcome);
    assert!(!is_completed());
    crate::serial_println!("  [1/8] initial state: OK");

    // 2: Advance through steps.
    let next = advance().expect("advance");
    assert_eq!(next, SetupStep::Language);
    crate::serial_println!("  [2/8] advance: OK");

    // 3: Set language.
    set_language("de-DE", "DE").expect("lang");
    let choices = get_choices().expect("choices");
    assert_eq!(choices.language, "de-DE");
    crate::serial_println!("  [3/8] set language: OK");

    // 4: Skip keyboard step.
    advance().ok(); // to Keyboard
    let next = skip().expect("skip");
    assert_eq!(next, SetupStep::Network);
    assert_eq!(skipped_steps().len(), 1);
    crate::serial_println!("  [4/8] skip step: OK");

    // 5: Go back.
    let prev = go_back().expect("back");
    assert_eq!(prev, SetupStep::Keyboard);
    crate::serial_println!("  [5/8] go back: OK");

    // 6: Set user.
    advance().ok(); // Network
    advance().ok(); // UserAccount
    set_user("admin", "my-pc").expect("user");
    let choices = get_choices().expect("choices2");
    assert_eq!(choices.username, "admin");
    assert_eq!(choices.hostname, "my-pc");
    crate::serial_println!("  [6/8] set user: OK");

    // 7: Force complete.
    force_complete().expect("force");
    assert!(is_completed());
    assert_eq!(current_step(), SetupStep::Complete);
    crate::serial_println!("  [7/8] force complete: OK");

    // 8: Stats.
    let (step, completed, skipped, ops) = stats();
    assert_eq!(step, 8); // Complete
    assert!(completed);
    assert!(skipped >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("oobe::self_test() — all 8 tests passed");
}
