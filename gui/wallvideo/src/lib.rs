//! A video as the desktop's background: the arithmetic of `wallvideo`, the
//! background program (`gui/backdrop`) that plays a file -- looping, at its
//! own pace, at the size the desktop's background needs -- and writes its
//! pictures for the desktop to show.
//!
//! The program itself is `main.rs`: it opens the file, waits for the
//! desktop to say the background's size, then decodes each frame, waits
//! until it is due, makes it no larger than the background needs
//! ([`cover_size`], [`shrink`]) and writes it. At the end it goes back to
//! the start. While the desktop says nobody can see the background it
//! decodes nothing, and when it is seen again it carries on from where it
//! was rather than racing to catch up.
//!
//! **Why the size matters.** Each picture goes through two pipes -- this
//! program's to the desktop, the desktop's to the compositor -- a whole
//! picture each time. A film shot at 4K on a 1080p screen would move four
//! times the bytes the screen can show. So a frame larger than it needs to
//! be to cover the background is made smaller first, its shape kept; the
//! desktop's own fit (fill, fit, stretch, centre) is applied to what
//! arrives, as to a picture file.

use std::time::{Duration, Instant};

/// The size a `frame`-sized picture is sent at for a background `screen`
/// pixels: its own, or -- where it is larger than it needs to be to cover
/// the screen whole -- just large enough to, its shape kept. Never larger
/// than its own: scaling up is the compositor's, at no cost on the pipes.
#[must_use]
pub fn cover_size(frame: (u32, u32), screen: (u32, u32)) -> (u32, u32) {
    let ((w, h), (sw, sh)) = (frame, screen);
    if w == 0 || h == 0 || sw == 0 || sh == 0 {
        return frame;
    }
    // The factor that makes it cover the screen in both directions.
    let scale = (f64::from(sw) / f64::from(w)).max(f64::from(sh) / f64::from(h));
    if scale >= 1.0 {
        return frame;
    }
    let side = |n: u32| {
        let scaled = (f64::from(n) * scale).ceil();
        // At most `n`, as `scale` is below one, and at least one pixel.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a side scaled by a factor below one, within u32 and above nought"
        )]
        let side = scaled as u32;
        side.clamp(1, n)
    };
    (side(w), side(h))
}

/// `pixels`, `from` in size, made `to` in size by averaging the pixels each
/// new one covers -- a box filter, which is what a picture made smaller
/// wants: every source pixel counts once, so nothing flickers in and out as
/// a film moves. Each channel is averaged alone, alpha too (the pixels are
/// straight, not premultiplied: a film's are opaque).
///
/// A size larger than `from`, or pixels that do not fill `from`, give
/// `pixels` back as they were.
#[must_use]
pub fn shrink(pixels: &[u32], from: (u32, u32), to: (u32, u32)) -> Vec<u32> {
    let ((w, h), (tw, th)) = (from, to);
    let fills =
        usize::try_from(u64::from(w).saturating_mul(u64::from(h))).is_ok_and(|n| n == pixels.len());
    if !fills || tw == 0 || th == 0 || tw > w || th > h {
        return pixels.to_vec();
    }
    let (w, h, tw, th) = (u64::from(w), u64::from(h), u64::from(tw), u64::from(th));
    // The source rows or columns new row or column `i` of `n` covers, of
    // `of` in all: never empty, as the new size is no larger.
    let span = |i: u64, n: u64, of: u64| {
        let start = i.saturating_mul(of).checked_div(n).unwrap_or(0);
        let end = i
            .saturating_add(1)
            .saturating_mul(of)
            .checked_div(n)
            .unwrap_or(0)
            .max(start.saturating_add(1))
            .min(of);
        start..end
    };
    let mut out = Vec::with_capacity(usize::try_from(tw.saturating_mul(th)).unwrap_or(0));
    for oy in 0..th {
        let rows = span(oy, th, h);
        for ox in 0..tw {
            let cols = span(ox, tw, w);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for y in rows.clone() {
                for x in cols.clone() {
                    let at = usize::try_from(y.saturating_mul(w).saturating_add(x))
                        .unwrap_or(usize::MAX);
                    let px = pixels.get(at).copied().unwrap_or(0);
                    for (shift, total) in [24u32, 16, 8, 0].into_iter().zip(sum.iter_mut()) {
                        *total = total.saturating_add(u64::from((px >> shift) & 0xFF));
                    }
                    count = count.saturating_add(1);
                }
            }
            let mut px = 0u32;
            for (shift, total) in [24u32, 16, 8, 0].into_iter().zip(sum) {
                // Rounded to nearest; a mean of bytes is a byte.
                let mean = total
                    .saturating_add(count / 2)
                    .checked_div(count)
                    .unwrap_or(0);
                px |= (u32::try_from(mean).unwrap_or(0xFF) & 0xFF) << shift;
            }
            out.push(px);
        }
    }
    out
}

/// When a frame `time` nanoseconds into the file is due, the playing having
/// (re)started at `start` with the frame at `first` on the file's clock. A
/// frame before `first` -- a file whose times step back -- is due at once.
#[must_use]
pub fn due(start: Instant, first: i64, time: i64) -> Instant {
    let after = u64::try_from(time.saturating_sub(first)).unwrap_or(0);
    start
        .checked_add(Duration::from_nanos(after))
        .unwrap_or(start)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
