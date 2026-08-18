//! Cryptographically-secure randomness for userspace.
//!
//! This module is the single source of random bytes in libc.  It backs
//! [`getrandom`](crate::unistd::getrandom) and
//! [`getentropy`](crate::unistd::getentropy), and it implements the BSD
//! [`arc4random`] family.
//!
//! ## Why this module exists
//!
//! `getrandom`/`getentropy` used to be filled by an LCG:
//!
//! ```text
//! state = state * 0x5851F42D4C957F2D + 0x14057B7EF767814F
//! byte  = (state >> 56) as u8
//! ```
//!
//! seeded from a *single* 64-bit `RDRAND` draw — or, if `RDRAND` failed,
//! from the monotonic clock.  Those two calls are contractually CSPRNGs:
//! they are exactly what crypto code reaches for when it needs key
//! material, and every caller in the tree and every ported program assumes
//! so.  What they actually got was at most 64 bits of entropy no matter how
//! many bytes were asked for, from a generator whose entire internal state
//! is recoverable from a handful of output bytes.  A clock-seeded fallback
//! is worse still: predictable to anyone who can guess the uptime to within
//! a few hundred nanoseconds.
//!
//! ## Where the bytes come from now
//!
//! 1. **The kernel.** [`SYS_GETRANDOM`] draws from `kernel/src/rng.rs`, a
//!    ChaCha20 CSPRNG seeded from RDRAND/RDSEED, TSC jitter, the HPET
//!    counter and interrupt-arrival timing.  Userspace cannot see any of
//!    those sources itself, so this is the only complete answer.
//! 2. **RDSEED/RDRAND**, guarded by a `CPUID` feature check, used only to
//!    seed the local pool when the kernel is unreachable — which on this
//!    codebase means the host build, where `cargo test` runs against the
//!    Windows triple and the raw `SYSCALL` instruction is gated off.
//! 3. **Nothing else.**  If neither source is available we **fail**:
//!    `getrandom`/`getentropy` return `-1`/`EIO` and `arc4random` aborts.
//!    Fabricating plausible-looking bytes from a clock is what caused the
//!    bug above; a caller that gets an error can fail closed, a caller
//!    handed predictable bytes cannot.
//!
//! ## The `arc4random` pool
//!
//! A syscall per `arc4random()` would defeat the point of the API, so
//! `arc4random` runs a ChaCha20 stream in userspace, seeded from the two
//! sources above.  The pool lives in [`crate::perthread::PerThread`], so it
//! needs no lock and two threads never hand out the same bytes.
//!
//! It uses **fast key erasure** (the OpenBSD/Linux `chacha_rng` design):
//! each refill computes 256 bytes of keystream, immediately overwrites the
//! key with the first 32 and hands out only the remaining 224.  The key that
//! produced already-returned bytes therefore no longer exists anywhere, so
//! an attacker who later reads the pool cannot roll it backwards to recover
//! output the process has already used.
//!
//! After `fork()` the child's pool is a byte-for-byte copy of the parent's
//! and would emit the identical stream, so [`reseed_after_fork`] bumps a
//! process-wide generation counter that invalidates every thread's pool.
//! Checking the counter costs one relaxed atomic load per call, which is
//! why it is a counter rather than a `getpid()` comparison.

// Fixed-size `[u32; 16]` / `[u8; 64]` arrays indexed by compile-time
// constants — the ChaCha round function is written the way the RFC states
// it, and `get()` on every word would obscure it without adding a check
// that can ever fire.
#![allow(clippy::indexing_slicing)]
// ChaCha is defined over modular arithmetic; the additions below are
// deliberately wrapping and are written as such.
#![allow(clippy::arithmetic_side_effects)]

use core::sync::atomic::{AtomicU32, Ordering};

use crate::syscall::{SYS_GETRANDOM, syscall3};

/// Bytes of keystream produced per refill (four ChaCha20 blocks).
const REFILL: usize = 256;

/// Bytes of each refill consumed by re-keying, i.e. never handed out.
const KEY_BYTES: usize = 32;

/// Usable bytes per refill.  The 32-byte shortfall is the cost of fast key
/// erasure, and 224/256 makes it a 12.5% overhead rather than the 50% a
/// single-block refill would pay.
const POOL_BYTES: usize = REFILL - KEY_BYTES;

/// Process-wide pool generation.
///
/// A thread's pool is stale when its recorded generation differs from this.
/// Starts at 1 so that a freshly-zeroed [`RandomState`] (generation 0, per
/// the [`crate::perthread`] all-zero invariant) is stale and seeds on first
/// use without needing a separate "initialised" flag.
static GENERATION: AtomicU32 = AtomicU32::new(1);

// ---------------------------------------------------------------------------
// ChaCha20 core
// ---------------------------------------------------------------------------

/// `"expand 32-byte k"` — the ChaCha constant, as four little-endian words.
const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646E, 0x7962_2D32, 0x6B20_6574];

/// One ChaCha quarter round on `state`.
#[inline(always)]
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] = (state[d] ^ state[a]).rotate_left(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_left(12);
    state[a] = state[a].wrapping_add(state[b]);
    state[d] = (state[d] ^ state[a]).rotate_left(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_left(7);
}

/// The ChaCha20 block function proper: 20 rounds over `init`, then add
/// `init` back and serialise little-endian.
///
/// Split out from [`chacha20_block`] so the known-answer test can supply the
/// full 16-word state that RFC 8439 §2.3.2 specifies, including the nonce
/// words that the key-erasure construction above never uses.
fn chacha20_core(init: &[u32; 16], out: &mut [u8; 64]) {
    let mut s = *init;

    // 20 rounds = 10 double rounds (column round then diagonal round).
    for _ in 0..10 {
        quarter_round(&mut s, 0, 4, 8, 12);
        quarter_round(&mut s, 1, 5, 9, 13);
        quarter_round(&mut s, 2, 6, 10, 14);
        quarter_round(&mut s, 3, 7, 11, 15);
        quarter_round(&mut s, 0, 5, 10, 15);
        quarter_round(&mut s, 1, 6, 11, 12);
        quarter_round(&mut s, 2, 7, 8, 13);
        quarter_round(&mut s, 3, 4, 9, 14);
    }

    for i in 0..16 {
        let bytes = s[i].wrapping_add(init[i]).to_le_bytes();
        let base = i * 4;
        out[base] = bytes[0];
        out[base + 1] = bytes[1];
        out[base + 2] = bytes[2];
        out[base + 3] = bytes[3];
    }
}

/// Produce one 64-byte ChaCha20 keystream block under `key`.
///
/// `counter` occupies word 12; words 13–15 (the 96-bit nonce in RFC 8439)
/// stay zero.  A nonce exists to separate several streams sharing one key —
/// here the key is destroyed after a single 256-byte refill, so there is no
/// second stream to separate and an all-zero nonce is not a reuse hazard.
fn chacha20_block(key: &[u32; 8], counter: u32, out: &mut [u8; 64]) {
    let init: [u32; 16] = [
        SIGMA[0], SIGMA[1], SIGMA[2], SIGMA[3], key[0], key[1], key[2], key[3], key[4], key[5],
        key[6], key[7], counter, 0, 0, 0,
    ];
    chacha20_core(&init, out);
}

// ---------------------------------------------------------------------------
// Entropy sources
// ---------------------------------------------------------------------------

/// The outcome of asking the kernel for bytes.
///
/// This is three-valued rather than a `bool` because "no kernel answered" and
/// "the kernel answered, and the answer was no" call for opposite responses,
/// and collapsing them is what made [`GRND_NONBLOCK`](crate::unistd::GRND_NONBLOCK)
/// unimplementable: a `-EAGAIN` that becomes `false` becomes `EIO`, and `EIO`
/// is not a value any caller retries on.
pub(crate) enum KernelFill {
    /// `out` is fully populated.
    Filled,
    /// There is no kernel to ask.  Try the hardware sources.
    ///
    /// Constructed only by the host build: [`classify_refusal`] keys this on
    /// `-ENOSYS`, which every `syscallN` returns when built for the host, and
    /// that check is `cfg`'d away on the OS target.  On the target there is
    /// always a kernel, so `dead_code` correctly observes that nothing
    /// constructs this — the `allow` records that as the intended invariant
    /// rather than an oversight.
    ///
    /// It is deliberately *not* `cfg`'d out of the enum. Removing the variant
    /// would make both matches on it exhaustive after two arms that always
    /// return, which turns the hardware fallback following each match into
    /// `unreachable_code` — trading one warning for two and scattering `cfg`
    /// through three functions to silence them.  Nor is the fix to let the
    /// target construct it: a target kernel that does not implement 90 is a
    /// *broken* kernel, and quietly serving `RDRAND` in its place is precisely
    /// what design-decisions.md §334 decided against.
    #[cfg_attr(target_os = "none", allow(dead_code))]
    Absent,
    /// The kernel is present and declined, reporting this errno.  Its answer
    /// is final — see [`secure_bytes`] for why we do not then try hardware.
    Refused(i32),
}

/// Fill `out` from the kernel CSPRNG, passing `flags` through as `GRND_*`.
///
/// The kernel caps a single call at 1 MiB, so a larger request loops.
///
/// # Why the third argument matters
///
/// This used to be `syscall2`, which declares only `rdi`/`rsi`; `rdx` — where
/// the kernel reads `arg2` — held whatever the compiler last left there.  The
/// kernel therefore could not begin honouring `GRND_*` on the native ABI
/// without every already-built binary starting to pass garbage flags, which is
/// why the switch to `syscall3` had to land here first and be rebuilt into the
/// `services/ctest-*` fixtures before the kernel side could follow.  See
/// `requests/a-b-getrandom-now-waits-for-a-credited-pool.md`.
fn kernel_fill(out: &mut [u8], flags: u32) -> KernelFill {
    let mut done: usize = 0;
    while done < out.len() {
        let Some(rest) = out.get_mut(done..) else {
            return KernelFill::Refused(crate::errno::EIO);
        };
        let ret = syscall3(
            SYS_GETRANDOM,
            rest.as_mut_ptr() as u64,
            rest.len() as u64,
            u64::from(flags),
        );
        if ret < 0 {
            return classify_refusal(ret);
        }
        if ret == 0 {
            // A zero-length request is already excluded by the loop condition,
            // so a zero return means the kernel made no progress on a non-empty
            // buffer.  Looping again would spin forever.
            return KernelFill::Refused(crate::errno::EIO);
        }
        // Clamp: a kernel that claimed to have written more than we asked for
        // would otherwise walk `done` past `out.len()` and, worse, make us
        // believe bytes we never received are random.
        #[allow(clippy::cast_sign_loss)] // `ret > 0` checked above.
        let n = (ret as usize).min(rest.len());
        done = done.saturating_add(n);
    }
    KernelFill::Filled
}

/// Turn a negative `SYS_GETRANDOM` return into [`KernelFill`].
///
/// On the host build every `syscallN` returns `-ENOSYS`, which is the one
/// value meaning "there is no kernel here".  It is safe to key on: `-38` sits
/// in none of the bands `errno::native` assigns (`-1..=-9`, `-100`, `-200`,
/// `-300`, `-400`, `-500`, `-600`), so no real kernel answer can collide with
/// it.  On the OS target there is always a kernel, so a refusal is always a
/// genuine refusal — a kernel that does not implement 90 is a broken kernel,
/// and quietly substituting `RDRAND` for it would hide that.
fn classify_refusal(ret: i64) -> KernelFill {
    #[cfg(not(target_os = "none"))]
    if ret == -i64::from(crate::errno::ENOSYS) {
        return KernelFill::Absent;
    }

    // Reuse `errno::translate` rather than restating the table: it is the one
    // place the kernel's native codes are mapped, and a second copy here would
    // be a copy that drifts.  It sets errno as its side effect, so read it
    // back rather than duplicating the match.
    let _ = crate::errno::translate(ret);
    let err = crate::errno::get_errno();

    // One deliberate override.  `KernelError::TimedOut` — the pool was never
    // credited within the kernel's bounded wait — translates to `ETIMEDOUT`,
    // which is honest but which no portable caller tests for.  `getentropy(3)`
    // is *specified* to report `EIO` when it cannot fill the buffer, and
    // `getrandom` shares this path, so both report the value the standard
    // names.  Every other code passes through untouched, which is what keeps
    // `WOULD_BLOCK` → `EAGAIN` intact for `GRND_NONBLOCK`.
    if err == crate::errno::ETIMEDOUT {
        return KernelFill::Refused(crate::errno::EIO);
    }
    KernelFill::Refused(err)
}

/// Whether this CPU has `RDSEED` / `RDRAND`, resolved once via `CPUID`.
///
/// 0 = not yet probed, 1 = neither, 2 = RDRAND only, 3 = RDSEED and RDRAND.
/// Executing `RDRAND` on a CPU that lacks it raises `#UD`, so the probe is
/// not optional — the previous implementation issued it unconditionally.
static HW_RNG: AtomicU32 = AtomicU32::new(0);

const HW_NONE: u32 = 1;
const HW_RDRAND: u32 = 2;
const HW_RDSEED: u32 = 3;

#[cfg(target_arch = "x86_64")]
fn hw_rng_kind() -> u32 {
    let cached = HW_RNG.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }

    // `CPUID` is unprivileged and architecturally present on every x86_64
    // CPU, so `__cpuid`/`__cpuid_count` are safe functions; they handle the
    // RBX save/restore that LLVM requires.
    let has_rdrand = (core::arch::x86_64::__cpuid(1).ecx & (1 << 30)) != 0;

    // Leaf 7 only exists if leaf 0 reports a max leaf of at least 7.  Asking
    // for a leaf above the maximum returns the *highest* supported leaf's
    // data rather than zeroes, so skipping this check would read some
    // unrelated leaf's bit 18 as "has RDSEED".
    let max_leaf = core::arch::x86_64::__cpuid(0).eax;
    let has_rdseed =
        max_leaf >= 7 && (core::arch::x86_64::__cpuid_count(7, 0).ebx & (1 << 18)) != 0;

    let kind = if has_rdseed && has_rdrand {
        HW_RDSEED
    } else if has_rdrand {
        HW_RDRAND
    } else {
        HW_NONE
    };
    HW_RNG.store(kind, Ordering::Relaxed);
    kind
}

#[cfg(not(target_arch = "x86_64"))]
fn hw_rng_kind() -> u32 {
    HW_NONE
}

/// One 64-bit draw from `RDSEED` (true entropy) or `RDRAND` (a
/// hardware-seeded DRBG), retrying the documented number of times.
///
/// Intel specifies that `RDSEED` may legitimately fail under load while the
/// entropy pool refills, and recommends bounded retries; 32 is more than the
/// 10 Intel suggests for `RDRAND` and cheap next to failing outright.
#[cfg(target_arch = "x86_64")]
fn hw_word() -> Option<u64> {
    let kind = hw_rng_kind();
    if kind == HW_NONE {
        return None;
    }
    for _ in 0..32 {
        let val: u64;
        let ok: u8;
        if kind == HW_RDSEED {
            // SAFETY: `RDSEED` is present — `hw_rng_kind` checked CPUID leaf
            // 7 EBX bit 18.  It touches no memory and only writes `val` and
            // the carry flag.
            unsafe {
                core::arch::asm!(
                    "rdseed {val}",
                    "setc {ok}",
                    val = out(reg) val,
                    ok = out(reg_byte) ok,
                    options(nostack, nomem),
                );
            }
        } else {
            // SAFETY: `RDRAND` is present — `hw_rng_kind` checked CPUID leaf
            // 1 ECX bit 30.  Same no-memory contract as above.
            unsafe {
                core::arch::asm!(
                    "rdrand {val}",
                    "setc {ok}",
                    val = out(reg) val,
                    ok = out(reg_byte) ok,
                    options(nostack, nomem),
                );
            }
        }
        if ok != 0 {
            return Some(val);
        }
    }
    None
}

#[cfg(not(target_arch = "x86_64"))]
fn hw_word() -> Option<u64> {
    None
}

/// Draw 32 bytes of seed material for the local pool.
///
/// Kernel first; hardware second.  Returns `false` if neither answered, in
/// which case `key` is untouched and the caller must fail rather than run on
/// a guessable key.
///
/// Flags are `0`: this key seeds the `arc4random` pool, which has no error
/// channel and no way to tell a caller "these bytes are provisional", so it
/// must **wait** for a credited pool rather than take whatever is available.
/// Before the kernel gained its readiness gate that distinction did not exist;
/// now, asking with `GRND_INSECURE` here would key every process's pool from
/// material that correlates across boots of the same image.
fn seed_material(key: &mut [u8; KEY_BYTES]) -> bool {
    match kernel_fill(key, 0) {
        KernelFill::Filled => return true,
        // The kernel is there and said no.  Its answer is final for the same
        // reason as in `secure_bytes`.
        KernelFill::Refused(_) => return false,
        KernelFill::Absent => {}
    }
    let mut scratch = [0u8; KEY_BYTES];
    for chunk in scratch.chunks_mut(8) {
        let Some(word) = hw_word() else {
            return false;
        };
        for (dst, src) in chunk.iter_mut().zip(word.to_le_bytes()) {
            *dst = src;
        }
    }
    *key = scratch;
    true
}

// ---------------------------------------------------------------------------
// Per-thread pool
// ---------------------------------------------------------------------------

/// A thread's `arc4random` ChaCha20 pool.
///
/// Lives in [`crate::perthread::PerThread`] and so must be valid when
/// all-zero: generation 0 is "stale" (see [`GENERATION`]), and `avail` 0 is
/// "no buffered bytes", so a zeroed pool correctly seeds on first use.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RandomState {
    /// Current ChaCha20 key, replaced from its own keystream every refill.
    key: [u32; 8],
    /// Keystream not yet handed out.  Bytes are consumed from the front of
    /// `buf[POOL_BYTES - avail ..]`.
    buf: [u8; POOL_BYTES],
    /// Unconsumed bytes remaining in `buf`.
    avail: usize,
    /// [`GENERATION`] value this pool was seeded at; 0 means never seeded.
    generation: u32,
}

impl RandomState {
    /// The all-zero initial state required by [`crate::perthread`].
    pub const ZERO: Self = Self {
        key: [0; 8],
        buf: [0; POOL_BYTES],
        avail: 0,
        generation: 0,
    };
}

/// Refill `buf` with 224 fresh bytes and replace the key with the other 32.
///
/// There is no persistent block counter: every refill starts from counter 0
/// under a brand-new key, so the four blocks of one refill are the only
/// keystream that key ever produces.
fn refill(pool: &mut RandomState) {
    let mut block = [0u8; 64];
    let mut stream = [0u8; REFILL];
    for (i, out) in stream.chunks_mut(64).enumerate() {
        // `i` is 0..4, so the cast is exact.
        #[allow(clippy::cast_possible_truncation)]
        chacha20_block(&pool.key, i as u32, &mut block);
        out.copy_from_slice(&block);
    }

    // Re-key *before* publishing the output: after this the key that produced
    // `stream` no longer exists, so reading the pool later cannot reconstruct
    // bytes already handed out.
    for (i, word) in pool.key.iter_mut().enumerate() {
        let base = i * 4;
        *word = u32::from_le_bytes([
            stream[base],
            stream[base + 1],
            stream[base + 2],
            stream[base + 3],
        ]);
    }

    pool.buf.copy_from_slice(&stream[KEY_BYTES..]);
    pool.avail = POOL_BYTES;
    // The keystream copy still holds the new key in its first 32 bytes.
    stream.fill(0);
    block.fill(0);
}

/// Fill `out` from the calling thread's pool, seeding or re-seeding it first
/// if it has never been used or was inherited across a `fork`.
///
/// Returns `false` only when no entropy source could be reached, in which
/// case `out` is left untouched.
fn pool_fill(out: &mut [u8]) -> bool {
    // SAFETY: `perthread::current()` returns a pointer to the calling
    // thread's own block, which is live for the whole thread and reachable
    // from no other thread, so the exclusive borrow below is sound and
    // needs no lock.
    let pool: &mut RandomState = unsafe { &mut (*crate::perthread::current()).random };

    let generation = GENERATION.load(Ordering::Relaxed);
    if pool.generation != generation {
        let mut key = [0u8; KEY_BYTES];
        if !seed_material(&mut key) {
            return false;
        }
        for (i, word) in pool.key.iter_mut().enumerate() {
            let base = i * 4;
            *word = u32::from_le_bytes([key[base], key[base + 1], key[base + 2], key[base + 3]]);
        }
        key.fill(0);
        pool.avail = 0;
        pool.generation = generation;
    }

    let mut done: usize = 0;
    while done < out.len() {
        if pool.avail == 0 {
            refill(pool);
        }
        let start = POOL_BYTES.saturating_sub(pool.avail);
        let want = out.len().saturating_sub(done).min(pool.avail);
        let (Some(dst), Some(src)) = (
            out.get_mut(done..done.saturating_add(want)),
            pool.buf.get_mut(start..start.saturating_add(want)),
        ) else {
            return false;
        };
        dst.copy_from_slice(src);
        // Wipe as we go: a byte already handed to the caller must not sit in
        // the pool where a later leak could expose it a second time.
        src.fill(0);
        pool.avail = pool.avail.saturating_sub(want);
        done = done.saturating_add(want);
    }
    true
}

/// Invalidate every thread's pool.
///
/// Called from the child side of [`fork`](crate::process::fork): the child's
/// copy of the pool would otherwise replay the parent's stream byte for byte,
/// so a parent and child that both generate a "random" session key would
/// generate the *same* one.
pub(crate) fn reseed_after_fork() {
    // Wrapping, and skipping 0: 0 is the sentinel for "never seeded", so a
    // pool must never legitimately record it.
    let next = GENERATION.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if next == 0 {
        GENERATION.store(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Internal API used by getrandom/getentropy
// ---------------------------------------------------------------------------

/// Fill `out` with cryptographically-secure bytes.
///
/// `flags` are the caller's `GRND_*` bits, passed to the kernel unchanged.
///
/// Returns `Err(errno)` if no bytes could be obtained, in which case `out`
/// holds unspecified contents (the kernel may have filled part of it before
/// failing) and must not be used.  Callers must surface that as an error;
/// there is deliberately no "best effort" path.
///
/// This goes to the kernel directly rather than through the `arc4random`
/// pool, matching Linux's `getrandom(2)`: a caller asking for key material
/// by name gets it from the kernel's pool, not from a userspace copy that
/// outlives the call in this process's memory.
///
/// # Why a refusal is not retried against hardware
///
/// The hardware path exists for one situation: there is no kernel to ask.
/// When the kernel *is* present and declines, falling through to `RDRAND`
/// would make the decline unobservable — `GRND_NONBLOCK` would report
/// `EAGAIN` on a machine without `RDRAND` and quietly succeed on one with it,
/// so the same program would take different branches on two machines for a
/// reason it cannot see.  That is worse than either outcome consistently.
/// The case barely arises in any event: the kernel credits its pool from
/// `RDSEED`/`RDRAND` at init, so a machine that could serve the fallback is
/// a machine whose pool was credited before userspace started.
pub(crate) fn secure_bytes(out: &mut [u8], flags: u32) -> Result<(), i32> {
    match kernel_fill(out, flags) {
        KernelFill::Filled => Ok(()),
        KernelFill::Refused(err) => Err(err),
        KernelFill::Absent => {
            if pool_fill(out) {
                Ok(())
            } else {
                Err(crate::errno::EIO)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// arc4random — BSD API
// ---------------------------------------------------------------------------

/// Fill the pool or die.
///
/// The `arc4random` family has no error channel — the whole point of the API
/// is that it cannot fail — so a process that cannot obtain randomness has
/// no correct way to continue.  OpenBSD handles this the same way (it raises
/// `SIGKILL`); returning zeroes or clock bits would hand the caller a key it
/// believes is secret.
fn must_fill(out: &mut [u8]) {
    if !pool_fill(out) {
        crate::unistd::abort();
    }
}

/// Return a uniformly distributed random `u32`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn arc4random() -> u32 {
    let mut buf = [0u8; 4];
    must_fill(&mut buf);
    u32::from_le_bytes(buf)
}

/// Fill `buf` with `nbytes` random bytes.
///
/// # Safety
///
/// `buf` must be valid for writes of `nbytes` bytes, or `nbytes` must be 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn arc4random_buf(buf: *mut u8, nbytes: usize) {
    if nbytes == 0 {
        // A zero-length request is a no-op even for a null pointer, which is
        // what callers looping over a possibly-empty buffer expect.
        return;
    }
    if buf.is_null() {
        // No error channel to report through, and continuing would write
        // through null.  This is a caller bug, so fail loudly.
        crate::unistd::abort();
    }
    // SAFETY: the caller guarantees `buf` is writable for `nbytes`, and it is
    // non-null and `nbytes` nonzero by the checks above.
    let out = unsafe { core::slice::from_raw_parts_mut(buf, nbytes) };
    must_fill(out);
}

/// Return a uniformly distributed random value in `[0, upper_bound)`.
///
/// Uniform, not merely "random mod n": the naive `arc4random() % n` is biased
/// towards small values whenever `n` does not divide 2^32, and the bias is
/// large enough to matter for `n` anywhere near 2^32.  This uses OpenBSD's
/// rejection loop — discard draws below `2^32 mod n`, which leaves an
/// interval whose length is an exact multiple of `n`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn arc4random_uniform(upper_bound: u32) -> u32 {
    if upper_bound < 2 {
        // 0 and 1 both have exactly one legal answer; without this the loop
        // below would never terminate for 0.
        return 0;
    }

    // `2^32 % upper_bound`, computed without 64-bit arithmetic exactly as
    // OpenBSD does: `-upper_bound` as a u32 is `2^32 - upper_bound`.
    let min = upper_bound.wrapping_neg() % upper_bound;

    loop {
        let r = arc4random();
        if r >= min {
            return r % upper_bound;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: the failure paths (`must_fill` aborting, `secure_bytes` returning
    // false) cannot be exercised here — they call `_exit`, and there is no way
    // to make CPUID stop reporting RDRAND on the test host.  The host build
    // reaches them only on a CPU with neither RDSEED nor RDRAND, which no
    // x86_64 part sold since 2012 is.

    // -- ChaCha20 core --

    #[test]
    fn test_chacha20_rfc8439_known_answer() {
        // RFC 8439 §2.3.2.  Key = 00 01 02 … 1f, nonce =
        // 00:00:00:09:00:00:00:4a:00:00:00:00, block counter = 1.  The nonce
        // is nonzero, which is why this goes through `chacha20_core` — the
        // pool's own `chacha20_block` pins the nonce to zero.  A known-answer
        // test is the only thing that catches a transposed quarter-round
        // index or a wrong rotation constant: every such bug still produces
        // convincing-looking random bytes.
        let init: [u32; 16] = [
            0x6170_7865,
            0x3320_646e,
            0x7962_2d32,
            0x6b20_6574,
            0x0302_0100,
            0x0706_0504,
            0x0b0a_0908,
            0x0f0e_0d0c,
            0x1312_1110,
            0x1716_1514,
            0x1b1a_1918,
            0x1f1e_1d1c,
            0x0000_0001,
            0x0900_0000,
            0x4a00_0000,
            0x0000_0000,
        ];
        let mut out = [0u8; 64];
        chacha20_core(&init, &mut out);
        let expected: [u8; 64] = [
            0x10, 0xf1, 0xe7, 0xe4, 0xd1, 0x3b, 0x59, 0x15, 0x50, 0x0f, 0xdd, 0x1f, 0xa3, 0x20,
            0x71, 0xc4, 0xc7, 0xd1, 0xf4, 0xc7, 0x33, 0xc0, 0x68, 0x03, 0x04, 0x22, 0xaa, 0x9a,
            0xc3, 0xd4, 0x6c, 0x4e, 0xd2, 0x82, 0x64, 0x46, 0x07, 0x9f, 0xaa, 0x09, 0x14, 0xc2,
            0xd7, 0x05, 0xd9, 0x8b, 0x02, 0xa2, 0xb5, 0x12, 0x9c, 0xd1, 0xde, 0x16, 0x4e, 0xb9,
            0xcb, 0xd0, 0x83, 0xe8, 0xa2, 0x50, 0x3c, 0x4e,
        ];
        assert_eq!(out, expected);
    }

    #[test]
    fn test_chacha20_block_is_deterministic() {
        let key = [1u32, 2, 3, 4, 5, 6, 7, 8];
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        chacha20_block(&key, 7, &mut a);
        chacha20_block(&key, 7, &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn test_chacha20_counter_changes_output() {
        let key = [1u32, 2, 3, 4, 5, 6, 7, 8];
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        chacha20_block(&key, 0, &mut a);
        chacha20_block(&key, 1, &mut b);
        assert_ne!(a, b);
    }

    #[test]
    fn test_chacha20_key_changes_output() {
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        chacha20_block(&[1u32, 2, 3, 4, 5, 6, 7, 8], 0, &mut a);
        chacha20_block(&[1u32, 2, 3, 4, 5, 6, 7, 9], 0, &mut b);
        assert_ne!(a, b);
    }

    // -- pool --

    #[test]
    fn test_secure_bytes_succeeds_on_host() {
        // The host build has no kernel syscall, so this exercises the
        // RDRAND-seeded pool path end to end.
        let mut buf = [0u8; 64];
        assert!(secure_bytes(&mut buf, 0).is_ok());
        assert_ne!(buf, [0u8; 64]);
    }

    #[test]
    fn test_successive_fills_differ() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        assert!(secure_bytes(&mut a, 0).is_ok());
        assert!(secure_bytes(&mut b, 0).is_ok());
        assert_ne!(a, b);
    }

    #[test]
    fn test_fill_spanning_multiple_refills() {
        // Larger than POOL_BYTES, so the pool must re-key mid-request and the
        // seam must not repeat: a bug that failed to advance the key would
        // make byte i and byte i + POOL_BYTES identical.
        let mut buf = [0u8; POOL_BYTES * 3 + 17];
        assert!(secure_bytes(&mut buf, 0).is_ok());
        let (first, rest) = buf.split_at(POOL_BYTES);
        assert_ne!(first, &rest[..POOL_BYTES]);
    }

    #[test]
    fn test_zero_length_fill_is_ok() {
        let mut buf: [u8; 0] = [];
        assert!(secure_bytes(&mut buf, 0).is_ok());
    }

    #[test]
    fn test_output_is_not_all_one_byte() {
        // The old LCG emitted `(state >> 56)`, which is fine on this test but
        // a stuck source would not be; cheap sanity check that we are not
        // returning a constant.
        let mut buf = [0u8; 256];
        assert!(secure_bytes(&mut buf, 0).is_ok());
        let first = buf[0];
        assert!(buf.iter().any(|&b| b != first));
    }

    #[test]
    fn test_reseed_after_fork_changes_the_stream() {
        // Not a real fork — this asserts the mechanism: bumping the
        // generation must make the next draw come from a fresh key rather
        // than continuing the buffered keystream.
        let mut before = [0u8; POOL_BYTES];
        assert!(pool_fill(&mut before));
        reseed_after_fork();
        let mut after = [0u8; POOL_BYTES];
        assert!(pool_fill(&mut after));
        assert_ne!(before, after);
    }

    #[test]
    fn test_generation_never_lands_on_zero_sentinel() {
        // Walk the counter to just below the wrap and step over it.
        GENERATION.store(u32::MAX, Ordering::Relaxed);
        reseed_after_fork();
        assert_ne!(GENERATION.load(Ordering::Relaxed), 0);
        // Leave the counter somewhere sane for any test that runs after this
        // one on the same thread.
        GENERATION.store(1, Ordering::Relaxed);
    }

    // -- arc4random --

    #[test]
    fn test_arc4random_varies() {
        let a = arc4random();
        let b = arc4random();
        let c = arc4random();
        assert!(a != b || b != c);
    }

    #[test]
    fn test_arc4random_buf_fills() {
        let mut buf = [0u8; 48];
        unsafe { arc4random_buf(buf.as_mut_ptr(), buf.len()) };
        assert_ne!(buf, [0u8; 48]);
    }

    #[test]
    fn test_arc4random_buf_zero_length_is_noop_even_for_null() {
        unsafe { arc4random_buf(core::ptr::null_mut(), 0) };
    }

    #[test]
    fn test_arc4random_buf_does_not_overrun() {
        let mut buf = [0u8; 16];
        unsafe { arc4random_buf(buf.as_mut_ptr(), 8) };
        assert_eq!(&buf[8..], &[0u8; 8]);
    }

    #[test]
    fn test_arc4random_uniform_zero_and_one() {
        assert_eq!(arc4random_uniform(0), 0);
        assert_eq!(arc4random_uniform(1), 0);
    }

    #[test]
    fn test_arc4random_uniform_in_range() {
        for _ in 0..1000 {
            assert!(arc4random_uniform(10) < 10);
        }
    }

    #[test]
    fn test_arc4random_uniform_covers_the_range() {
        // With 2000 draws over 4 values, missing one is a ~1-in-10^250 event,
        // so a failure here means the generator is stuck or the modulus is
        // wrong — not bad luck.
        let mut seen = [false; 4];
        for _ in 0..2000 {
            let v = arc4random_uniform(4) as usize;
            assert!(v < 4);
            seen[v] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn test_arc4random_uniform_min_is_the_bias_cutoff() {
        // The rejection threshold must be `2^32 mod n`.  Check the identity
        // the implementation relies on against 64-bit arithmetic.
        for n in [2u32, 3, 7, 100, 0xFFFF, 0x8000_0001, u32::MAX] {
            let expected = ((1u64 << 32) % u64::from(n)) as u32;
            assert_eq!(n.wrapping_neg() % n, expected, "n = {n}");
        }
    }

    #[test]
    fn test_arc4random_uniform_large_bound() {
        // `upper_bound` above 2^31 makes `min` large, so the rejection loop
        // runs often; it must still terminate and stay in range.
        for _ in 0..200 {
            assert!(arc4random_uniform(0x8000_0001) < 0x8000_0001);
        }
    }

    // -- entropy sources --

    #[test]
    fn test_hw_rng_kind_is_cached_and_stable() {
        let a = hw_rng_kind();
        let b = hw_rng_kind();
        assert_eq!(a, b);
        assert!(a == HW_NONE || a == HW_RDRAND || a == HW_RDSEED);
    }

    #[test]
    fn test_kernel_fill_reports_absent_on_host() {
        // The raw SYSCALL instruction is gated off on host builds, so this
        // must report failure rather than silently "succeed" with a buffer
        // the kernel never touched.
        //
        // It must report it as `Absent` specifically, not as a refusal: that
        // is the discriminator the hardware fallback hangs off, so a
        // misclassification here would either strand the host build with no
        // entropy source at all (`Refused` → no fallback) or, on the target,
        // silently paper over a kernel that declined.
        let mut buf = [0u8; 8];
        assert!(matches!(kernel_fill(&mut buf, 0), KernelFill::Absent));
        assert_eq!(buf, [0u8; 8]);
    }

    #[test]
    fn test_host_sentinel_cannot_collide_with_a_kernel_code() {
        // `classify_refusal` keys "there is no kernel" on `-ENOSYS`, which is
        // only sound while no native error code shares that number. The bands
        // in `errno::native` are -1..=-9, -100..=-103, -200..=-203, -300..=-304,
        // -400..=-401, -500..=-511 and -600..=-602; -38 is in none of them.
        // If a future band ever reaches it, this fires before the entropy path
        // starts silently taking the fallback on a live kernel refusal.
        let sentinel = -i64::from(crate::errno::ENOSYS);
        assert_eq!(sentinel, -38);
        let mut occupied = (-9..=-1)
            .chain(-103..=-100)
            .chain(-203..=-200)
            .chain(-304..=-300)
            .chain(-401..=-400)
            .chain(-511..=-500)
            .chain(-602..=-600);
        assert!(!occupied.any(|c| c == sentinel));
    }

    #[test]
    fn test_seed_material_succeeds_on_host() {
        let mut key = [0u8; KEY_BYTES];
        assert!(seed_material(&mut key));
        assert_ne!(key, [0u8; KEY_BYTES]);
    }

    #[test]
    fn test_random_state_zero_is_all_zero_bytes() {
        // Pins the `perthread` all-zero invariant: a fresh thread's block is
        // carved out of untouched anonymous memory and never initialised.
        let zero = RandomState::ZERO;
        // SAFETY: `RandomState` is `repr(C)` and holds only integers and
        // arrays of integers, so every byte of it is initialised and reading
        // it as bytes is well-defined.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(&zero).cast::<u8>(),
                core::mem::size_of::<RandomState>(),
            )
        };
        assert!(bytes.iter().all(|&b| b == 0));
    }
}
