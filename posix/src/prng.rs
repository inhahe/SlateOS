//! The C library's pseudo-random number generators -- `rand`, `random` and
//! the `rand48` family, and their reentrant forms: the deterministic ones,
//! which repeat a sequence from a seed. (`arc4random`, which draws from the
//! kernel's entropy and repeats nothing, is [`crate::random`].)
//!
//! Written from POSIX.1-2024 and the published algorithms; glibc 2.39 is run
//! only for its answers, which the tests replay
//! (`posix/tools/oracle/random_harness.py`, `random_oracle.txt`), so that a
//! program seeded here draws what it draws on Linux.
//!
//! # `random` and `rand`
//!
//! POSIX specifies `random` as "a non-linear additive feedback random-number
//! generator employing a default state array size of 31 long integers";
//! `initstate` picks the generator by the size of the array it is given --
//! 8, 32, 64, 128 or 256 bytes, other sizes rounded down -- and `setstate`
//! switches between arrays. The generator is 4.3BSD's, which every Unix since
//! has kept. What makes its sequences glibc's:
//!
//! * A generator of degree `d` and separation `s` is `d` 32-bit words. An
//!   output adds the *rear* word into the *front* one, `s` words ahead of it,
//!   wrapping at 32 bits; returns that sum less its lowest bit; and moves both
//!   on a word, back to the first after the last.
//! * Seeding makes word 0 the seed (1 for a seed of 0) and each next word
//!   16807 times the one before modulo 2^31 - 1 -- Park and Miller's "minimal
//!   standard" generator, by Schrage's method, the seed read as a signed
//!   32-bit number -- then draws and discards `10 d` outputs.
//! * The 8-byte array is degree 0, a linear congruential generator instead:
//!   `x = (1103515245 x + 12345) mod 2^31`, returning `x`.
//! * The array's first word records which generator it holds and where that
//!   stood -- 5 times the rear word's index, plus the generator's number --
//!   so that `setstate` takes it up where it was left.
//!
//! `rand` is `random` and `srand` is `srandom`, as glibc's are, so a seed
//! gives both one sequence. Both take one lock: POSIX requires `random` to be
//! thread-safe, and `rand`, which need not be, to avoid data races with it.
//! `fork` holds the lock across the system call (`lock_for_fork`), so a
//! child is never left a generator half-stepped by a thread it does not have.
//!
//! # The `rand48` family
//!
//! POSIX gives the algorithm outright -- `X = (a X + c) mod 2^48`, with
//! `a = 0x5DEECE66D` and `c = 0xB` until `lcong48` sets others -- and each
//! function's conversion of `X`. The process's `X` starts at 0, as glibc's
//! does.
//!
//! Only `drand48`, `lrand48` and `mrand48` are exempt from thread safety; the
//! three initializers and the three that step a caller's own `X` are not,
//! though glibc's lock none of them. So here `X` and the parameters are
//! atomics -- `a` and `c` in one word, so that nothing reads `a` from one
//! `lcong48` and `c` from another -- and the initializers take a lock among
//! themselves. The three exempt functions stay unlocked, as glibc's are: two
//! threads drawing at once may draw one number twice, which POSIX allows,
//! where a lock would make every draw contend in a program that shares one
//! generator among its threads. `seed48`'s copy of the `X` it replaced is the
//! calling thread's own, so another thread's `seed48` cannot overwrite it
//! while it is being read.
//!
//! # The reentrant forms
//!
//! glibc's: `random_r` and its kin over a `struct random_data`, and
//! `drand48_r` and its kin over a `struct drand48_data`, laid out as glibc's
//! (`posix/include/stdlib.h`, held to glibc's by
//! `scripts/check-libc-overlay.py`). A `struct drand48_data` only zeroed is a
//! generator with `X = 0` and POSIX's parameters, as glibc documents.
//!
//! The `random_r` four are an archive member of their own, as
//! `scripts/check-libc-shape.py` requires of a name gnulib defines where the
//! C library lacks it -- its `random_r` module does, under the plain names,
//! wherever musl is what `./configure` probes. The `drand48_r` nine have one
//! too, for the same hazard from a program that brings its own.

use core::cell::UnsafeCell;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::errno::{EINVAL, set_errno};
use crate::lowlevellock::{lll_lock, lll_unlock};

/// The largest number `rand` returns, as `<stdlib.h>`'s `RAND_MAX` says.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static RAND_MAX: i32 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// random, rand, and the random_r family's generator
// ---------------------------------------------------------------------------

/// glibc's `struct random_data`: one generator of the `random_r` family, over
/// an array of the caller's that [`initstate_r`] lays out -- a word of
/// bookkeeping, then the generator's words -- and that must outlive its use.
#[repr(C)]
#[derive(Debug)]
pub struct RandomData {
    /// The front word, which the next output adds the rear one into.
    pub fptr: *mut i32,
    /// The rear word, `rand_sep` words behind the front one.
    pub rptr: *mut i32,
    /// The generator's first word; the bookkeeping word is the one before.
    pub state: *mut i32,
    /// Which of the five generators: 0 to 4, by the array's size.
    pub rand_type: i32,
    /// Its degree: how many words it has.
    pub rand_deg: i32,
    /// Its separation.
    pub rand_sep: i32,
    /// One past its last word.
    pub end_ptr: *mut i32,
}

impl RandomData {
    /// Set up by nothing yet -- what a zeroed `struct random_data` is.
    pub const UNSET: Self = Self {
        fptr: null_mut(),
        rptr: null_mut(),
        state: null_mut(),
        rand_type: 0,
        rand_deg: 0,
        rand_sep: 0,
        end_ptr: null_mut(),
    };
}

/// One of the five generators `initstate` chooses among.
struct Generator {
    /// The least array, in bytes, that takes it -- the bookkeeping word
    /// included. POSIX names these sizes.
    least: usize,
    /// How many words it has; 0 for the linear congruential one.
    degree: i32,
    /// How far the front word is ahead of the rear one.
    separation: i32,
}

/// The five, by array size: the degrees and separations are 4.3BSD's.
const GENERATORS: [Generator; 5] = [
    Generator {
        least: 8,
        degree: 0,
        separation: 0,
    },
    Generator {
        least: 32,
        degree: 7,
        separation: 3,
    },
    Generator {
        least: 64,
        degree: 15,
        separation: 1,
    },
    Generator {
        least: 128,
        degree: 31,
        separation: 3,
    },
    Generator {
        least: 256,
        degree: 63,
        separation: 1,
    },
];

/// How the bookkeeping word counts generators: `KINDS * rear + kind`.
const KINDS: i32 = 5;

/// `p` moved on a word, back to `first` after the one before `end`.
fn step_on(p: *mut i32, first: *mut i32, end: *mut i32) -> *mut i32 {
    let next = p.wrapping_add(1);
    if next >= end { first } else { next }
}

/// Record in the bookkeeping word of `d`'s array where its generator stands,
/// for `setstate` to take it up there again: the generator's number, plus,
/// for any but the linear congruential one, `KINDS` times the rear word's
/// index.
///
/// # Safety
///
/// `d` is set up: its pointers are into one live array.
unsafe fn record_position(d: &RandomData) {
    let word = if d.rand_type == 0 {
        0
    } else {
        // SAFETY: `rptr` and `state` are into the same array, by the contract.
        let rear = unsafe { d.rptr.offset_from(d.state) };
        // The rear word's index is below the degree, 63 at most, so neither
        // the conversion nor the arithmetic can overflow.
        (rear as i32).wrapping_mul(KINDS).wrapping_add(d.rand_type)
    };
    // SAFETY: the bookkeeping word, the one before `state`. The array is the
    // caller's `char` array, so its words need not be aligned.
    unsafe { d.state.wrapping_sub(1).write_unaligned(word) };
}

/// One output of `d`'s generator, advancing it.
///
/// # Safety
///
/// `d` is set up: its pointers are into one live array.
unsafe fn next_output(d: &mut RandomData) -> i32 {
    if d.rand_type == 0 {
        // SAFETY: the linear congruential generator's one word.
        let x = unsafe { d.state.read_unaligned() }.cast_unsigned();
        let x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345) & 0x7FFF_FFFF;
        // SAFETY: as above.
        unsafe { d.state.write_unaligned(x.cast_signed()) };
        return x.cast_signed();
    }
    // SAFETY: the front and rear words, both in the array by the contract.
    let sum = unsafe {
        let front = d.fptr.read_unaligned().cast_unsigned();
        let rear = d.rptr.read_unaligned().cast_unsigned();
        let sum = front.wrapping_add(rear);
        d.fptr.write_unaligned(sum.cast_signed());
        sum
    };
    d.fptr = step_on(d.fptr, d.state, d.end_ptr);
    d.rptr = step_on(d.rptr, d.state, d.end_ptr);
    (sum >> 1).cast_signed()
}

/// Seed `d`'s generator, as `srandom` and `srandom_r` do.
///
/// # Safety
///
/// `d` is set up: its pointers are into one live array.
unsafe fn seed_generator(d: &mut RandomData, seed: u32) {
    let seed = if seed == 0 { 1 } else { seed };
    // SAFETY: the generator's first word.
    unsafe { d.state.write_unaligned(seed.cast_signed()) };
    if d.rand_type == 0 {
        return;
    }
    let degree = usize::try_from(d.rand_deg).unwrap_or(0);
    // Each next word is 16807 times the one before, modulo 2^31 - 1, by
    // Schrage's method: 127773 and 2836 are the modulus's quotient and
    // remainder by 16807, so that with |lo| < 127773 and |hi| <= 16807 no
    // product or difference below leaves 32 bits. The seed is read as a
    // signed number, as glibc reads it: from 2^31 up, the first word is
    // negative.
    let mut word = seed.cast_signed();
    for i in 1..degree {
        let hi = word.wrapping_div(127_773);
        let lo = word.wrapping_rem(127_773);
        word = 16_807_i32
            .wrapping_mul(lo)
            .wrapping_sub(2_836_i32.wrapping_mul(hi));
        if word < 0 {
            word = word.wrapping_add(0x7FFF_FFFF);
        }
        // SAFETY: word `i` of the generator's `degree`.
        unsafe { d.state.add(i).write_unaligned(word) };
    }
    d.fptr = d
        .state
        .wrapping_add(usize::try_from(d.rand_sep).unwrap_or(0));
    d.rptr = d.state;
    for _ in 0..degree.wrapping_mul(10) {
        // SAFETY: set up just now.
        unsafe { next_output(d) };
    }
}

/// Set `d` up over the array `buf`, `n` bytes, as the generator that size
/// picks, seeded with `seed`: `initstate` and `initstate_r`. False for fewer
/// than 8 bytes, `d` then unchanged but for its old array's bookkeeping word,
/// which records where it stood either way.
///
/// # Safety
///
/// `buf` is `n` writable bytes that outlive `d`'s use of them; `d` is unset
/// or set up (its pointers into one live array).
unsafe fn init_generator(d: &mut RandomData, seed: u32, buf: *mut u8, n: usize) -> bool {
    if !d.state.is_null() {
        // SAFETY: set up, by the contract.
        unsafe { record_position(d) };
    }
    let Some((kind, g)) = GENERATORS
        .iter()
        .enumerate()
        .rev()
        .find(|(_, g)| n >= g.least)
    else {
        return false;
    };
    let state = buf.cast::<i32>().wrapping_add(1);
    // Fewer than 5 generators, so the number fits.
    d.rand_type = kind as i32;
    d.rand_deg = g.degree;
    d.rand_sep = g.separation;
    d.state = state;
    d.end_ptr = state.wrapping_add(usize::try_from(g.degree).unwrap_or(0));
    // The linear congruential generator has no front and rear words; point
    // them at its one word rather than leave them in the array it replaced.
    d.fptr = state;
    d.rptr = state;
    // SAFETY: laid out just now over the caller's `n` bytes.
    unsafe {
        seed_generator(d, seed);
        record_position(d);
    }
    true
}

/// Take up the generator in `buf`, an array `initstate` or `initstate_r` set
/// up, where it was left: `setstate` and `setstate_r`. False for an array
/// whose bookkeeping word no generator wrote -- `d` then unchanged but for
/// its old array's bookkeeping word, which records where it stood either way.
///
/// A rear word past the generator's last is refused as well, where glibc's
/// would read and write past the array.
///
/// # Safety
///
/// `buf` is such an array, alive while `d` uses it; `d` is unset or set up.
unsafe fn set_generator(d: &mut RandomData, buf: *mut u8) -> bool {
    if !d.state.is_null() {
        // SAFETY: set up, by the contract.
        unsafe { record_position(d) };
    }
    // SAFETY: the array's bookkeeping word, by the contract.
    let word = unsafe { buf.cast::<i32>().read_unaligned() };
    let (rear, kind) = (word.wrapping_div(KINDS), word.wrapping_rem(KINDS));
    let Some(g) = usize::try_from(kind).ok().and_then(|k| GENERATORS.get(k)) else {
        return false;
    };
    if g.degree != 0 && !(0..g.degree).contains(&rear) {
        return false;
    }
    let state = buf.cast::<i32>().wrapping_add(1);
    d.rand_type = kind;
    d.rand_deg = g.degree;
    d.rand_sep = g.separation;
    d.state = state;
    d.end_ptr = state.wrapping_add(usize::try_from(g.degree).unwrap_or(0));
    if g.degree == 0 {
        d.fptr = state;
        d.rptr = state;
    } else {
        // 0 <= rear < degree, checked above, and the separation is below the
        // degree too, so the front word's index is one subtraction from the
        // sum at most, and none of this overflows.
        let sum = rear.wrapping_add(g.separation);
        let front = if sum >= g.degree {
            sum.wrapping_sub(g.degree)
        } else {
            sum
        };
        d.rptr = state.wrapping_add(usize::try_from(rear).unwrap_or(0));
        d.fptr = state.wrapping_add(usize::try_from(front).unwrap_or(0));
    }
    true
}

/// The generator `random` and `rand` draw from, the array it starts in, and
/// the lock that serialises both.
struct Global {
    /// A `lowlevellock` word.
    lock: AtomicI32,
    /// The generator; [`RandomData::UNSET`] until first used.
    data: UnsafeCell<RandomData>,
    /// The array of POSIX's `initstate(1, state, 128)`, for the process that
    /// never gives one of its own.
    array: UnsafeCell<[i32; 32]>,
}

// SAFETY: `data` and `array` are touched only holding `lock`, by
// `with_global` -- and by `forget_global_for_test`, which holds it too.
unsafe impl Sync for Global {}

/// The process's `random` generator.
static GLOBAL: Global = Global {
    lock: AtomicI32::new(0),
    data: UnsafeCell::new(RandomData::UNSET),
    array: UnsafeCell::new([0; 32]),
};

/// Holds [`GLOBAL`]'s lock, and gives it up when dropped, on every path out.
struct Held;

impl Held {
    fn take() -> Self {
        lll_lock(&GLOBAL.lock);
        Self
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        lll_unlock(&GLOBAL.lock);
    }
}

/// Run `f` on the process's generator, holding its lock. The first time, it
/// is set up as by `initstate(1, state, 128)` over [`Global::array`]: "if
/// initstate() has not been called, then random() shall behave as though
/// initstate() had been called with seed=1 and size=128."
fn with_global<R>(f: impl FnOnce(&mut RandomData) -> R) -> R {
    let _held = Held::take();
    // SAFETY: the lock is held, so no other thread touches `data`; and `f` is
    // never one that calls back in here, so this is its only reference.
    let d = unsafe { &mut *GLOBAL.data.get() };
    if d.state.is_null() {
        // SAFETY: the array is 128 bytes, static, and touched only under the
        // lock; `d` is unset.
        let set_up = unsafe { init_generator(d, 1, GLOBAL.array.get().cast::<u8>(), 128) };
        debug_assert!(set_up, "128 bytes take the degree-31 generator");
    }
    f(d)
}

/// `random`: the process's generator's next number, 0 to 2^31 - 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn random() -> i64 {
    // SAFETY: `with_global` hands over a generator that is set up.
    i64::from(with_global(|d| unsafe { next_output(d) }))
}

/// `srandom`: start the process's generator's sequence for `seed` again.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn srandom(seed: u32) {
    // SAFETY: as `random`.
    with_global(|d| unsafe { seed_generator(d, seed) });
}

/// `initstate`: make `state`, `size` bytes, the process's generator's array
/// -- the generator that size picks, seeded with `seed` -- and return the
/// array it replaced. NULL, the generator unchanged, for a NULL array or
/// fewer than 8 bytes, as POSIX says.
///
/// # Safety
///
/// `state` is NULL or `size` writable bytes that outlive their use: `random`
/// and `rand` write to them until `initstate` or `setstate` gives another.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn initstate(seed: u32, state: *mut u8, size: usize) -> *mut u8 {
    if state.is_null() {
        set_errno(EINVAL);
        return null_mut();
    }
    with_global(|d| {
        let previous = d.state.wrapping_sub(1).cast::<u8>();
        // SAFETY: this function's contract, and a generator that is set up.
        if unsafe { init_generator(d, seed, state, size) } {
            previous
        } else {
            set_errno(EINVAL);
            null_mut()
        }
    })
}

/// `setstate`: make `state`, an array `initstate` set up, the process's
/// generator's array again, where it was left, and return the array it
/// replaced. NULL, the generator unchanged, for a NULL array or one whose
/// first word no generator wrote.
///
/// # Safety
///
/// `state` is NULL or such an array, alive while it is used, as for
/// [`initstate`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setstate(state: *mut u8) -> *mut u8 {
    if state.is_null() {
        set_errno(EINVAL);
        return null_mut();
    }
    with_global(|d| {
        let previous = d.state.wrapping_sub(1).cast::<u8>();
        // SAFETY: this function's contract, and a generator that is set up.
        if unsafe { set_generator(d, state) } {
            previous
        } else {
            set_errno(EINVAL);
            null_mut()
        }
    })
}

/// `rand`: `random`'s next number. glibc's `rand` is `random`, so the two
/// share one generator and one seed gives both one sequence.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rand() -> i32 {
    // SAFETY: as `random`.
    with_global(|d| unsafe { next_output(d) })
}

/// `srand`: `srandom`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn srand(seed: u32) {
    srandom(seed);
}

/// `rand_r`: a number from 0 to 2^31 - 1 from the generator whose whole state
/// is `*seed`, advancing it. glibc's: three steps of the linear congruential
/// generator of POSIX's example of a portable `rand`, `x = 1103515245 x +
/// 12345` modulo 2^32, taking `x / 65536 % 2048` from the first -- the high 11
/// bits of the result -- and `x / 65536 % 1024` from each of the others. A
/// NULL `seed` gives 0.
///
/// # Safety
///
/// `seed` is NULL or a readable and writable `unsigned int`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rand_r(seed: *mut u32) -> i32 {
    if seed.is_null() {
        return 0;
    }
    // SAFETY: the caller's `unsigned int`.
    let mut x = unsafe { seed.read() };
    let mut step = || {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        x >> 16
    };
    let high = step() & 0x7FF;
    let middle = step() & 0x3FF;
    let low = step() & 0x3FF;
    // SAFETY: as above.
    unsafe { seed.write(x) };
    ((high << 20) | (middle << 10) | low).cast_signed()
}

/// Own archive member -- gnulib's `random_r` module defines these four under
/// their own names where the C library lacks them, as musl does. See
/// string.rs's module header, and this one's.
mod gnu_random_r {
    use super::{RandomData, init_generator, next_output, seed_generator, set_generator};
    use crate::errno::{EINVAL, set_errno};

    /// -1, with `errno` EINVAL.
    fn invalid() -> i32 {
        set_errno(EINVAL);
        -1
    }

    /// `random_r`: the next number, 0 to 2^31 - 1, from the generator `buf`
    /// describes, into `*result`. 0; or -1, `errno` EINVAL, for a NULL
    /// pointer or a `buf` never set up.
    ///
    /// # Safety
    ///
    /// `buf` is NULL, zeroed, or set up by [`initstate_r`] or [`setstate_r`]
    /// over an array still alive; `result` is NULL or a writable `int32_t`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn random_r(buf: *mut RandomData, result: *mut i32) -> i32 {
        if buf.is_null() || result.is_null() {
            return invalid();
        }
        // SAFETY: the caller's, by the contract.
        let d = unsafe { &mut *buf };
        if d.state.is_null() {
            return invalid();
        }
        // SAFETY: set up, by the contract; `result` writable.
        unsafe { result.write(next_output(d)) };
        0
    }

    /// `srandom_r`: start the sequence of the generator `buf` describes for
    /// `seed` again. 0; or -1, `errno` EINVAL, as for [`random_r`].
    ///
    /// # Safety
    ///
    /// As [`random_r`]'s `buf`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn srandom_r(seed: u32, buf: *mut RandomData) -> i32 {
        if buf.is_null() {
            return invalid();
        }
        // SAFETY: the caller's, by the contract.
        let d = unsafe { &mut *buf };
        if d.state.is_null() || !(0..5).contains(&d.rand_type) {
            return invalid();
        }
        // SAFETY: set up, by the contract.
        unsafe { seed_generator(d, seed) };
        0
    }

    /// `initstate_r`: set `buf` up over `state`, `size` bytes, as the
    /// generator that size picks -- see [`crate::prng::initstate`] --
    /// seeded with `seed`. 0; or -1, `errno` EINVAL, for a NULL pointer or
    /// fewer than 8 bytes.
    ///
    /// # Safety
    ///
    /// `state` is NULL or `size` writable bytes, alive while `buf` is used;
    /// `buf` is NULL, zeroed, or set up over an array still alive -- whose
    /// first word records where its generator stood, for [`setstate_r`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn initstate_r(
        seed: u32,
        state: *mut u8,
        size: usize,
        buf: *mut RandomData,
    ) -> i32 {
        if state.is_null() || buf.is_null() {
            return invalid();
        }
        // SAFETY: the caller's, by the contract.
        if unsafe { init_generator(&mut *buf, seed, state, size) } {
            0
        } else {
            invalid()
        }
    }

    /// `setstate_r`: make `state`, an array [`initstate_r`] or `initstate`
    /// set up, the one `buf` describes, where its generator was left. 0; or
    /// -1, `errno` EINVAL, for a NULL pointer or an array whose first word no
    /// generator wrote.
    ///
    /// # Safety
    ///
    /// `state` is NULL or such an array, alive while `buf` is used; `buf` as
    /// for [`initstate_r`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn setstate_r(state: *mut u8, buf: *mut RandomData) -> i32 {
        if state.is_null() || buf.is_null() {
            return invalid();
        }
        // SAFETY: the caller's, by the contract.
        if unsafe { set_generator(&mut *buf, state) } {
            0
        } else {
            invalid()
        }
    }
}
pub use gnu_random_r::{initstate_r, random_r, setstate_r, srandom_r};

/// `fork`, before the system call: take the locks of `random`'s generator and
/// of the `rand48` initializers, so that the child's copies of neither are
/// caught half-changed by a thread the child will not have. Neither lock is
/// held while anything else is taken, so where this comes among `fork`'s
/// locks does not matter. Paired with [`unlock_after_fork`].
pub(crate) fn lock_for_fork() {
    lll_lock(&GLOBAL.lock);
    lll_lock(&RAND48_LOCK);
}

/// `fork`, afterwards -- in the parent, after a failed fork, and in the child,
/// whose one thread is the one that took them: give both locks back.
pub(crate) fn unlock_after_fork() {
    lll_unlock(&RAND48_LOCK);
    lll_unlock(&GLOBAL.lock);
}

// ---------------------------------------------------------------------------
// The rand48 family
// ---------------------------------------------------------------------------

/// POSIX's multiplier, `a`, until `lcong48` gives another.
const A: u64 = 0x5_DEEC_E66D;
/// POSIX's addend, `c`.
const C: u16 = 0xB;
/// 2^48 - 1: `X` is 48 bits.
const MASK48: u64 = 0xFFFF_FFFF_FFFF;
/// The process's `X` before anything sets it: glibc's.
const X_INITIAL: u64 = 0;
/// [`PARAMETERS`] for POSIX's `a` and `c`.
const STANDARD: u64 = (A << 16) | C as u64;

/// The process's `X`, for `drand48`, `lrand48` and `mrand48`.
static X: AtomicU64 = AtomicU64::new(X_INITIAL);
/// The multiplier and addend in use, `a << 16 | c`: POSIX's until `lcong48`
/// sets others, and again after `srand48` or `seed48`. One word, so that
/// whatever reads them never sees `a` from one initializer and `c` from
/// another.
static PARAMETERS: AtomicU64 = AtomicU64::new(STANDARD);
/// Serialises the initializers, each of which sets both [`X`] and
/// [`PARAMETERS`]. A `lowlevellock` word.
static RAND48_LOCK: AtomicI32 = AtomicI32::new(0);

/// Holds [`RAND48_LOCK`], and gives it up when dropped.
struct Held48;

impl Held48 {
    fn take() -> Self {
        lll_lock(&RAND48_LOCK);
        Self
    }
}

impl Drop for Held48 {
    fn drop(&mut self) {
        lll_unlock(&RAND48_LOCK);
    }
}

/// `X` from its three 16-bit words, lowest first, as POSIX's arrays hold it.
fn join(w: [u16; 3]) -> u64 {
    let [low, middle, high] = w;
    (u64::from(high) << 32) | (u64::from(middle) << 16) | u64::from(low)
}

/// `X`'s three words, lowest first.
fn split(x: u64) -> [u16; 3] {
    [x as u16, (x >> 16) as u16, (x >> 32) as u16]
}

/// One step: `(a X + c) mod 2^48`.
fn next48(x: u64, a: u64, c: u64) -> u64 {
    x.wrapping_mul(a).wrapping_add(c) & MASK48
}

/// `drand48`'s and `erand48`'s conversion: 2^-48 times `X`, exactly.
fn to_double(x: u64) -> f64 {
    // 48 bits fit a double's 53, and the scaling is by a power of two.
    x as f64 / 281_474_976_710_656.0
}

/// `lrand48`'s and `nrand48`'s: `X`'s high 31 bits.
fn to_nonnegative(x: u64) -> i64 {
    (x >> 17) as i64
}

/// `mrand48`'s and `jrand48`'s: `X`'s high 32 bits, as a signed number.
fn to_signed(x: u64) -> i64 {
    i64::from((x >> 16) as u32 as i32)
}

/// `srand48`'s `X`: `seedval`'s low 32 bits over 0x330E.
fn seeded_x(seedval: i64) -> u64 {
    ((seedval as u64 & 0xFFFF_FFFF) << 16) | 0x330E
}

/// The process's multiplier and addend.
fn parameters() -> (u64, u64) {
    let p = PARAMETERS.load(Ordering::Relaxed);
    (p >> 16, p & 0xFFFF)
}

/// Step the process's `X`. Unlocked, as `drand48`, `lrand48` and `mrand48`
/// need not be thread-safe; atomic, so that what a race costs is a number
/// drawn twice, never undefined behaviour.
fn draw() -> u64 {
    let (a, c) = parameters();
    let x = next48(X.load(Ordering::Relaxed), a, c);
    X.store(x, Ordering::Relaxed);
    x
}

/// Step the caller's `X`, in `xsubi`, with the process's multiplier and
/// addend -- which `lcong48` sets for these too.
///
/// # Safety
///
/// `xsubi` is three readable and writable `unsigned short`s.
unsafe fn draw_from(xsubi: *mut u16) -> u64 {
    let (a, c) = parameters();
    let words = xsubi.cast::<[u16; 3]>();
    // SAFETY: the caller's three words.
    let x = next48(join(unsafe { words.read() }), a, c);
    // SAFETY: as above.
    unsafe { words.write(split(x)) };
    x
}

/// `drand48`: a double in [0, 1) from the process's `X`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn drand48() -> f64 {
    to_double(draw())
}

/// `lrand48`: a number in [0, 2^31) from the process's `X`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lrand48() -> i64 {
    to_nonnegative(draw())
}

/// `mrand48`: a number in [-2^31, 2^31) from the process's `X`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mrand48() -> i64 {
    to_signed(draw())
}

/// `erand48`: [`drand48`]'s, from the caller's `X` in `xsubi`. NULL gives 0.
///
/// # Safety
///
/// `xsubi` is NULL or three readable and writable `unsigned short`s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn erand48(xsubi: *mut u16) -> f64 {
    if xsubi.is_null() {
        return 0.0;
    }
    // SAFETY: this function's contract.
    to_double(unsafe { draw_from(xsubi) })
}

/// `nrand48`: [`lrand48`]'s, from the caller's `X` in `xsubi`. NULL gives 0.
///
/// # Safety
///
/// As [`erand48`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn nrand48(xsubi: *mut u16) -> i64 {
    if xsubi.is_null() {
        return 0;
    }
    // SAFETY: this function's contract.
    to_nonnegative(unsafe { draw_from(xsubi) })
}

/// `jrand48`: [`mrand48`]'s, from the caller's `X` in `xsubi`. NULL gives 0.
///
/// # Safety
///
/// As [`erand48`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn jrand48(xsubi: *mut u16) -> i64 {
    if xsubi.is_null() {
        return 0;
    }
    // SAFETY: this function's contract.
    to_signed(unsafe { draw_from(xsubi) })
}

/// `srand48`: the process's `X` from `seedval` -- its low 32 bits as `X`'s
/// high 32, and 0x330E below them -- and POSIX's multiplier and addend back.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn srand48(seedval: i64) {
    let _held = Held48::take();
    X.store(seeded_x(seedval), Ordering::Relaxed);
    PARAMETERS.store(STANDARD, Ordering::Relaxed);
}

/// `seed48`: the process's `X` from `seed16v`'s three words, lowest first,
/// and POSIX's multiplier and addend back. Returns the `X` it replaced, in
/// three words of the calling thread's own that its next `seed48`
/// overwrites. A NULL `seed16v` changes nothing and returns them as they are.
///
/// # Safety
///
/// `seed16v` is NULL or three readable `unsigned short`s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn seed48(seed16v: *const u16) -> *mut u16 {
    // SAFETY: the calling thread's block, which no other thread touches.
    let kept = unsafe { core::ptr::addr_of_mut!((*crate::perthread::current()).seed48) };
    if seed16v.is_null() {
        return kept.cast();
    }
    // SAFETY: this function's contract. Read before `kept` is written, since
    // it may be what the last call returned.
    let x = join(unsafe { seed16v.cast::<[u16; 3]>().read() });
    let old = {
        let _held = Held48::take();
        PARAMETERS.store(STANDARD, Ordering::Relaxed);
        X.swap(x, Ordering::Relaxed)
    };
    // SAFETY: as above.
    unsafe { kept.write(split(old)) };
    kept.cast()
}

/// `lcong48`: the process's `X` from `param[0..3]`, and the multiplier from
/// `param[3..6]` -- lowest word first, both -- and the addend `param[6]`, for
/// every function of the family until `srand48` or `seed48` puts POSIX's
/// back. A NULL `param` changes nothing.
///
/// # Safety
///
/// `param` is NULL or seven readable `unsigned short`s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn lcong48(param: *const u16) {
    if param.is_null() {
        return;
    }
    // SAFETY: this function's contract.
    let [x0, x1, x2, a0, a1, a2, c] = unsafe { param.cast::<[u16; 7]>().read() };
    let x = join([x0, x1, x2]);
    let a = join([a0, a1, a2]);
    let _held = Held48::take();
    X.store(x, Ordering::Relaxed);
    PARAMETERS.store((a << 16) | u64::from(c), Ordering::Relaxed);
}

/// glibc's `struct drand48_data`: a `rand48` generator of the caller's, for
/// the `_r` forms. Only zeroed, it is one with `X = 0` and POSIX's multiplier
/// and addend: its first step sets `__a` and `__c` and marks it (`__init`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Drand48Data {
    /// `__x`: `X`, lowest word first.
    pub x: [u16; 3],
    /// `__old_x`: the `X` [`seed48_r`] replaced.
    pub old_x: [u16; 3],
    /// `__c`: the addend.
    pub c: u16,
    /// `__init`: nonzero once `__a` and `__c` are set.
    pub init: u16,
    /// `__a`: the multiplier.
    pub a: u64,
}

/// The multiplier and addend of the generator in `buffer`, setting POSIX's in
/// one only zeroed.
///
/// # Safety
///
/// `buffer` is a readable and writable `struct drand48_data`. Its fields are
/// reached through the pointer, never a reference, since the `xsubi` of a
/// caller of this may point into the same struct.
unsafe fn parameters_of(buffer: *mut Drand48Data) -> (u64, u64) {
    // SAFETY: this function's contract.
    unsafe {
        if (*buffer).init == 0 {
            (*buffer).a = A;
            (*buffer).c = C;
            (*buffer).init = 1;
        }
        ((*buffer).a, u64::from((*buffer).c))
    }
}

/// Step the `X` in `xsubi` with the multiplier and addend of `buffer`.
///
/// # Safety
///
/// `xsubi` is three readable and writable `unsigned short`s -- perhaps
/// `buffer`'s own `__x` -- and `buffer` as for [`parameters_of`].
unsafe fn draw_r(xsubi: *mut u16, buffer: *mut Drand48Data) -> u64 {
    // SAFETY: this function's contract.
    let (a, c) = unsafe { parameters_of(buffer) };
    let words = xsubi.cast::<[u16; 3]>();
    // SAFETY: as above.
    let x = next48(join(unsafe { words.read() }), a, c);
    // SAFETY: as above.
    unsafe { words.write(split(x)) };
    x
}

/// The `rand48` family's reentrant forms, glibc's, over a
/// `struct drand48_data` of the caller's -- an archive member of their own,
/// since a program may bring its own copy of names only glibc has. Each gives
/// 0; or -1, `errno` EINVAL, for a NULL pointer.
mod rand48_r {
    use super::{A, C, Drand48Data, draw_r, seeded_x, split, to_double, to_nonnegative, to_signed};
    use crate::errno::{EINVAL, set_errno};

    /// -1, with `errno` EINVAL.
    fn invalid() -> i32 {
        set_errno(EINVAL);
        -1
    }

    /// `buffer`'s own `__x`, as the `xsubi` of the forms that step it.
    fn own_x(buffer: *mut Drand48Data) -> *mut u16 {
        // SAFETY: a place computed from the caller's pointer, never read here.
        unsafe { core::ptr::addr_of_mut!((*buffer).x) }.cast()
    }

    /// `drand48_r`: `drand48`'s, from `buffer`'s generator, into `*result`.
    ///
    /// # Safety
    ///
    /// `buffer` is NULL or a readable and writable `struct drand48_data`;
    /// `result` is NULL or a writable `double`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn drand48_r(buffer: *mut Drand48Data, result: *mut f64) -> i32 {
        if buffer.is_null() || result.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe { result.write(to_double(draw_r(own_x(buffer), buffer))) };
        0
    }

    /// `erand48_r`: `erand48`'s, from the `X` in `xsubi` with `buffer`'s
    /// multiplier and addend, into `*result`.
    ///
    /// # Safety
    ///
    /// `xsubi` is NULL or three readable and writable `unsigned short`s;
    /// `buffer` and `result` as for [`drand48_r`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn erand48_r(
        xsubi: *mut u16,
        buffer: *mut Drand48Data,
        result: *mut f64,
    ) -> i32 {
        if xsubi.is_null() || buffer.is_null() || result.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe { result.write(to_double(draw_r(xsubi, buffer))) };
        0
    }

    /// `lrand48_r`: `lrand48`'s, from `buffer`'s generator, into `*result`.
    ///
    /// # Safety
    ///
    /// As [`drand48_r`], `result` a writable `long`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn lrand48_r(buffer: *mut Drand48Data, result: *mut i64) -> i32 {
        if buffer.is_null() || result.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe { result.write(to_nonnegative(draw_r(own_x(buffer), buffer))) };
        0
    }

    /// `nrand48_r`: `nrand48`'s, from the `X` in `xsubi` with `buffer`'s
    /// multiplier and addend, into `*result`.
    ///
    /// # Safety
    ///
    /// As [`erand48_r`], `result` a writable `long`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn nrand48_r(
        xsubi: *mut u16,
        buffer: *mut Drand48Data,
        result: *mut i64,
    ) -> i32 {
        if xsubi.is_null() || buffer.is_null() || result.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe { result.write(to_nonnegative(draw_r(xsubi, buffer))) };
        0
    }

    /// `mrand48_r`: `mrand48`'s, from `buffer`'s generator, into `*result`.
    ///
    /// # Safety
    ///
    /// As [`lrand48_r`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mrand48_r(buffer: *mut Drand48Data, result: *mut i64) -> i32 {
        if buffer.is_null() || result.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe { result.write(to_signed(draw_r(own_x(buffer), buffer))) };
        0
    }

    /// `jrand48_r`: `jrand48`'s, from the `X` in `xsubi` with `buffer`'s
    /// multiplier and addend, into `*result`.
    ///
    /// # Safety
    ///
    /// As [`nrand48_r`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn jrand48_r(
        xsubi: *mut u16,
        buffer: *mut Drand48Data,
        result: *mut i64,
    ) -> i32 {
        if xsubi.is_null() || buffer.is_null() || result.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe { result.write(to_signed(draw_r(xsubi, buffer))) };
        0
    }

    /// `srand48_r`: `srand48`, for `buffer`'s generator.
    ///
    /// # Safety
    ///
    /// `buffer` is NULL or a writable `struct drand48_data`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn srand48_r(seedval: i64, buffer: *mut Drand48Data) -> i32 {
        if buffer.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe {
            (*buffer).x = split(seeded_x(seedval));
            (*buffer).a = A;
            (*buffer).c = C;
            (*buffer).init = 1;
        }
        0
    }

    /// `seed48_r`: `seed48`, for `buffer`'s generator: the `X` it replaces
    /// kept in its `__old_x`.
    ///
    /// # Safety
    ///
    /// `seed16v` is NULL or three readable `unsigned short`s; `buffer` is
    /// NULL or a readable and writable `struct drand48_data`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn seed48_r(seed16v: *const u16, buffer: *mut Drand48Data) -> i32 {
        if seed16v.is_null() || buffer.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract; the seed is read before the
        // struct is written, in case it is the struct's own `__old_x`.
        unsafe {
            let x = seed16v.cast::<[u16; 3]>().read();
            (*buffer).old_x = (*buffer).x;
            (*buffer).x = x;
            (*buffer).a = A;
            (*buffer).c = C;
            (*buffer).init = 1;
        }
        0
    }

    /// `lcong48_r`: `lcong48`, for `buffer`'s generator.
    ///
    /// # Safety
    ///
    /// `param` is NULL or seven readable `unsigned short`s; `buffer` is NULL
    /// or a writable `struct drand48_data`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn lcong48_r(param: *const u16, buffer: *mut Drand48Data) -> i32 {
        if param.is_null() || buffer.is_null() {
            return invalid();
        }
        // SAFETY: this function's contract.
        unsafe {
            let [x0, x1, x2, a0, a1, a2, c] = param.cast::<[u16; 7]>().read();
            (*buffer).x = [x0, x1, x2];
            (*buffer).a = super::join([a0, a1, a2]);
            (*buffer).c = c;
            (*buffer).init = 1;
        }
        0
    }
}
pub use rand48_r::{
    drand48_r, erand48_r, jrand48_r, lcong48_r, lrand48_r, mrand48_r, nrand48_r, seed48_r,
    srand48_r,
};

/// Unset the process's `random` generator, so that its next use sets it up
/// afresh, as a process's first does.
#[cfg(test)]
fn forget_global_for_test() {
    let _held = Held::take();
    // SAFETY: under the lock, as every touch of `data` is.
    unsafe { *GLOBAL.data.get() = RandomData::UNSET };
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Serialising the process-wide state these tests drive ---------------
    //
    // `cargo test` runs tests on several threads, and POSIX gives a process
    // one `random` generator and one `rand48` X, shared by specification.
    // Each test that drives one holds its guard for its whole body: the unit
    // is "seed, draw, compare", not any single call. Poison is recovered, so
    // that one failure reports once rather than failing its siblings too.

    static RANDOM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static RAND48_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Serialises the `random` generator (`rand`, `srand`, `random`,
    /// `srandom`, `initstate`, `setstate`).
    #[must_use = "the guard serialises the random generator; bind it to `_g`"]
    fn lock_random_for_test() -> std::sync::MutexGuard<'static, ()> {
        RANDOM_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Serialises the process's `X` and parameters, which the functions that
    /// step a caller's `X` use too.
    #[must_use = "the guard serialises the rand48 state; bind it to `_g`"]
    fn lock_rand48_for_test() -> std::sync::MutexGuard<'static, ()> {
        RAND48_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // -- The oracle ----------------------------------------------------------

    /// glibc 2.39's answers (`posix/tools/oracle/random_harness.py`).
    const ORACLE: &str = include_str!("random_oracle.txt");

    /// A token of an oracle line: a number, a double (C's `%a`, compared bit
    /// for bit), or punctuation.
    #[derive(Debug, PartialEq)]
    enum Tok {
        N(i64),
        F(u64),
        S(&'static str),
    }

    fn n(v: impl Into<i64>) -> Tok {
        Tok::N(v.into())
    }

    fn f(v: f64) -> Tok {
        Tok::F(v.to_bits())
    }

    /// C's `%a` of a double, exactly: `0x1.8p-3`, `0x0p+0`.
    fn hex_float(s: &str) -> f64 {
        let body = s.strip_prefix("0x").expect("C's %a");
        let (mantissa, exponent) = body.split_once('p').expect("an exponent");
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = u64::from_str_radix(&format!("{int}{frac}"), 16).expect("hex digits");
        assert!(digits < 1 << 53, "{s}: more digits than a double holds");
        let exponent: i32 = exponent.parse().expect("a decimal exponent");
        let shift = exponent - 4 * i32::try_from(frac.len()).unwrap();
        digits as f64 * 2f64.powi(shift)
    }

    /// The oracle's line for `key`, tokenised.
    fn oracle(key: &str) -> Vec<Tok> {
        let line = ORACLE
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(" = "))
            .unwrap_or_else(|| panic!("no `{key}` in random_oracle.txt"));
        line.split_whitespace()
            .map(|t| {
                if t.starts_with("0x") {
                    Tok::F(hex_float(t).to_bits())
                } else if let Ok(v) = t.parse::<i64>() {
                    Tok::N(v)
                } else {
                    Tok::S(match t {
                        "/" => "/",
                        "|" => "|",
                        ":" => ":",
                        other => panic!("`{other}` in the oracle's `{key}`"),
                    })
                }
            })
            .collect()
    }

    /// An array that outlives every test: one the process's generator is
    /// pointed at must, or a test failing before it points it back would leave
    /// the generator in a dead stack frame for the next.
    fn array(n: usize) -> &'static mut [u8] {
        Box::leak(vec![0u8; n].into_boxed_slice())
    }

    /// The seeds and sizes the harness ran.
    const SEEDS: [u32; 8] = [0, 1, 2, 42, 12345, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF];
    const SIZES: [usize; 11] = [8, 16, 31, 32, 63, 64, 100, 128, 255, 256, 1000];

    #[test]
    fn the_oracle_has_every_line() {
        let lines = ORACLE.lines().filter(|l| !l.starts_with('#')).count();
        // rand-default, drand48-default, three per seed, one per size and
        // seed, initstate-small, switch, one per size and seed again,
        // random_r-small, one per seed, five more, and two layouts.
        let want = 2
            + 3 * SEEDS.len()
            + SIZES.len() * SEEDS.len()
            + 2
            + SIZES.len() * SEEDS.len()
            + 1
            + SEEDS.len()
            + 5
            + 2;
        assert_eq!(lines, want);
    }

    // -- random and rand -----------------------------------------------------

    /// A process that never seeds: `rand`'s first 64, glibc's -- the
    /// generator set up afresh, as a process's first use sets it up.
    #[test]
    fn rand_unseeded_is_glibcs() {
        let _g = lock_random_for_test();
        forget_global_for_test();
        let got: Vec<Tok> = (0..64).map(|_| n(rand())).collect();
        assert_eq!(got, oracle("rand-default"));
        // "the same sequence shall be generated as when srand() is first
        // called with a seed value of 1"
        assert_eq!(oracle("rand-default"), oracle("rand 1"));
    }

    #[test]
    fn rand_and_random_are_glibcs() {
        let _g = lock_random_for_test();
        // The harness drew these from the generator a process starts with.
        forget_global_for_test();
        for seed in SEEDS {
            srand(seed);
            let got: Vec<Tok> = (0..64).map(|_| n(rand())).collect();
            assert_eq!(got, oracle(&format!("rand {seed}")), "srand({seed})");
            srandom(seed);
            let got: Vec<Tok> = (0..64).map(|_| n(random())).collect();
            assert_eq!(got, oracle(&format!("random {seed}")), "srandom({seed})");
        }
    }

    #[test]
    fn rand_r_is_glibcs() {
        for seed in SEEDS {
            let mut state = seed;
            let mut got: Vec<Tok> = (0..64)
                .map(|_| n(unsafe { rand_r(&raw mut state) }))
                .collect();
            got.push(Tok::S(":"));
            got.push(n(state));
            assert_eq!(got, oracle(&format!("rand_r {seed}")), "rand_r from {seed}");
        }
        assert_eq!(unsafe { rand_r(null_mut()) }, 0);
    }

    /// Every array size's generator, for every seed.
    #[test]
    fn initstate_is_glibcs() {
        let _g = lock_random_for_test();
        let buf = array(1024);
        for size in SIZES {
            for seed in SEEDS {
                buf.fill(0);
                let old = unsafe { initstate(seed, buf.as_mut_ptr(), size) };
                assert!(!old.is_null(), "initstate({seed}, {size})");
                let got: Vec<Tok> = (0..64).map(|_| n(random())).collect();
                assert_eq!(
                    got,
                    oracle(&format!("initstate {size} {seed}")),
                    "initstate({seed}, buf, {size})"
                );
                assert_eq!(unsafe { setstate(old) }, buf.as_mut_ptr(), "{size} {seed}");
            }
        }
    }

    /// Fewer than 8 bytes: NULL, and the generator goes on as it was.
    #[test]
    fn initstate_refuses_less_than_8_bytes() {
        let _g = lock_random_for_test();
        let mut small = [0u8; 8];
        forget_global_for_test();
        srandom(3);
        crate::errno::set_errno(0);
        let r = unsafe { initstate(1, small.as_mut_ptr(), 7) };
        let errno = crate::errno::get_errno();
        let got = vec![n(i64::from(r.is_null())), n(random())];
        assert_eq!(got, oracle("initstate-small"));
        assert_eq!(errno, EINVAL);
        assert!(unsafe { initstate(1, null_mut(), 128) }.is_null());
    }

    /// Two arrays, switched between: each generator is taken up where it was
    /// left, and `setstate` returns the array it replaced.
    #[test]
    fn setstate_switches_as_glibcs() {
        let _g = lock_random_for_test();
        let (pa, pb) = (array(128).as_mut_ptr(), array(256).as_mut_ptr());
        let orig = unsafe { initstate(7, pa, 128) };
        let _ = unsafe { initstate(9, pb, 256) };
        let mut got: Vec<Tok> = (0..4).map(|_| n(random())).collect();
        let back = unsafe { setstate(pa) };
        got.extend([Tok::S("|"), n(i64::from(back == pb)), Tok::S("|")]);
        got.extend((0..4).map(|_| n(random())));
        let back = unsafe { setstate(pb) };
        got.extend([Tok::S("|"), n(i64::from(back == pa)), Tok::S("|")]);
        got.extend((0..4).map(|_| n(random())));
        assert_eq!(got, oracle("switch"));
        unsafe { setstate(orig) };
    }

    /// A first word no generator could have written -- a rear word past the
    /// generator's end, or a negative generator number -- is refused, and the
    /// generator in use goes on as it was.
    #[test]
    fn setstate_refuses_what_no_generator_wrote() {
        let _g = lock_random_for_test();
        let orig = unsafe { initstate(5, array(128).as_mut_ptr(), 128) };
        let first = random();
        let mut bad = [0u8; 256];
        for word in [KINDS * 31 + 3, KINDS * 63 + 4, -1, -7] {
            bad[..4].copy_from_slice(&word.to_ne_bytes());
            crate::errno::set_errno(0);
            assert!(unsafe { setstate(bad.as_mut_ptr()) }.is_null(), "{word}");
            assert_eq!(crate::errno::get_errno(), EINVAL);
        }
        assert!(unsafe { setstate(null_mut()) }.is_null());
        // The sequence of seed 5 carried on through all that.
        let second = random();
        let _ = unsafe { initstate(5, array(128).as_mut_ptr(), 128) };
        assert_eq!([random(), random()], [first, second]);
        unsafe { setstate(orig) };
    }

    /// The array is the caller's `char` array: nothing needs it aligned.
    #[test]
    fn initstate_takes_an_unaligned_array() {
        let _g = lock_random_for_test();
        let orig = unsafe { initstate(42, array(129).as_mut_ptr().wrapping_add(1), 128) };
        let got: Vec<Tok> = (0..64).map(|_| n(random())).collect();
        assert_eq!(got, oracle("initstate 128 42"));
        unsafe { setstate(orig) };
    }

    /// `random` is thread-safe and `rand` shares it: four threads drawing at
    /// once draw what one thread drawing alone does, each number once.
    #[test]
    fn random_is_one_sequence_across_threads() {
        let _g = lock_random_for_test();
        srandom(99);
        let mut alone: Vec<i64> = (0..8000).map(|_| random()).collect();
        srandom(99);
        let mut together: Vec<i64> = std::thread::scope(|s| {
            let threads: Vec<_> = (0..4)
                .map(|t| {
                    s.spawn(move || {
                        (0..2000)
                            .map(|_| {
                                if t % 2 == 0 {
                                    random()
                                } else {
                                    i64::from(rand())
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            threads
                .into_iter()
                .flat_map(|h| h.join().expect("a drawing thread panicked"))
                .collect()
        });
        alone.sort_unstable();
        together.sort_unstable();
        assert_eq!(together, alone, "none lost to a race, none drawn twice");
    }

    // -- the random_r family --------------------------------------------------

    #[test]
    fn random_r_is_glibcs() {
        let mut buf = [0u8; 1024];
        for size in SIZES {
            for seed in SEEDS {
                let mut d = RandomData::UNSET;
                buf.fill(0);
                let rc = unsafe { initstate_r(seed, buf.as_mut_ptr(), size, &raw mut d) };
                let mut got = vec![n(rc), Tok::S(":")];
                for _ in 0..64 {
                    let mut r = 0;
                    assert_eq!(unsafe { random_r(&raw mut d, &raw mut r) }, 0);
                    got.push(n(r));
                }
                assert_eq!(
                    got,
                    oracle(&format!("random_r {size} {seed}")),
                    "{size} {seed}"
                );
            }
        }
    }

    #[test]
    fn random_r_refuses_what_is_not_set_up() {
        let mut d = RandomData::UNSET;
        let mut small = [0u8; 8];
        crate::errno::set_errno(0);
        let rc = unsafe { initstate_r(1, small.as_mut_ptr(), 7, &raw mut d) };
        let got = vec![n(rc), n(i64::from(crate::errno::get_errno() == EINVAL))];
        assert_eq!(got, oracle("random_r-small"));
        // Never set up: refused, where glibc's would follow a null pointer.
        let mut r = 0;
        assert_eq!(unsafe { random_r(&raw mut d, &raw mut r) }, -1);
        assert_eq!(unsafe { srandom_r(1, &raw mut d) }, -1);
        assert_eq!(unsafe { random_r(null_mut(), &raw mut r) }, -1);
        assert_eq!(unsafe { random_r(&raw mut d, null_mut()) }, -1);
        assert_eq!(unsafe { initstate_r(1, null_mut(), 128, &raw mut d) }, -1);
        assert_eq!(unsafe { setstate_r(null_mut(), &raw mut d) }, -1);
        assert_eq!(crate::errno::get_errno(), EINVAL);
    }

    /// `srandom_r` restarts a generator's sequence; `setstate_r` takes one up
    /// where it was left, as the global ones do.
    #[test]
    fn srandom_r_and_setstate_r() {
        let (mut a, mut b) = ([0u8; 64], [0u8; 256]);
        let mut d = RandomData::UNSET;
        assert_eq!(unsafe { initstate_r(4, a.as_mut_ptr(), 64, &raw mut d) }, 0);
        let draw = |d: &mut RandomData| {
            let mut r = 0;
            assert_eq!(unsafe { random_r(d, &raw mut r) }, 0);
            r
        };
        let first: Vec<i32> = (0..10).map(|_| draw(&mut d)).collect();
        assert_eq!(unsafe { srandom_r(4, &raw mut d) }, 0);
        assert_eq!((0..10).map(|_| draw(&mut d)).collect::<Vec<_>>(), first);
        // Leave a after 5, run b, come back: a goes on from its sixth.
        assert_eq!(unsafe { srandom_r(4, &raw mut d) }, 0);
        let _ = (0..5).map(|_| draw(&mut d)).count();
        assert_eq!(
            unsafe { initstate_r(8, b.as_mut_ptr(), 256, &raw mut d) },
            0
        );
        let _ = draw(&mut d);
        assert_eq!(unsafe { setstate_r(a.as_mut_ptr(), &raw mut d) }, 0);
        assert_eq!((0..5).map(|_| draw(&mut d)).collect::<Vec<_>>(), first[5..]);
    }

    #[test]
    fn random_data_is_glibcs_layout() {
        use core::mem::{offset_of, size_of};
        let got = format!(
            "{} fptr={},rptr={},state={},rand_type={},rand_deg={},rand_sep={},end_ptr={}",
            size_of::<RandomData>(),
            offset_of!(RandomData, fptr),
            offset_of!(RandomData, rptr),
            offset_of!(RandomData, state),
            offset_of!(RandomData, rand_type),
            offset_of!(RandomData, rand_deg),
            offset_of!(RandomData, rand_sep),
            offset_of!(RandomData, end_ptr),
        );
        assert!(
            ORACLE.contains(&format!("layout struct_random_data {got}\n")),
            "{got}"
        );
        let got = format!(
            "{} __x={},__old_x={},__c={},__init={},__a={}",
            size_of::<Drand48Data>(),
            offset_of!(Drand48Data, x),
            offset_of!(Drand48Data, old_x),
            offset_of!(Drand48Data, c),
            offset_of!(Drand48Data, init),
            offset_of!(Drand48Data, a),
        );
        assert!(
            ORACLE.contains(&format!("layout struct_drand48_data {got}\n")),
            "{got}"
        );
    }

    // -- the fork pair ---------------------------------------------------------

    #[test]
    fn fork_holds_both_generators() {
        let _g = lock_random_for_test();
        let _h = lock_rand48_for_test();
        lock_for_fork();
        // Read, release, then assert: a failed assertion with the locks still
        // held would hang every other test here.
        let held = (
            GLOBAL.lock.load(Ordering::Relaxed),
            RAND48_LOCK.load(Ordering::Relaxed),
        );
        unlock_after_fork();
        assert!(held.0 != 0 && held.1 != 0, "{held:?}");
        let _ = random();
        srand48(0);
    }

    // -- the rand48 family -----------------------------------------------------

    /// Nothing seeded: `X` starts where glibc's does. (Replayed from
    /// `X_INITIAL`, since other tests will have moved the process's own.)
    #[test]
    fn rand48_unseeded_is_glibcs() {
        let mut x = X_INITIAL;
        let mut step = || {
            x = next48(x, A, u64::from(C));
            x
        };
        let mut got: Vec<Tok> = (0..8).map(|_| f(to_double(step()))).collect();
        got.push(Tok::S("/"));
        got.extend((0..8).map(|_| n(to_nonnegative(step()))));
        got.push(Tok::S("/"));
        got.extend((0..8).map(|_| n(to_signed(step()))));
        assert_eq!(got, oracle("drand48-default"));
        assert_eq!(X_INITIAL, 0);
    }

    /// `drand48`, `lrand48` and `mrand48` through the process's own `X`, and
    /// `srand48`'s seeding of it, against the reentrant forms' oracle lines.
    #[test]
    fn srand48_is_glibcs() {
        let _g = lock_rand48_for_test();
        for seed in SEEDS {
            srand48(i64::from(seed));
            let mut got: Vec<Tok> = (0..16).map(|_| f(drand48())).collect();
            got.push(Tok::S("/"));
            got.extend((0..16).map(|_| n(lrand48())));
            got.push(Tok::S("/"));
            got.extend((0..16).map(|_| n(mrand48())));
            assert_eq!(got, oracle(&format!("drand48_r {seed}")), "srand48({seed})");
        }
        // Only the low 32 bits of the seed count.
        srand48(0x1_0000_0005);
        let high = lrand48();
        srand48(5);
        assert_eq!(lrand48(), high);
        srand48(0);
    }

    #[test]
    fn seed48_is_glibcs() {
        let _g = lock_rand48_for_test();
        srand48(77);
        let old = unsafe { seed48([0x1234, 0x5678, 0x9ABC].as_ptr()) };
        let [a, b, c] = unsafe { old.cast::<[u16; 3]>().read() };
        let mut got = vec![n(a), n(b), n(c), Tok::S("/")];
        got.extend((0..8).map(|_| n(lrand48())));
        assert_eq!(got, oracle("seed48"));
        // NULL changes nothing and gives the same three words back.
        let again = unsafe { seed48(core::ptr::null()) };
        assert_eq!(again, old);
        assert_eq!(unsafe { again.cast::<[u16; 3]>().read() }, [a, b, c]);
        srand48(0);
    }

    /// `seed48`'s copy of the `X` it replaced is the calling thread's own: a
    /// `seed48` in another thread neither shares it nor overwrites it.
    #[test]
    fn seed48s_copy_is_the_calling_threads() {
        let _g = lock_rand48_for_test();
        srand48(1);
        let mine = unsafe { seed48([1, 2, 3].as_ptr()) };
        let theirs = std::thread::spawn(|| unsafe { seed48([4, 5, 6].as_ptr()) as usize })
            .join()
            .expect("the other thread panicked");
        assert_ne!(mine as usize, theirs);
        // Mine still holds what my call replaced, though theirs ran since.
        assert_eq!(unsafe { mine.cast::<[u16; 3]>().read() }, [0x330E, 1, 0]);
        srand48(0);
    }

    /// `lcong48` sets the multiplier and addend for every function of the
    /// family, a caller's own `X` included, until `srand48` puts POSIX's back.
    #[test]
    fn lcong48_is_glibcs() {
        let _g = lock_rand48_for_test();
        let param: [u16; 7] = [0x1111, 0x2222, 0x3333, 0x4444, 0x5555, 0x0066, 0x0777];
        let mut xsubi: [u16; 3] = [1, 2, 3];
        unsafe { lcong48(param.as_ptr()) };
        let mut got: Vec<Tok> = (0..8).map(|_| n(lrand48())).collect();
        got.push(Tok::S("/"));
        got.extend((0..4).map(|_| n(unsafe { nrand48(xsubi.as_mut_ptr()) })));
        srand48(5);
        got.push(Tok::S("/"));
        got.extend((0..4).map(|_| n(lrand48())));
        assert_eq!(got, oracle("lcong48"));
        unsafe { lcong48(core::ptr::null()) };
        srand48(0);
    }

    /// POSIX's own example, from the `drand48` page's EXAMPLES: the values
    /// and states the standard's generator must give.
    #[test]
    fn posixs_example_holds() {
        let _g = lock_rand48_for_test();
        srand48(0); // POSIX's multiplier and addend
        let mut xsubi: [u16; 3] = [37174, 64810, 11603];
        let steps: [(f64, [u16; 3]); 5] = [
            (0.896, [22537, 47966, 58735]),
            (0.337, [37344, 32911, 22119]),
            (0.647, [23659, 29872, 42445]),
            (0.500, [31642, 7875, 32802]),
            (0.506, [64669, 14399, 33170]),
        ];
        for (low, state) in steps {
            let d = unsafe { erand48(xsubi.as_mut_ptr()) };
            assert!((low..=low + 0.001).contains(&d), "{d}");
            assert_eq!(xsubi, state);
        }
        let mut xsubi: [u16; 3] = [25175, 11052, 45015];
        let steps: [(i64, [u16; 3]); 5] = [
            (1_699_503_220, [2326, 23668, 25932]),
            (-992_276_007, [41577, 4569, 50395]),
            (-19_535_776, [31936, 59488, 65237]),
            (79_438_377, [40395, 8745, 1212]),
            (-1_258_917_728, [37242, 28832, 46326]),
        ];
        for (v, state) in steps {
            assert_eq!(unsafe { jrand48(xsubi.as_mut_ptr()) }, v);
            assert_eq!(xsubi, state);
        }
        let mut xsubi: [u16; 3] = [546, 33817, 23389];
        let steps: [(i64, [u16; 3]); 5] = [
            (914_920_692, [29829, 10728, 27921]),
            (754_104_482, [6828, 28997, 23013]),
            (609_453_945, [58183, 3826, 18599]),
            (1_878_644_360, [36678, 44304, 57331]),
            (2_114_923_686, [58585, 22861, 64542]),
        ];
        for (v, state) in steps {
            assert_eq!(unsafe { nrand48(xsubi.as_mut_ptr()) }, v);
            assert_eq!(xsubi, state);
        }
        assert_eq!(unsafe { erand48(null_mut()) }, 0.0);
        assert_eq!(unsafe { nrand48(null_mut()) }, 0);
        assert_eq!(unsafe { jrand48(null_mut()) }, 0);
    }

    #[test]
    fn the_ranges_hold() {
        let _g = lock_rand48_for_test();
        srand48(99);
        let (mut negative, mut positive) = (false, false);
        for _ in 0..2000 {
            let d = drand48();
            assert!((0.0..1.0).contains(&d), "{d}");
            let l = lrand48();
            assert!((0..1 << 31).contains(&l), "{l}");
            let m = mrand48();
            assert!((-(1 << 31)..1 << 31).contains(&m), "{m}");
            negative |= m < 0;
            positive |= m > 0;
        }
        assert!(negative && positive);
        srand48(0);
    }

    // -- the drand48_r family --------------------------------------------------

    #[test]
    fn drand48_r_is_glibcs() {
        for seed in SEEDS {
            let mut d = Drand48Data::default();
            assert_eq!(unsafe { srand48_r(i64::from(seed), &raw mut d) }, 0);
            let mut got = Vec::new();
            for _ in 0..16 {
                let mut v = 0.0;
                assert_eq!(unsafe { drand48_r(&raw mut d, &raw mut v) }, 0);
                got.push(f(v));
            }
            got.push(Tok::S("/"));
            for _ in 0..16 {
                let mut v = 0;
                assert_eq!(unsafe { lrand48_r(&raw mut d, &raw mut v) }, 0);
                got.push(n(v));
            }
            got.push(Tok::S("/"));
            for _ in 0..16 {
                let mut v = 0;
                assert_eq!(unsafe { mrand48_r(&raw mut d, &raw mut v) }, 0);
                got.push(n(v));
            }
            assert_eq!(
                got,
                oracle(&format!("drand48_r {seed}")),
                "srand48_r({seed})"
            );
        }
    }

    /// A struct only zeroed is a generator with X = 0 and POSIX's parameters.
    #[test]
    fn drand48_r_zeroed_is_glibcs() {
        let mut d = Drand48Data::default();
        let got: Vec<Tok> = (0..8)
            .map(|_| {
                let mut v = 0;
                assert_eq!(unsafe { lrand48_r(&raw mut d, &raw mut v) }, 0);
                n(v)
            })
            .collect();
        assert_eq!(got, oracle("drand48_r-zeroed"));
        assert_eq!((d.a, d.c, d.init), (A, C, 1));
    }

    #[test]
    fn lcong48_r_is_glibcs() {
        let param: [u16; 7] = [0xAAAA, 0xBBBB, 0xCCCC, 0x0101, 0x0202, 0x0003, 0x0005];
        let mut xsubi: [u16; 3] = [7, 8, 9];
        let mut d = Drand48Data::default();
        assert_eq!(unsafe { lcong48_r(param.as_ptr(), &raw mut d) }, 0);
        let mut got = Vec::new();
        for _ in 0..8 {
            let mut v = 0;
            assert_eq!(unsafe { lrand48_r(&raw mut d, &raw mut v) }, 0);
            got.push(n(v));
        }
        got.push(Tok::S("/"));
        for _ in 0..4 {
            let mut v = 0.0;
            assert_eq!(
                unsafe { erand48_r(xsubi.as_mut_ptr(), &raw mut d, &raw mut v) },
                0
            );
            got.push(f(v));
        }
        for _ in 0..4 {
            let mut v = 0;
            assert_eq!(
                unsafe { nrand48_r(xsubi.as_mut_ptr(), &raw mut d, &raw mut v) },
                0
            );
            got.push(n(v));
        }
        for _ in 0..4 {
            let mut v = 0;
            assert_eq!(
                unsafe { jrand48_r(xsubi.as_mut_ptr(), &raw mut d, &raw mut v) },
                0
            );
            got.push(n(v));
        }
        assert_eq!(got, oracle("lcong48_r"));
    }

    #[test]
    fn seed48_r_is_glibcs() {
        let mut d = Drand48Data::default();
        assert_eq!(unsafe { srand48_r(1000, &raw mut d) }, 0);
        assert_eq!(
            unsafe { seed48_r([0xFEDC, 0xBA98, 0x7654].as_ptr(), &raw mut d) },
            0
        );
        let [a, b, c] = d.old_x;
        let mut got = vec![n(a), n(b), n(c), Tok::S("/")];
        for _ in 0..8 {
            let mut v = 0;
            assert_eq!(unsafe { lrand48_r(&raw mut d, &raw mut v) }, 0);
            got.push(n(v));
        }
        assert_eq!(got, oracle("seed48_r"));
    }

    #[test]
    fn the_r_forms_refuse_null() {
        let mut d = Drand48Data::default();
        let mut xs = [0u16; 3];
        let param = [0u16; 7];
        let (mut v, mut l) = (0.0, 0);
        crate::errno::set_errno(0);
        unsafe {
            assert_eq!(drand48_r(null_mut(), &raw mut v), -1);
            assert_eq!(drand48_r(&raw mut d, null_mut()), -1);
            assert_eq!(erand48_r(null_mut(), &raw mut d, &raw mut v), -1);
            assert_eq!(lrand48_r(&raw mut d, null_mut()), -1);
            assert_eq!(nrand48_r(xs.as_mut_ptr(), null_mut(), &raw mut l), -1);
            assert_eq!(mrand48_r(null_mut(), &raw mut l), -1);
            assert_eq!(jrand48_r(xs.as_mut_ptr(), &raw mut d, null_mut()), -1);
            assert_eq!(srand48_r(1, null_mut()), -1);
            assert_eq!(seed48_r(core::ptr::null(), &raw mut d), -1);
            assert_eq!(lcong48_r(param.as_ptr(), null_mut()), -1);
            assert_eq!(lcong48_r(core::ptr::null(), &raw mut d), -1);
        }
        assert_eq!(crate::errno::get_errno(), EINVAL);
        assert_eq!(d, Drand48Data::default(), "nothing written");
    }

    /// The `_r` forms' generators are their callers': one does not move
    /// another, nor the process's.
    #[test]
    fn the_r_forms_are_independent() {
        let _g = lock_rand48_for_test();
        srand48(3);
        let process = lrand48();
        srand48(3);
        let mut d1 = Drand48Data::default();
        let mut d2 = Drand48Data::default();
        unsafe {
            srand48_r(3, &raw mut d1);
            srand48_r(3, &raw mut d2);
        }
        let mut v = 0;
        unsafe { lrand48_r(&raw mut d1, &raw mut v) };
        assert_eq!(v, process);
        unsafe { lrand48_r(&raw mut d2, &raw mut v) };
        assert_eq!(v, process, "d1's draw did not move d2");
        assert_eq!(lrand48(), process, "nor the process's X");
        srand48(0);
    }
}
