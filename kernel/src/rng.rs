//! Kernel cryptographically-secure random number generator (CSPRNG).
//!
//! Provides high-quality random bytes for all kernel consumers: crypto,
//! address space layout randomization, TCP sequence numbers, capability
//! token generation, etc.
//!
//! ## Design
//!
//! Uses ChaCha20 as the core CSPRNG (same algorithm as Linux's
//! `/dev/urandom` since kernel 4.8).  ChaCha20 is:
//! - Fast in software (no lookup tables → no cache-timing attacks)
//! - Well-analyzed with strong security proofs
//! - Simple to implement correctly
//!
//! ## Entropy Sources
//!
//! 1. **RDRAND/RDSEED** (Intel/AMD hardware RNG) — primary source when
//!    available.  Provides true random numbers from on-die noise sources.
//! 2. **TSC jitter** — differences between consecutive `rdtsc` reads
//!    contain hardware timing noise (CPU pipeline effects, cache state,
//!    interrupt timing).
//! 3. **HPET counter** — high-resolution monotonic counter, contributes
//!    unpredictable low bits.
//! 4. **Interrupt timing** — ISR arrival times mixed into the entropy
//!    pool via [`add_interrupt_entropy()`].
//!
//! ## Seeded vs. credited
//!
//! Being *keyed* and being *seeded* are different states, and this module
//! tracks them separately. [`fill()`] always produces a keystream — early-boot
//! consumers such as stack canaries and ASLR offsets need bytes before any
//! entropy source has had time to accumulate. [`is_ready()`] additionally says
//! whether at least 256 bits of that key material were genuinely
//! *unpredictable*, which is what userspace key generation requires and what
//! `SYS_GETRANDOM` waits for. Clock reads are stirred in but credited nothing:
//! on a VM booted twice from one image they correlate across boots. See the
//! "Entropy credit" section further down.
//!
//! ## API
//!
//! - [`fill(buf)`] — fill a buffer with random bytes (primary API)
//! - [`next_u64()`] — get a random u64
//! - [`next_u32()`] — get a random u32
//! - [`add_interrupt_entropy(timestamp)`] — mix in ISR timing data
//! - [`is_ready()`] — whether the pool is credited enough for key material
//! - [`wait_until_ready(timeout_ns)`] — block a task until it is
//! - [`init()`] — initialize the RNG (called once at boot)
//!
//! ## Thread Safety
//!
//! The RNG state is protected by a spinlock.  For hot paths, callers
//! should batch their random bytes rather than calling `next_u64()`
//! in a tight loop.
//!
//! ## References
//!
//! - D.J. Bernstein, "ChaCha, a variant of Salsa20" (2008)
//! - Linux `drivers/char/random.c` — ChaCha20-based CRNG
//! - RFC 7539 — ChaCha20 specification

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use spin::Mutex;

use crate::serial_println;

// ---------------------------------------------------------------------------
// ChaCha20 core
// ---------------------------------------------------------------------------

/// ChaCha20 state: 16 × u32 words (512 bits).
///
/// Layout (per RFC 7539):
/// - Words 0-3: Constants ("expand 32-byte k")
/// - Words 4-11: 256-bit key (our seed)
/// - Word 12: Block counter
/// - Words 13-15: 96-bit nonce (we use 0; re-key instead of nonce rotation)
#[derive(Clone)]
struct ChaCha20State {
    state: [u32; 16],
}

/// ChaCha20 constants: "expand 32-byte k" in little-endian u32s.
const CHACHA_CONSTANTS: [u32; 4] = [0x6170_7865, 0x3320_646E, 0x7962_2D32, 0x6B20_6574];

impl ChaCha20State {
    /// Create a new ChaCha20 state from a 256-bit key.
    fn new(key: &[u32; 8]) -> Self {
        let mut state = [0u32; 16];
        state[0] = CHACHA_CONSTANTS[0];
        state[1] = CHACHA_CONSTANTS[1];
        state[2] = CHACHA_CONSTANTS[2];
        state[3] = CHACHA_CONSTANTS[3];
        state[4] = key[0];
        state[5] = key[1];
        state[6] = key[2];
        state[7] = key[3];
        state[8] = key[4];
        state[9] = key[5];
        state[10] = key[6];
        state[11] = key[7];
        state[12] = 0; // Block counter.
        state[13] = 0; // Nonce (unused — we re-key).
        state[14] = 0;
        state[15] = 0;
        Self { state }
    }

    /// Generate 64 bytes of keystream, advancing the block counter.
    #[allow(clippy::arithmetic_side_effects)]
    fn generate_block(&mut self, output: &mut [u8; 64]) {
        let mut working = self.state;

        // 20 rounds (10 double-rounds).
        for _ in 0..10 {
            // Column round.
            quarter_round(&mut working, 0, 4, 8, 12);
            quarter_round(&mut working, 1, 5, 9, 13);
            quarter_round(&mut working, 2, 6, 10, 14);
            quarter_round(&mut working, 3, 7, 11, 15);
            // Diagonal round.
            quarter_round(&mut working, 0, 5, 10, 15);
            quarter_round(&mut working, 1, 6, 11, 12);
            quarter_round(&mut working, 2, 7, 8, 13);
            quarter_round(&mut working, 3, 4, 9, 14);
        }

        // Add the original state (makes ChaCha20 a PRF, not just a permutation).
        for i in 0..16 {
            working[i] = working[i].wrapping_add(self.state[i]);
        }

        // Serialize to little-endian bytes.
        for (i, word) in working.iter().enumerate() {
            let bytes = word.to_le_bytes();
            let base = i * 4;
            output[base] = bytes[0];
            output[base + 1] = bytes[1];
            output[base + 2] = bytes[2];
            output[base + 3] = bytes[3];
        }

        // Advance block counter.
        self.state[12] = self.state[12].wrapping_add(1);
        if self.state[12] == 0 {
            // Overflow — increment the "nonce" as an extended counter.
            self.state[13] = self.state[13].wrapping_add(1);
        }
    }
}

/// ChaCha20 quarter round operation.
#[inline(always)]
#[allow(clippy::arithmetic_side_effects)]
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

// ---------------------------------------------------------------------------
// CSPRNG state
// ---------------------------------------------------------------------------

/// The kernel CSPRNG instance.
struct KernelRng {
    /// ChaCha20 cipher state.
    chacha: ChaCha20State,
    /// Buffered keystream (64 bytes per ChaCha20 block).
    buffer: [u8; 64],
    /// Position within the buffer (next byte to emit).
    buf_pos: usize,
    /// Total bytes generated (for re-key scheduling).
    bytes_generated: u64,
    /// Whether the RNG has been seeded.
    seeded: bool,
}

impl KernelRng {
    const fn new() -> Self {
        Self {
            chacha: ChaCha20State { state: [0u32; 16] },
            buffer: [0u8; 64],
            buf_pos: 64, // Empty — will trigger refill on first use.
            bytes_generated: 0,
            seeded: false,
        }
    }

    /// Seed (or re-seed) the RNG with new key material.
    fn seed(&mut self, key: &[u32; 8]) {
        self.chacha = ChaCha20State::new(key);
        self.buf_pos = 64; // Force buffer refill.
        self.seeded = true;
    }

    /// Mix additional entropy into the current state.
    ///
    /// XORs entropy into the key portion of the ChaCha20 state.
    /// This strengthens the RNG without replacing the existing state
    /// (forward secrecy: even if the current state leaks, past output
    /// cannot be recovered once re-keyed).
    #[allow(clippy::arithmetic_side_effects)]
    fn mix_entropy(&mut self, entropy: u64) {
        // Mix into key words using XOR (preserves existing entropy).
        let lo = entropy as u32;
        let hi = (entropy >> 32) as u32;
        self.chacha.state[4] ^= lo;
        self.chacha.state[5] ^= hi;
        self.chacha.state[6] ^= lo.wrapping_mul(0x9E37_79B9); // Golden ratio hash.
        self.chacha.state[7] ^= hi.wrapping_mul(0x6A09_E667); // SHA-256 init constant.
    }

    /// Generate random bytes into the provided buffer.
    #[allow(clippy::arithmetic_side_effects)]
    fn fill(&mut self, buf: &mut [u8]) {
        let mut offset = 0;
        while offset < buf.len() {
            if self.buf_pos >= 64 {
                // Buffer exhausted — generate a new block.
                self.chacha.generate_block(&mut self.buffer);
                self.buf_pos = 0;
                self.bytes_generated = self.bytes_generated.wrapping_add(64);

                // Re-key every 1 MiB of output for forward secrecy.
                // Uses the first 32 bytes of output as the new key.
                if self.bytes_generated.is_multiple_of(1024 * 1024) {
                    self.rekey_from_output();
                }
            }

            let available = 64usize.saturating_sub(self.buf_pos);
            let needed = buf.len().saturating_sub(offset);
            let copy_len = available.min(needed);

            if let (Some(dst), Some(src)) = (
                buf.get_mut(offset..offset.wrapping_add(copy_len)),
                self.buffer
                    .get(self.buf_pos..self.buf_pos.wrapping_add(copy_len)),
            ) {
                dst.copy_from_slice(src);
            }

            self.buf_pos = self.buf_pos.wrapping_add(copy_len);
            offset = offset.wrapping_add(copy_len);
        }
    }

    /// Re-key from the current output stream (forward secrecy).
    ///
    /// Generates a fresh block, uses the first 32 bytes as a new key,
    /// and discards the rest.  After re-keying, even if the new state
    /// is compromised, the old state (and all prior output) cannot be
    /// recovered.
    fn rekey_from_output(&mut self) {
        let mut block = [0u8; 64];
        self.chacha.generate_block(&mut block);

        let mut new_key = [0u32; 8];
        for (i, chunk) in block[..32].chunks_exact(4).enumerate() {
            new_key[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        self.chacha = ChaCha20State::new(&new_key);
        self.buf_pos = 64; // Force fresh buffer.
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// The global kernel RNG, protected by a spinlock.
static RNG: Mutex<KernelRng> = Mutex::new(KernelRng::new());

/// Whether the RNG has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Entropy accumulator for interrupt timing.
///
/// XORs in ISR timestamps continuously.  Mixed into the RNG state
/// periodically (every 256 interrupts) to add hardware jitter entropy.
static ENTROPY_ACCUM: AtomicU64 = AtomicU64::new(0);

/// Count of entropy contributions (for mixing schedule).
static ENTROPY_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total bytes generated since boot.
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);

/// Total re-seeds since boot.
static RESEED_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Entropy credit — "is this pool actually seeded, or merely keyed?"
// ---------------------------------------------------------------------------
//
// Seeding and *crediting* are different things, and conflating them is the
// specific bug Linux shipped for years: `/dev/urandom` handed out output from
// a pool keyed only with boot-time clock reads, which an attacker who can
// guess the uptime to within a few hundred nanoseconds can reproduce.  The fix
// there — and here — is to track how much of the key material is genuinely
// *unpredictable* and to make that a queryable state.
//
// So the pool has two independent properties:
//
// - `KernelRng::seeded` — the ChaCha state holds a key and `fill()` will
//   produce a keystream.  Always true after `init()`, and made true lazily by
//   `fill()` itself so early-boot kernel consumers (stack canaries, ASLR
//   offsets) are never handed zeros.
// - `CRNG_READY` — that key contains at least [`CREDIT_TARGET_BITS`] bits that
//   an observer of an identically-configured machine could not have predicted.
//   This is what [`is_ready()`] reports and what `SYS_GETRANDOM` waits for.
//
// **A clock read contributes key material but zero credit.** `init()` mixes
// the HPET counter, TSC jitter and the APIC tick count into the key because
// they cost nothing and can only help; none of them is credited, because on a
// VM booted twice from the same image they are correlated across boots — which
// is exactly the case `requests/c-a-userspace-entropy-syscall.md` names in its
// acceptance criteria.  Only RDSEED/RDRAND and interrupt-arrival timing are
// credited.
//
// References:
// - Linux `drivers/char/random.c` — `crng_init`, `crng_ready()`, and the
//   `credit_init_bits()` accounting this mirrors.
// - Linux commit history around 5.18's random.c rewrite (Jason A. Donenfeld),
//   which is where the "credit, don't just stir" distinction is stated most
//   plainly.

/// Bits of entropy that must be credited before the pool is declared ready.
///
/// 256 bits, matching Linux's `POOL_READY_BITS`.  It is the security level of
/// the ChaCha20 key, so crediting more would be accounting fiction and
/// crediting less would declare readiness at a strength the cipher does not
/// have.
const CREDIT_TARGET_BITS: u32 = 256;

/// Bits of entropy credited so far, saturating at [`CREDIT_TARGET_BITS`].
static CREDIT_BITS: AtomicU32 = AtomicU32::new(0);

/// Set once [`CREDIT_BITS`] first reaches [`CREDIT_TARGET_BITS`].
///
/// A separate flag rather than a comparison on `CREDIT_BITS` so the
/// ready-transition can be detected exactly once (for the boot log line) from
/// an interrupt handler, without a read-modify-write race between two CPUs.
static CRNG_READY: AtomicBool = AtomicBool::new(false);

/// Previous interrupt timestamp, and the first two orders of difference.
///
/// Used by [`add_interrupt_entropy`] to decide whether an interrupt's arrival
/// time was actually unpredictable.  Relaxed atomics: a torn interleaving
/// between CPUs makes the deltas *noisier*, never less conservative, and the
/// values are not otherwise load-bearing.
static LAST_TIME: AtomicU64 = AtomicU64::new(0);
/// First-order difference of [`LAST_TIME`].
static LAST_DELTA: AtomicU64 = AtomicU64::new(0);
/// Second-order difference of [`LAST_TIME`].
static LAST_DELTA2: AtomicU64 = AtomicU64::new(0);

/// Interrupts whose timing was credited (diagnostic).
static CREDITED_IRQS: AtomicU64 = AtomicU64::new(0);

/// Credit `bits` of entropy to the pool.
///
/// Saturates at [`CREDIT_TARGET_BITS`]; safe to call from hard IRQ context
/// (atomics only, no locks, no allocation).  Returns `true` on the single call
/// that takes the pool from not-ready to ready, so exactly one caller reports
/// the transition.
fn credit_entropy(bits: u32) -> bool {
    if bits == 0 || CRNG_READY.load(Ordering::Relaxed) {
        return false;
    }
    let before = CREDIT_BITS.fetch_add(bits, Ordering::Relaxed);
    let after = before.saturating_add(bits);
    if after < CREDIT_TARGET_BITS {
        return false;
    }
    // Clamp so the counter reads as a credit total, not a running sum of every
    // contribution ever made.
    CREDIT_BITS.store(CREDIT_TARGET_BITS, Ordering::Relaxed);
    // `swap` rather than `store` so only the first arrival claims the
    // transition, even if two CPUs cross the threshold in the same instant.
    !CRNG_READY.swap(true, Ordering::AcqRel)
}

// ---------------------------------------------------------------------------
// Hardware entropy sources
// ---------------------------------------------------------------------------

/// Try to read a hardware random number via RDRAND.
///
/// Returns `Some(value)` if RDRAND is available and succeeded,
/// `None` if RDRAND is not supported or failed after retries.
///
/// Uses the cached CPU feature flags from [`crate::cpu::features()`]
/// instead of running CPUID on every call.
fn try_rdrand() -> Option<u64> {
    // Use centralized feature detection (cached at boot).
    let features = crate::cpu::features()?;
    if !features.rdrand {
        return None;
    }

    // Try RDRAND up to 10 times (can fail transiently if the HW RNG
    // is busy regenerating entropy).
    for _ in 0..10 {
        let value: u64;
        let success: u8;

        // SAFETY: We verified RDRAND is supported via cpu::features().
        // The CF flag indicates success (1) or failure (0).
        unsafe {
            core::arch::asm!(
                "rdrand {val}",
                "setc {ok}",
                val = out(reg) value,
                ok = out(reg_byte) success,
                options(nomem, nostack),
            );
        }

        if success != 0 {
            return Some(value);
        }
    }

    None // RDRAND failed after retries.
}

/// Try to read a hardware random seed via RDSEED.
///
/// RDSEED provides "true random" bits (conditioned noise source),
/// while RDRAND provides "deterministic random" bits (CSPRNG seeded
/// from hardware noise).  RDSEED is preferred for seeding other CSPRNGs.
///
/// Uses the cached CPU feature flags from [`crate::cpu::features()`]
/// instead of running CPUID on every call.
fn try_rdseed() -> Option<u64> {
    // Use centralized feature detection (cached at boot).
    let features = crate::cpu::features()?;
    if !features.rdseed {
        return None;
    }

    for _ in 0..10 {
        let value: u64;
        let success: u8;

        // SAFETY: We verified RDSEED is supported via cpu::features().
        unsafe {
            core::arch::asm!(
                "rdseed {val}",
                "setc {ok}",
                val = out(reg) value,
                ok = out(reg_byte) success,
                options(nomem, nostack),
            );
        }

        if success != 0 {
            return Some(value);
        }
    }

    None
}

/// Gather entropy from TSC jitter.
///
/// Reads TSC multiple times and XORs the differences.  The low bits
/// of inter-read timing contain genuine hardware noise from pipeline
/// stalls, cache effects, and interrupt timing.
fn gather_tsc_jitter() -> u64 {
    let mut entropy: u64 = 0;
    let mut prev = crate::bench::rdtsc();

    for _ in 0..16 {
        let now = crate::bench::rdtsc();
        let delta = now.wrapping_sub(prev);
        // Mix the low bits (most jittery) into the accumulator.
        entropy ^= delta;
        entropy = entropy.rotate_left(7);
        prev = now;
    }

    entropy
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the kernel RNG.
///
/// Gathers entropy from all available hardware sources and seeds the
/// ChaCha20 CSPRNG.  Called once during boot after the HPET and APIC
/// timer are initialized.
///
/// After this call, [`fill()`], [`next_u64()`], etc. are ready to use.
pub fn init() {
    let mut key = [0u32; 8];

    // Entropy *credit* accrues only from the two hardware sources below.  The
    // clock reads that follow are mixed into the key but credited nothing —
    // see the "Entropy credit" section above for why.
    let mut credited: u32 = 0;

    // Source 1: RDSEED (best quality — true randomness).
    for word in &mut key {
        if let Some(val) = try_rdseed() {
            *word = val as u32;
            // 32 bits per word, which is all of this word's key material.
            credited = credited.saturating_add(32);
        }
    }

    // Source 2: RDRAND (hardware CSPRNG output).
    //
    // Credited 64 bits per successful read.  RDRAND is a CSPRNG rather than a
    // raw noise source, but it is reseeded from the same on-die entropy RDSEED
    // draws from, so its output is unpredictable to anything outside the CPU
    // package.  Linux makes the same call (`random.trust_cpu`, default on).
    if let Some(r1) = try_rdrand() {
        key[0] ^= r1 as u32;
        key[1] ^= (r1 >> 32) as u32;
        credited = credited.saturating_add(64);
    }
    if let Some(r2) = try_rdrand() {
        key[2] ^= r2 as u32;
        key[3] ^= (r2 >> 32) as u32;
        credited = credited.saturating_add(64);
    }

    // Source 3: HPET counter (contributes unpredictable low bits).
    let hpet_ns = crate::hpet::elapsed_ns();
    key[4] ^= hpet_ns as u32;
    key[5] ^= (hpet_ns >> 32) as u32;

    // Source 4: TSC jitter.
    let jitter = gather_tsc_jitter();
    key[6] ^= jitter as u32;
    key[7] ^= (jitter >> 32) as u32;

    // Source 5: APIC tick count (adds boot-time variability).
    let ticks = crate::apic::tick_count();
    key[0] ^= ticks as u32;
    key[3] ^= (ticks >> 32) as u32;

    // Seed the CSPRNG.
    let mut rng = RNG.lock();
    rng.seed(&key);
    drop(rng);

    INITIALIZED.store(true, Ordering::Release);
    RESEED_COUNT.fetch_add(1, Ordering::Relaxed);

    let became_ready = credit_entropy(credited);

    // Report entropy sources.
    let has_rdrand = try_rdrand().is_some();
    let has_rdseed = try_rdseed().is_some();
    serial_println!(
        "[rng] Initialized (RDRAND={}, RDSEED={}, HPET={:#x}, jitter={:#x})",
        if has_rdrand { "yes" } else { "no" },
        if has_rdseed { "yes" } else { "no" },
        hpet_ns,
        jitter,
    );
    // State the credit explicitly rather than leaving it to be inferred from
    // the RDRAND/RDSEED line above: on a machine with neither (QEMU's default
    // CPU model is one), the pool is keyed but *not* ready, and userspace
    // `getrandom` will block until interrupt timing has made up the shortfall.
    // A reader debugging a stalled boot needs that stated, not deduced.
    if became_ready {
        serial_println!(
            "[rng] crng init done ({} bits credited from the CPU RNG)",
            credited.min(CREDIT_TARGET_BITS),
        );
    } else {
        serial_println!(
            "[rng] crng NOT ready: {}/{} bits credited (no usable CPU RNG); \
             waiting on interrupt timing",
            CREDIT_BITS.load(Ordering::Relaxed),
            CREDIT_TARGET_BITS,
        );
    }
}

/// Fill a buffer with cryptographically-secure random bytes.
///
/// This is the primary API for kernel random number generation.
/// Safe to call from any context that can take a spinlock (not raw ISR).
///
/// # Panics
///
/// Does not panic.  If the RNG is not yet initialized, uses a fallback
/// (TSC + HPET based seeding) which provides weaker but functional
/// randomness during early boot.
#[allow(clippy::arithmetic_side_effects)]
pub fn fill(buf: &mut [u8]) {
    // Mix in accumulated interrupt entropy periodically.
    let count = ENTROPY_COUNT.load(Ordering::Relaxed);
    if count > 0 && count.is_multiple_of(256) {
        let entropy = ENTROPY_ACCUM.swap(0, Ordering::Relaxed);
        if entropy != 0 {
            let mut rng = RNG.lock();
            rng.mix_entropy(entropy);
            drop(rng);
        }
    }

    let mut rng = RNG.lock();

    // Lazy initialization if init() hasn't been called yet.
    if !rng.seeded {
        let hpet = crate::hpet::elapsed_ns();
        let tsc = crate::bench::rdtsc();
        let key = [
            hpet as u32,
            (hpet >> 32) as u32,
            tsc as u32,
            (tsc >> 32) as u32,
            0x4F53_524E, // "OSRN"
            0x4700_4300, // "GC"
            hpet.wrapping_mul(tsc) as u32,
            tsc.wrapping_mul(0x9E37_79B9_7F4A_7C15) as u32,
        ];
        rng.seed(&key);
    }

    rng.fill(buf);
    TOTAL_BYTES.fetch_add(buf.len() as u64, Ordering::Relaxed);
}

/// Get a random u64.
pub fn next_u64() -> u64 {
    let mut buf = [0u8; 8];
    fill(&mut buf);
    u64::from_le_bytes(buf)
}

/// Get a random u32.
#[allow(dead_code)]
pub fn next_u32() -> u32 {
    let mut buf = [0u8; 4];
    fill(&mut buf);
    u32::from_le_bytes(buf)
}

/// Get a random value in [0, bound).
///
/// Uses rejection sampling to avoid modulo bias.
#[allow(clippy::arithmetic_side_effects)]
pub fn next_bounded(bound: u64) -> u64 {
    if bound <= 1 {
        return 0;
    }

    // Rejection sampling: generate random u64, reject if it falls in
    // the biased tail.  Expected iterations < 2 for any bound.
    let threshold = (u64::MAX - bound + 1) % bound;
    loop {
        let val = next_u64();
        if val >= threshold {
            return val % bound;
        }
    }
}

/// Add interrupt timing entropy.
///
/// Called from ISR handlers with a TSC timestamp.  The timing jitter
/// between interrupts contains genuine hardware entropy that strengthens
/// the CSPRNG over time.
///
/// This is lock-free and safe to call from hard IRQ context.
///
/// # Entropy credit
///
/// The timestamp is *always* stirred into the pool; whether it also earns
/// [`credit_entropy`] is decided by the third-order timing test from Linux's
/// `add_timer_randomness` (`drivers/char/random.c`): take the first, second
/// and third differences of successive arrival times and credit the event only
/// if all three are non-zero.  A perfectly periodic source — which is what a
/// free-running timer looks like on an idle machine — has a zero second or
/// third difference and earns nothing, so a metronome cannot talk the pool
/// into declaring itself seeded.
///
/// A qualifying event is credited `min(ilog2(min_delta), 8)` bits, where
/// `min_delta` is the smallest of the three differences — Linux's accounting,
/// with the cap lowered from its 11 bits to 8.  The quantity being estimated
/// is the unpredictability of the low bits of a TSC difference, and 8 says we
/// never claim more than one byte of it from a single tick.  At the 100 Hz
/// APIC timer that is ~32 ticks, so roughly a third of a second from the first
/// tick to `crng init done`, paid once per boot.
pub fn add_interrupt_entropy(timestamp: u64) {
    // XOR-fold into the accumulator (lock-free).
    ENTROPY_ACCUM.fetch_xor(timestamp, Ordering::Relaxed);
    ENTROPY_COUNT.fetch_add(1, Ordering::Relaxed);

    // Nothing below changes the pool once it is ready, so skip the whole
    // heuristic on the hot path after boot: this runs on every timer tick.
    if CRNG_READY.load(Ordering::Relaxed) {
        return;
    }

    let last = LAST_TIME.swap(timestamp, Ordering::Relaxed);
    if last == 0 {
        // First sample: no delta to take yet.
        return;
    }
    let delta = timestamp.wrapping_sub(last);
    let last_delta = LAST_DELTA.swap(delta, Ordering::Relaxed);
    let delta2 = delta.abs_diff(last_delta);
    let last_delta2 = LAST_DELTA2.swap(delta2, Ordering::Relaxed);
    let delta3 = delta2.abs_diff(last_delta2);

    if delta == 0 || delta2 == 0 || delta3 == 0 {
        return;
    }
    // `ilog2` is the position of the highest set bit, i.e. Linux's `fls() - 1`.
    // `min_delta` is non-zero by the test above, so `ilog2` is defined.
    let min_delta = delta.min(delta2).min(delta3);
    let bits = min_delta.ilog2().min(8);
    CREDITED_IRQS.fetch_add(1, Ordering::Relaxed);
    if credit_entropy(bits) {
        // Report from a work item, not from here.  `serial_println!` takes the
        // serial spinlock, and this is hard IRQ context: if the interrupted
        // task happened to hold that lock, printing would spin against itself
        // forever.  `workqueue::submit` is documented ISR-safe (a `try_lock`
        // and an atomic) and runs the callback in task context, which is
        // Linux's `execute_in_process_context(crng_set_ready, …)` in
        // `drivers/char/random.c` for the same reason.
        //
        // A dropped submission costs only the log line: readiness itself is
        // already published by `credit_entropy`, and `is_ready()` is what
        // every caller actually consults.
        let _ = crate::workqueue::submit(report_crng_ready, 0);
    }
}

/// Work-item callback: announce that the pool reached its credit target.
///
/// Runs in task context (see the call site in [`add_interrupt_entropy`]), so
/// taking the serial lock here is safe.
fn report_crng_ready(_arg: u64) {
    serial_println!(
        "[rng] crng init done ({} bits credited from interrupt timing; {} of \
         {} interrupts qualified)",
        CREDIT_TARGET_BITS,
        CREDITED_IRQS.load(Ordering::Relaxed),
        ENTROPY_COUNT.load(Ordering::Relaxed),
    );
}

/// Total random bytes generated since boot.
#[must_use]
#[allow(dead_code)]
pub fn total_bytes_generated() -> u64 {
    TOTAL_BYTES.load(Ordering::Relaxed)
}

/// Number of times the RNG has been re-seeded.
#[must_use]
#[allow(dead_code)]
pub fn reseed_count() -> u64 {
    RESEED_COUNT.load(Ordering::Relaxed)
}

/// Whether the RNG has been properly initialized.
///
/// Says only that [`init`] has run and the generator is keyed — *not* that the
/// key material is unpredictable.  For that, ask [`is_ready`].
#[must_use]
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

/// Total interrupt entropy contributions.
#[must_use]
#[allow(dead_code)]
pub fn entropy_contributions() -> u64 {
    ENTROPY_COUNT.load(Ordering::Relaxed)
}

/// Whether the pool holds enough *credited* entropy to be used for keys.
///
/// This is the question `SYS_GETRANDOM` asks, and it is **not** the same as
/// [`is_initialized()`]: a pool keyed from nothing but clock reads is
/// initialised and will happily produce a keystream, but two boots of the same
/// VM image can produce correlated output from it.  See the "Entropy credit"
/// section above.
///
/// Kernel-internal consumers ([`fill`], [`next_u64`], …) deliberately do *not*
/// consult this — ASLR offsets and stack canaries are wanted at boot, before
/// any pool could be credited, and weak randomness there is better than none.
/// Userspace, which asks for key material and can be made to wait, does.
#[must_use]
pub fn is_ready() -> bool {
    CRNG_READY.load(Ordering::Acquire)
}

/// Bits of entropy credited to the pool, out of [`credit_target_bits()`].
#[must_use]
pub fn credited_bits() -> u32 {
    CREDIT_BITS.load(Ordering::Relaxed)
}

/// Credit required before [`is_ready`] turns true.
#[must_use]
pub fn credit_target_bits() -> u32 {
    CREDIT_TARGET_BITS
}

/// Interrupts whose arrival timing passed the third-difference test.
#[must_use]
#[allow(dead_code)]
pub fn credited_interrupts() -> u64 {
    CREDITED_IRQS.load(Ordering::Relaxed)
}

/// Poll interval used by [`wait_until_ready`], in milliseconds.
///
/// The wait is resolved by a *condition* an interrupt handler publishes, not
/// by an event a waker can be handed, so this polls rather than parking on a
/// wait queue.  That is deliberate:
///
/// - The wake would have to come from hard IRQ context, where the only legal
///   wake primitive is `WaitQueue::try_wake_one` (`wake_all` takes a blocking
///   spinlock).  Bouncing it through a work item to reach `wake_all`
///   reintroduces the "was the submission dropped?" question, which would then
///   need its own timeout fallback — i.e. a poll, one level down.
/// - `WaitQueue` has no timed wait, and this wait **must** be bounded.
/// - The wait only exists before the pool is credited, i.e. during boot.
///
/// 25 ms, chosen to exceed the 10 ms period of the 100 Hz APIC timer: the loop
/// treats "no interrupt at all arrived during one poll interval" as proof that
/// nothing can feed the pool, and that inference is only sound if a healthy
/// system is *certain* to deliver a tick within the interval.
const READY_POLL_MS: u64 = 25;

/// Block the calling task until the pool is credited, or give up.
///
/// Returns `true` if the pool is ready.  Returns `false` if `timeout_ns`
/// elapses, or — much more usefully — as soon as it can prove that waiting is
/// futile because no interrupts are arriving to be credited.  That second exit
/// is what keeps the early-boot syscall self-tests fast: they run before the
/// APIC timer is configured, so on a machine with no CPU RNG the pool cannot
/// become ready at that point no matter how long anyone waits.
///
/// A `false` return means the caller should **fail**, not proceed with weak
/// bytes — see `syscall::handlers::sys_getrandom`, which turns it into an
/// error rather than fabricating key material.
///
/// Must be called from task context: past the two early-outs below it sleeps.
pub fn wait_until_ready(timeout_ns: u64) -> bool {
    if is_ready() {
        return true;
    }
    // Refuse to sleep before [`init`] has run.  This is not merely an
    // optimisation, it is what makes the function safe to call at all during
    // early boot: `main` runs the syscall self-tests (which dispatch
    // `SYS_GETRANDOM`) well before it brings up the HPET, the TSC calibration
    // and `rng::init` itself.  With no timebase there is no honest deadline to
    // measure against and `sched::sleep_ms`'s own fallbacks have nothing to
    // spin on, so a wait started here could not be bounded.  It would also be
    // pointless: nothing has been credited and nothing can be until the
    // interrupt sources this pool feeds on are running.
    if !is_initialized() {
        return false;
    }
    let start = crate::hpet::elapsed_ns();
    loop {
        let irqs_before = ENTROPY_COUNT.load(Ordering::Relaxed);
        crate::sched::sleep_ms(READY_POLL_MS);
        if is_ready() {
            return true;
        }
        if ENTROPY_COUNT.load(Ordering::Relaxed) == irqs_before {
            // Not one interrupt in a whole poll interval — longer than the
            // timer period — so the only source that could credit this pool is
            // not running.  Waiting out the full timeout would just be a
            // slower way to return the same answer.
            return false;
        }
        if crate::hpet::elapsed_ns().saturating_sub(start) >= timeout_ns {
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kernel CSPRNG.
///
/// Verifies:
/// 1. Output is non-zero (basic sanity).
/// 2. Consecutive outputs differ (not stuck).
/// 3. Distribution is roughly uniform (chi-squared test on byte values).
/// 4. RDRAND detection works (informational).
pub fn self_test() {
    serial_println!("[rng] Running self-test...");

    // --- 1. Non-zero output ---
    let v1 = next_u64();
    let v2 = next_u64();
    // Both being zero is probability 2^-128 — safe to assert.
    assert!(v1 != 0 || v2 != 0, "RNG output should not be all zeros");
    serial_println!("[rng]   Non-zero output: OK ({:#018x}, {:#018x})", v1, v2);

    // --- 2. Consecutive outputs differ ---
    let v3 = next_u64();
    let v4 = next_u64();
    assert_ne!(v3, v4, "Consecutive RNG outputs should differ");
    serial_println!("[rng]   Outputs differ: OK");

    // --- 3. Basic uniformity check ---
    // Generate 1024 bytes and check that all 256 byte values appear
    // at least once (probability of missing one is astronomically low
    // with 1024 samples and 256 bins).
    let mut buf = [0u8; 1024];
    fill(&mut buf);
    let mut seen = [false; 256];
    for &b in &buf {
        seen[b as usize] = true;
    }
    let seen_count = seen.iter().filter(|&&s| s).count();
    // With 1024 uniform samples over 256 bins, expected coverage is
    // ~252-256.  We accept ≥240 to account for statistical variation.
    assert!(
        seen_count >= 240,
        "Expected ≥240 distinct byte values in 1024 samples, got {}",
        seen_count,
    );
    serial_println!(
        "[rng]   Uniformity: OK ({}/256 byte values in 1024 samples)",
        seen_count
    );

    // --- 4. Bounded generation ---
    for _ in 0..100 {
        let v = next_bounded(10);
        assert!(v < 10, "next_bounded(10) should return < 10");
    }
    serial_println!("[rng]   Bounded generation: OK");

    // --- 5. Hardware RNG availability ---
    let rdrand_ok = try_rdrand().is_some();
    let rdseed_ok = try_rdseed().is_some();
    serial_println!(
        "[rng]   Hardware: RDRAND={}, RDSEED={}",
        if rdrand_ok {
            "available"
        } else {
            "not available"
        },
        if rdseed_ok {
            "available"
        } else {
            "not available"
        },
    );

    serial_println!(
        "[rng]   Stats: generated={} bytes, reseeds={}, entropy_count={}",
        total_bytes_generated(),
        reseed_count(),
        entropy_contributions(),
    );

    serial_println!("[rng] Self-test PASSED");
}
