//! A decode holds the picture once, and a thumbnail holds it not at all.
//!
//! The PNG decoder reads its decompressed stream a row at a time
//! (`png::Scanlines`), so the stream -- every row of the picture plus a filter
//! byte each, as large as the picture itself -- never exists whole. That is a
//! claim about memory, and memory is what this measures: a counting global
//! allocator records the peak bytes live during one decode.
//!
//! The counts are kept per thread. libtest runs each test on a thread of its
//! own and the decoders do not start threads, so a test's counts are its
//! decode's and nothing else's: not the harness printing on another thread,
//! not a sibling test decoding at the same moment. That is what lets the tests
//! run side by side, with nothing shared for their order to matter to.
//!
//! Before the change, a full decode held the stream *and* the pixels, and a
//! scaled decode held the stream on its way to a thumbnail -- for a
//! 24-megapixel photograph, 96 MB spent producing 64 KB of preview. The
//! bounds below are set so either of those would fail them by a wide margin.

// A measurement divides and asserts on the result; the defensive lints that
// forbid that in production code are off here, as `CLAUDE.md` prescribes for
// test code.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use imagecodec::Limits;

/// The system allocator, counting.
struct Counting;

thread_local! {
    // Signed, because a thread can free what another allocated -- a test's
    // name, handed to it by the harness -- and its count then falls below
    // where it started. `const`-initialised with no destructor, so reading
    // them allocates nothing, which is what code inside an allocator needs.
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

/// Add `bytes` (negative for a free) to this thread's live count, raising its
/// peak to match.
fn count(bytes: isize) {
    // `try_with`, not `with`: an allocation made while the thread is being
    // torn down, after its locals are gone, is simply not counted -- panicking
    // inside an allocator would abort the whole run.
    let _ = LIVE.try_with(|live| {
        let now = live.get().wrapping_add(bytes);
        live.set(now);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
    });
}

/// A size as a signed count. No allocation exceeds `isize::MAX` bytes.
fn signed(bytes: usize) -> isize {
    isize::try_from(bytes).unwrap_or(isize::MAX)
}

// SAFETY: every method forwards to `System` with the arguments it was given,
// so the allocator's contract is `System`'s; the counters are bookkeeping on
// the side and never influence what is allocated.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            count(signed(layout.size()));
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged; `ptr` came from this allocator.
        unsafe { System.dealloc(ptr, layout) };
        count(-signed(layout.size()));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `realloc`'s contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            count(signed(new_size) - signed(layout.size()));
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Peak bytes live on this thread during `work`, above what was live when it
/// started.
fn peak_during<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    let out = work();
    let peak = PEAK.with(Cell::get);
    (out, usize::try_from(peak - base).unwrap_or(0))
}

const W: u32 = 2000;
const H: u32 = 1500;
/// What the decompressed stream of the test picture weighs: RGBA, and a
/// filter byte per row.
const STREAM_BYTES: usize = (H as usize) * (W as usize * 4 + 1);
/// What the decoded pixels weigh.
const PIXEL_BYTES: usize = (W as usize) * (H as usize) * 4;

#[test]
fn a_thumbnail_never_holds_the_picture_at_its_own_size() {
    let file = imagecodec::testing::png_gradient(W, H);
    let (thumb, peak) =
        peak_during(|| imagecodec::decode_scaled(&file, Limits::default(), 128, 128));
    let thumb = thumb.expect("a thumbnail");
    assert!(thumb.width <= 128 && thumb.height <= 128);
    println!("thumbnail of a {W}x{H} picture: peak {peak} bytes (the stream is {STREAM_BYTES})");
    assert!(
        peak < STREAM_BYTES / 8,
        "a thumbnail peaked at {peak} bytes against a {STREAM_BYTES}-byte stream: the source was held at something like its own size"
    );
}

#[test]
fn a_full_decode_holds_the_picture_once_not_twice() {
    let file = imagecodec::testing::png_gradient(W, H);
    let (image, peak) = peak_during(|| imagecodec::decode(&file, Limits::default()));
    let image = image.expect("a picture");
    assert_eq!((image.width, image.height), (W, H));
    println!("full decode of a {W}x{H} picture: peak {peak} bytes (the pixels are {PIXEL_BYTES})");
    assert!(
        peak < PIXEL_BYTES + STREAM_BYTES / 8,
        "a full decode peaked at {peak} bytes for {PIXEL_BYTES} bytes of pixels: the decompressed stream was held alongside them"
    );
}
