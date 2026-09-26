//! A decode holds the picture once, and a thumbnail holds it not at all.
//!
//! The PNG decoder reads its decompressed stream a row at a time
//! (`png::Scanlines`), so the stream -- every row of the picture plus a filter
//! byte each, as large as the picture itself -- never exists whole. That is a
//! claim about memory, and memory is what this measures: a counting global
//! allocator records the peak bytes live during one decode.
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
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use imagecodec::Limits;

/// The system allocator, counting.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the arguments it was given,
// so the allocator's contract is `System`'s; the counters are bookkeeping on
// the side and never influence what is allocated.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = LIVE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged; `ptr` came from this allocator.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `realloc`'s contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                let grown = new_size - layout.size();
                let now = LIVE.fetch_add(grown, Ordering::SeqCst) + grown;
                PEAK.fetch_max(now, Ordering::SeqCst);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::SeqCst);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The tests in this file share the counters, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Peak bytes live during `work`, above what was live when it started.
fn peak_during<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let out = work();
    (out, PEAK.load(Ordering::SeqCst).saturating_sub(base))
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
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
