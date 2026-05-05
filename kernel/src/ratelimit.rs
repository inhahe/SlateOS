//! Kernel rate limiter.
//!
//! Provides token-bucket rate limiting for kernel events — log messages,
//! warnings, error paths, interrupt handlers.  Prevents event floods from
//! consuming excessive CPU time or filling ring buffers.
//!
//! ## Design
//!
//! Each rate limiter is a simple token bucket:
//! - Tokens replenish at a fixed rate (tokens per second).
//! - Each event consumes one token.
//! - If no tokens are available, the event is suppressed.
//! - A counter tracks suppressed events for reporting.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::ratelimit::RateLimiter;
//!
//! // Allow 10 events per second, burst of 10.
//! static MY_LIMITER: RateLimiter = RateLimiter::new(10, 10);
//!
//! if MY_LIMITER.allow() {
//!     serial_println!("This message is rate-limited to 10/sec");
//! }
//!
//! // Or use the macro (auto-creates a static limiter per call site):
//! ratelimit_println!(10, "Rate-limited message: {}", value);
//! ```
//!
//! ## References
//!
//! - Linux `__ratelimit()` in `lib/ratelimit.c`
//! - Linux `printk_ratelimit()`
//! - Token bucket algorithm (Wikipedia)

use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Rate limiter
// ---------------------------------------------------------------------------

/// A token-bucket rate limiter.
///
/// Lock-free, safe to use from any context (ISR, softirq, normal).
/// Each instance is independent — create one per event source.
pub struct RateLimiter {
    /// Tokens replenished per second.
    rate: u64,
    /// Maximum burst size (bucket capacity).
    burst: u64,
    /// Current token count (fixed-point: multiplied by PRECISION).
    tokens: AtomicU64,
    /// TSC timestamp of last replenish.
    last_replenish: AtomicU64,
    /// Number of events suppressed since last successful allow().
    suppressed: AtomicU64,
    /// Total events suppressed since creation.
    total_suppressed: AtomicU64,
    /// Total events allowed since creation.
    total_allowed: AtomicU64,
}

/// Fixed-point precision multiplier for token fractions.
const PRECISION: u64 = 1024;

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// - `rate`: events allowed per second.
    /// - `burst`: maximum burst size (events that can fire immediately
    ///   after a period of silence).
    pub const fn new(rate: u64, burst: u64) -> Self {
        Self {
            rate,
            burst,
            tokens: AtomicU64::new(burst * PRECISION),
            last_replenish: AtomicU64::new(0),
            suppressed: AtomicU64::new(0),
            total_suppressed: AtomicU64::new(0),
            total_allowed: AtomicU64::new(0),
        }
    }

    /// Try to consume one token.
    ///
    /// Returns `true` if the event is allowed (token consumed).
    /// Returns `false` if rate-limited (no tokens available).
    pub fn allow(&self) -> bool {
        self.replenish();

        // Try to consume one token.
        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current < PRECISION {
                // No tokens available — suppress.
                self.suppressed.fetch_add(1, Ordering::Relaxed);
                self.total_suppressed.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            let new_val = current - PRECISION;
            if self.tokens.compare_exchange_weak(
                current, new_val, Ordering::Relaxed, Ordering::Relaxed
            ).is_ok() {
                // Report suppressed count if any.
                let _supp = self.suppressed.swap(0, Ordering::Relaxed);
                self.total_allowed.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
    }

    /// Try to consume one token and return the number of suppressed
    /// events since the last allowed event.
    ///
    /// Useful for "N events suppressed" messages.
    pub fn allow_with_suppressed(&self) -> Option<u64> {
        self.replenish();

        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current < PRECISION {
                self.suppressed.fetch_add(1, Ordering::Relaxed);
                self.total_suppressed.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            let new_val = current - PRECISION;
            if self.tokens.compare_exchange_weak(
                current, new_val, Ordering::Relaxed, Ordering::Relaxed
            ).is_ok() {
                let supp = self.suppressed.swap(0, Ordering::Relaxed);
                self.total_allowed.fetch_add(1, Ordering::Relaxed);
                return Some(supp);
            }
        }
    }

    /// Replenish tokens based on elapsed time.
    fn replenish(&self) {
        let now = crate::bench::rdtsc();
        let last = self.last_replenish.load(Ordering::Relaxed);

        // First call: initialize timestamp.
        if last == 0 {
            let _ = self.last_replenish.compare_exchange(
                0, now, Ordering::Relaxed, Ordering::Relaxed
            );
            return;
        }

        let elapsed_cycles = now.saturating_sub(last);
        let freq = crate::bench::tsc_freq();
        if freq == 0 {
            return;
        }

        // Calculate tokens to add: elapsed_seconds * rate * PRECISION
        // = elapsed_cycles * rate * PRECISION / freq
        // To avoid overflow: compute in two steps.
        let tokens_to_add = elapsed_cycles
            .saturating_mul(self.rate)
            .saturating_mul(PRECISION)
            / freq;

        if tokens_to_add == 0 {
            return;
        }

        // Update last replenish time (best-effort CAS — if another CPU
        // beat us, that's fine; some extra tokens may be granted but
        // the burst cap prevents accumulation beyond the limit).
        let _ = self.last_replenish.compare_exchange(
            last, now, Ordering::Relaxed, Ordering::Relaxed
        );

        // Add tokens, capped at burst.
        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            let cap = self.burst * PRECISION;
            let new_val = current.saturating_add(tokens_to_add).min(cap);
            if new_val == current {
                break; // Already at cap.
            }
            if self.tokens.compare_exchange_weak(
                current, new_val, Ordering::Relaxed, Ordering::Relaxed
            ).is_ok() {
                break;
            }
        }
    }

    /// Get the number of currently suppressed events (since last allow).
    #[must_use]
    pub fn current_suppressed(&self) -> u64 {
        self.suppressed.load(Ordering::Relaxed)
    }

    /// Get total events suppressed since creation.
    #[must_use]
    pub fn total_suppressed(&self) -> u64 {
        self.total_suppressed.load(Ordering::Relaxed)
    }

    /// Get total events allowed since creation.
    #[must_use]
    pub fn total_allowed(&self) -> u64 {
        self.total_allowed.load(Ordering::Relaxed)
    }

    /// Reset the limiter to full capacity.
    pub fn reset(&self) {
        self.tokens.store(self.burst * PRECISION, Ordering::Relaxed);
        self.last_replenish.store(0, Ordering::Relaxed);
        self.suppressed.store(0, Ordering::Relaxed);
        self.total_suppressed.store(0, Ordering::Relaxed);
        self.total_allowed.store(0, Ordering::Relaxed);
    }
}

// Safe to share across threads — all fields are atomic.
unsafe impl Sync for RateLimiter {}
unsafe impl Send for RateLimiter {}

// ---------------------------------------------------------------------------
// Macros
// ---------------------------------------------------------------------------

/// Rate-limited serial print (creates a per-call-site static limiter).
///
/// Usage: `ratelimit_println!(rate_per_sec, "message: {}", arg);`
///
/// The first argument is the max events per second.  Subsequent arguments
/// are the same as `serial_println!`.
#[macro_export]
macro_rules! ratelimit_println {
    ($rate:expr, $($arg:tt)*) => {{
        static LIMITER: $crate::ratelimit::RateLimiter =
            $crate::ratelimit::RateLimiter::new($rate, $rate);
        if let Some(suppressed) = LIMITER.allow_with_suppressed() {
            if suppressed > 0 {
                $crate::serial_println!("  ({} similar messages suppressed)", suppressed);
            }
            $crate::serial_println!($($arg)*);
        }
    }};
}
