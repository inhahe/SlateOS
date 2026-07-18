//! Face Unlock — facial recognition authentication.
//!
//! Provides facial recognition enrollment, verification, and
//! authentication as an alternative unlock method.
//!
//! ## Architecture
//!
//! ```text
//! User enrollment
//!   → faceunlock::enroll(user) → capture face data
//!   → faceunlock::verify() → authenticate user
//!
//! Security
//!   → faceunlock::set_liveness(enabled) → anti-spoofing
//!   → faceunlock::set_threshold(level) → match strictness
//!
//! Integration:
//!   → screenlock (lock screen)
//!   → webcam (camera access)
//!   → useracct (user accounts)
//!   → credentials (credential store)
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

/// Security level for face matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Faster but less strict.
    Low,
    /// Default balance.
    Standard,
    /// Stricter matching.
    High,
    /// Maximum security (may have more false rejections).
    Maximum,
}

impl SecurityLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Standard => "Standard",
            Self::High => "High",
            Self::Maximum => "Maximum",
        }
    }
}

/// Verification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    Matched,
    NoMatch,
    LivenessCheckFailed,
    NoEnrollment,
    CameraError,
    Timeout,
}

impl VerifyResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Matched => "Matched",
            Self::NoMatch => "No Match",
            Self::LivenessCheckFailed => "Liveness Failed",
            Self::NoEnrollment => "No Enrollment",
            Self::CameraError => "Camera Error",
            Self::Timeout => "Timeout",
        }
    }
}

/// Enrolled user face data.
#[derive(Debug, Clone)]
pub struct Enrollment {
    pub user_id: u32,
    pub user_name: String,
    pub enrolled_ns: u64,
    pub last_verified_ns: u64,
    pub verify_count: u64,
    pub fail_count: u64,
    pub model_quality: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ENROLLMENTS: usize = 10;

struct State {
    enrollments: Vec<Enrollment>,
    security_level: SecurityLevel,
    liveness_detection: bool,
    require_attention: bool,
    auto_dismiss: bool,
    enabled: bool,
    total_verifications: u64,
    total_matches: u64,
    total_rejections: u64,
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
        enrollments: Vec::new(),
        security_level: SecurityLevel::Standard,
        liveness_detection: true,
        require_attention: true,
        auto_dismiss: true,
        enabled: false,
        total_verifications: 0,
        total_matches: 0,
        total_rejections: 0,
        ops: 0,
    });
}

/// Enable/disable face unlock.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        Ok(())
    })
}

/// Set security level.
pub fn set_security(level: SecurityLevel) -> KernelResult<()> {
    with_state(|state| {
        state.security_level = level;
        Ok(())
    })
}

/// Enable/disable liveness detection.
pub fn set_liveness(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.liveness_detection = enabled;
        Ok(())
    })
}

/// Enable/disable attention requirement (eyes open, looking at camera).
pub fn set_require_attention(required: bool) -> KernelResult<()> {
    with_state(|state| {
        state.require_attention = required;
        Ok(())
    })
}

/// Enable/disable auto-dismiss lock screen on match.
pub fn set_auto_dismiss(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.auto_dismiss = enabled;
        Ok(())
    })
}

/// Enroll a user.
pub fn enroll(user_id: u32, user_name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.enrollments.iter().any(|e| e.user_id == user_id) {
            return Err(KernelError::AlreadyExists);
        }
        if state.enrollments.len() >= MAX_ENROLLMENTS {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        state.enrollments.push(Enrollment {
            user_id,
            user_name: String::from(user_name),
            enrolled_ns: now,
            last_verified_ns: 0,
            verify_count: 0,
            fail_count: 0,
            model_quality: 85,
        });
        Ok(())
    })
}

/// Remove enrollment.
pub fn unenroll(user_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.enrollments.len();
        state.enrollments.retain(|e| e.user_id != user_id);
        if state.enrollments.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Verify face (simulate). Returns result for the given user.
pub fn verify(user_id: u32, is_live: bool) -> KernelResult<VerifyResult> {
    with_state(|state| {
        state.total_verifications += 1;
        let now = crate::hpet::elapsed_ns();

        if !state.enabled {
            return Ok(VerifyResult::CameraError);
        }

        let enrollment = match state.enrollments.iter_mut().find(|e| e.user_id == user_id) {
            Some(e) => e,
            None => return Ok(VerifyResult::NoEnrollment),
        };

        // Liveness check.
        if state.liveness_detection && !is_live {
            enrollment.fail_count += 1;
            state.total_rejections += 1;
            return Ok(VerifyResult::LivenessCheckFailed);
        }

        // Simulate match (always matches enrolled user).
        enrollment.verify_count += 1;
        enrollment.last_verified_ns = now;
        state.total_matches += 1;
        Ok(VerifyResult::Matched)
    })
}

/// Get enrollment for a user.
pub fn get_enrollment(user_id: u32) -> Option<Enrollment> {
    STATE.lock().as_ref().and_then(|s| {
        s.enrollments.iter().find(|e| e.user_id == user_id).cloned()
    })
}

/// List all enrollments.
pub fn list_enrollments() -> Vec<Enrollment> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.enrollments.clone())
}

/// Is face unlock enabled?
pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.enabled)
}

/// Get current security level.
pub fn get_security() -> SecurityLevel {
    STATE.lock().as_ref().map_or(SecurityLevel::Standard, |s| s.security_level)
}

/// Statistics: (enrollments, verifications, matches, rejections, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.enrollments.len(), s.total_verifications, s.total_matches, s.total_rejections, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("faceunlock::self_test() — running tests...");
    init_defaults();

    // 1: Not enabled by default.
    assert!(!is_enabled());
    assert_eq!(get_security(), SecurityLevel::Standard);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Enroll user.
    set_enabled(true).expect("enable");
    enroll(1, "alice").expect("enroll");
    assert_eq!(list_enrollments().len(), 1);
    crate::serial_println!("  [2/8] enroll: OK");

    // 3: Duplicate enrollment fails.
    assert!(enroll(1, "alice").is_err());
    crate::serial_println!("  [3/8] no dup: OK");

    // 4: Verify (success).
    let result = verify(1, true).expect("verify");
    assert_eq!(result, VerifyResult::Matched);
    crate::serial_println!("  [4/8] verify ok: OK");

    // 5: Liveness check failure.
    let result = verify(1, false).expect("verify2");
    assert_eq!(result, VerifyResult::LivenessCheckFailed);
    crate::serial_println!("  [5/8] liveness: OK");

    // 6: Non-enrolled user.
    let result = verify(99, true).expect("verify3");
    assert_eq!(result, VerifyResult::NoEnrollment);
    crate::serial_println!("  [6/8] no enrollment: OK");

    // 7: Security level.
    set_security(SecurityLevel::High).expect("sec");
    assert_eq!(get_security(), SecurityLevel::High);
    crate::serial_println!("  [7/8] security: OK");

    // 8: Stats.
    let (enrollments, verifications, matches, rejections, ops) = stats();
    assert_eq!(enrollments, 1);
    assert_eq!(verifications, 3);
    assert_eq!(matches, 1);
    assert_eq!(rejections, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("faceunlock::self_test() — all 8 tests passed");
}
