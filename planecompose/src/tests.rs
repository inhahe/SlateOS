//! Host tests. Every expected pixel is computed by hand or by `reference`,
//! which is the definition written as plainly as possible: for every target
//! pixel, every layer, bottom first, with no clipping cleverness, no fast path
//! and no pages.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_arguments
)]

extern crate std;
use std::vec;
use std::vec::Vec;

use super::*;

const BG: u32 = 0xFF00_0000;

fn px(buf: &[u8], pitch: u32, x: u32, y: u32) -> u32 {
    let at = (y * pitch + x * 4) as usize;
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

/// A `w` x `h` framebuffer whose pixel at (x, y) is `f(x, y)`, with `extra`
/// bytes of padding per row so pitch != width * 4 is exercised.
fn fb(w: u32, h: u32, extra: u32, f: impl Fn(u32, u32) -> u32) -> (Vec<u8>, u32) {
    let pitch = w * 4 + extra;
    let mut v = vec![0xEEu8; (pitch * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let at = (y * pitch + x * 4) as usize;
            v[at..at + 4].copy_from_slice(&f(x, y).to_le_bytes());
        }
    }
    (v, pitch)
}

fn coord(x: u32, y: u32) -> u32 {
    0xFF00_0000 | (y << 12) | x
}

/// One layer as the tests describe it: (buffer, width, height, pitch, page
/// size, source rectangle, destination rectangle, blend).
type Spec<'a> = (&'a [u8], u32, u32, u32, usize, FixedRect, Rect, Blend);

fn fixed(x: u32, y: u32, w: u32, h: u32) -> FixedRect {
    FixedRect::pixels(x, y, w, h).unwrap()
}

/// Compose `layers` (given as (buffer, w, h, pitch, page size, src, dst,
/// blend)) into `out`, with the target split into pages of `tpage` bytes.
fn run(
    out: &mut [u8],
    tw: u32,
    th: u32,
    tp: u32,
    tpage: usize,
    damage: Rect,
    layers: &[Spec<'_>],
) -> Result<(), Error> {
    let page_lists: Vec<Vec<&[u8]>> = layers.iter().map(|l| l.0.chunks(l.4).collect()).collect();
    let built: Vec<Layer<'_>> = layers
        .iter()
        .zip(page_lists.iter())
        .map(|(l, pages)| Layer {
            source: Source {
                data: Paged {
                    pages,
                    page_size: l.4,
                },
                width: l.1,
                height: l.2,
                pitch: l.3,
            },
            src: l.5,
            dst: l.6,
            blend: l.7,
        })
        .collect();
    let mut tpages: Vec<&mut [u8]> = out.chunks_mut(tpage).collect();
    let mut t = Target {
        data: PagedMut {
            pages: &mut tpages,
            page_size: tpage,
        },
        width: tw,
        height: th,
        pitch: tp,
    };
    compose(&mut t, damage, &built, BG)
}

/// The definition, spelled out.
fn reference(
    tw: u32,
    th: u32,
    pitch: u32,
    init: &[u8],
    damage: Rect,
    layers: &[Spec<'_>],
) -> Vec<u8> {
    let mut out = init.to_vec();
    for y in 0..th {
        for x in 0..tw {
            let (xi, yi) = (i64::from(x), i64::from(y));
            let in_damage = xi >= i64::from(damage.x)
                && xi < i64::from(damage.x) + i64::from(damage.w)
                && yi >= i64::from(damage.y)
                && yi < i64::from(damage.y) + i64::from(damage.h);
            if !in_damage {
                continue;
            }
            let at = (y * pitch + x * 4) as usize;
            let mut cur = BG.to_le_bytes();
            for &(buf, _, _, sp, _, src, d, blend) in layers {
                let (dx, dy) = (xi - i64::from(d.x), yi - i64::from(d.y));
                if dx < 0 || dy < 0 || dx >= i64::from(d.w) || dy >= i64::from(d.h) {
                    continue;
                }
                let sx = sample(src.x, src.w, d.w, dx as u32);
                let sy = sample(src.y, src.h, d.h, dy as u32);
                let s = px(buf, sp, sx, sy).to_le_bytes();
                match blend {
                    Blend::Opaque => cur = s,
                    Blend::Premultiplied => {
                        let a = u32::from(s[3]);
                        for c in 0..4 {
                            let rest = (u32::from(cur[c]) * (255 - a) + 127) / 255;
                            cur[c] = (u32::from(s[c]) + rest).min(255) as u8;
                        }
                    }
                }
            }
            out[at..at + 4].copy_from_slice(&cur);
        }
    }
    out
}

// --- rectangles --------------------------------------------------------------

#[test]
fn intersect_overlapping_disjoint_and_touching() {
    let a = Rect::new(0, 0, 10, 10);
    assert_eq!(
        a.intersect(Rect::new(5, 5, 10, 10)),
        Some(Rect::new(5, 5, 5, 5))
    );
    assert_eq!(
        a.intersect(Rect::new(10, 0, 5, 5)),
        None,
        "touching edges share no pixel"
    );
    assert_eq!(
        a.intersect(Rect::new(-5, -5, 7, 7)),
        Some(Rect::new(0, 0, 2, 2))
    );
    assert_eq!(a.intersect(Rect::new(20, 20, 1, 1)), None);
    assert_eq!(
        a.intersect(Rect::new(2, 2, 0, 5)),
        None,
        "an empty rectangle meets nothing"
    );
}

#[test]
fn union_covers_both_and_ignores_empty() {
    let a = Rect::new(0, 0, 4, 4);
    assert_eq!(a.union(Rect::new(10, 2, 2, 2)), Rect::new(0, 0, 12, 4));
    assert_eq!(a.union(Rect::new(-3, -1, 1, 1)), Rect::new(-3, -1, 7, 5));
    assert_eq!(a.union(Rect::new(50, 50, 0, 9)), a);
    assert_eq!(Rect::new(1, 1, 0, 0).union(a), a);
}

// --- check_layer ----------------------------------------------------------------

#[test]
fn check_layer_accepts_the_whole_framebuffer_unscaled() {
    assert_eq!(
        check_layer(
            64,
            32,
            fixed(0, 0, 64, 32),
            Rect::new(0, 0, 64, 32),
            Limits::UNSCALED
        ),
        Ok(())
    );
}

#[test]
fn check_layer_refuses_empty_rectangles() {
    assert_eq!(
        check_layer(
            64,
            32,
            fixed(0, 0, 0, 32),
            Rect::new(0, 0, 64, 32),
            Limits::SOFTWARE
        ),
        Err(Error::EmptyRect)
    );
    assert_eq!(
        check_layer(
            64,
            32,
            fixed(0, 0, 64, 32),
            Rect::new(0, 0, 64, 0),
            Limits::SOFTWARE
        ),
        Err(Error::EmptyRect)
    );
}

#[test]
fn check_layer_refuses_a_source_leaving_its_framebuffer() {
    assert_eq!(
        check_layer(
            64,
            32,
            fixed(1, 0, 64, 32),
            Rect::new(0, 0, 64, 32),
            Limits::SOFTWARE
        ),
        Err(Error::SourceOutOfBounds)
    );
    assert_eq!(
        check_layer(
            64,
            32,
            fixed(0, 0, 64, 33),
            Rect::new(0, 0, 64, 33),
            Limits::SOFTWARE
        ),
        Err(Error::SourceOutOfBounds)
    );
    let src = FixedRect {
        x: 0x8000,
        y: 0,
        w: 64 << 16,
        h: 32 << 16,
    };
    assert_eq!(
        check_layer(64, 32, src, Rect::new(0, 0, 64, 32), Limits::SOFTWARE),
        Err(Error::SourceOutOfBounds),
        "by half a pixel"
    );
}

#[test]
fn check_layer_enforces_the_scale_limits() {
    let src = fixed(0, 0, 10, 10);
    assert_eq!(
        check_layer(10, 10, src, Rect::new(0, 0, 20, 10), Limits::UNSCALED),
        Err(Error::ScaleOutOfRange)
    );
    assert_eq!(
        check_layer(10, 10, src, Rect::new(0, 0, 160, 160), Limits::SOFTWARE),
        Ok(())
    );
    assert_eq!(
        check_layer(10, 10, src, Rect::new(0, 0, 161, 10), Limits::SOFTWARE),
        Err(Error::ScaleOutOfRange)
    );
    assert_eq!(
        check_layer(
            160,
            160,
            fixed(0, 0, 160, 160),
            Rect::new(0, 0, 10, 10),
            Limits::SOFTWARE
        ),
        Ok(())
    );
    assert_eq!(
        check_layer(
            170,
            10,
            fixed(0, 0, 170, 10),
            Rect::new(0, 0, 10, 10),
            Limits::SOFTWARE
        ),
        Err(Error::ScaleOutOfRange)
    );
}

#[test]
fn check_layer_refuses_oversized_or_zero_framebuffers() {
    assert_eq!(
        check_layer(
            0,
            10,
            fixed(0, 0, 1, 1),
            Rect::new(0, 0, 1, 1),
            Limits::SOFTWARE
        ),
        Err(Error::BadDimension)
    );
    assert_eq!(
        check_layer(
            MAX_DIMENSION + 1,
            10,
            fixed(0, 0, 1, 1),
            Rect::new(0, 0, 1, 1),
            Limits::SOFTWARE
        ),
        Err(Error::BadDimension)
    );
}

// --- sampling ---------------------------------------------------------------------

#[test]
fn sample_is_the_identity_when_unscaled() {
    for i in 0..50 {
        assert_eq!(sample(3 << 16, 50 << 16, 50, i), 3 + i);
    }
}

#[test]
fn sample_doubles_and_halves_exactly() {
    let up: Vec<u32> = (0..8).map(|i| sample(0, 4 << 16, 8, i)).collect();
    assert_eq!(up, [0, 0, 1, 1, 2, 2, 3, 3]);
    // down by two: the pixel under each destination centre, the odd one
    let down: Vec<u32> = (0..4).map(|i| sample(0, 8 << 16, 4, i)).collect();
    assert_eq!(down, [1, 3, 5, 7]);
}

#[test]
fn sample_honours_a_fractional_origin() {
    assert_eq!(sample(0x8000, 4 << 16, 4, 0), 1);
    assert_eq!(sample(0x7FFF, 4 << 16, 4, 0), 0);
}

// --- compose ------------------------------------------------------------------------

#[test]
fn identity_copies_the_framebuffer() {
    let (src, sp) = fb(16, 8, 12, coord);
    let (mut out, tp) = fb(16, 8, 0, |_, _| 0);
    run(
        &mut out,
        16,
        8,
        tp,
        64,
        Rect::new(0, 0, 16, 8),
        &[(
            &src,
            16,
            8,
            sp,
            76,
            fixed(0, 0, 16, 8),
            Rect::new(0, 0, 16, 8),
            Blend::Opaque,
        )],
    )
    .unwrap();
    for y in 0..8 {
        for x in 0..16 {
            assert_eq!(px(&out, tp, x, y), coord(x, y));
        }
    }
}

#[test]
fn crop_translate_and_background() {
    let (src, sp) = fb(16, 16, 4, coord);
    let (mut out, tp) = fb(12, 10, 8, |_, _| 0);
    run(
        &mut out,
        12,
        10,
        tp,
        1 << 20,
        Rect::new(0, 0, 12, 10),
        &[(
            &src,
            16,
            16,
            sp,
            1 << 20,
            fixed(4, 3, 4, 4),
            Rect::new(5, 2, 4, 4),
            Blend::Opaque,
        )],
    )
    .unwrap();
    for y in 0..10 {
        for x in 0..12 {
            let want = if (5..9).contains(&x) && (2..6).contains(&y) {
                coord(x - 5 + 4, y - 2 + 3)
            } else {
                BG
            };
            assert_eq!(px(&out, tp, x, y), want, "at ({x},{y})");
        }
    }
}

#[test]
fn a_layer_hanging_off_the_top_left_is_clipped() {
    let (src, sp) = fb(8, 8, 0, coord);
    let (mut out, tp) = fb(6, 6, 0, |_, _| 0);
    run(
        &mut out,
        6,
        6,
        tp,
        24,
        Rect::new(0, 0, 6, 6),
        &[(
            &src,
            8,
            8,
            sp,
            20,
            fixed(0, 0, 8, 8),
            Rect::new(-3, -2, 8, 8),
            Blend::Opaque,
        )],
    )
    .unwrap();
    assert_eq!(px(&out, tp, 0, 0), coord(3, 2));
    assert_eq!(px(&out, tp, 4, 5), coord(7, 7));
    assert_eq!(px(&out, tp, 5, 0), BG, "past the layer's right edge");
}

#[test]
fn scaling_up_and_down_by_two() {
    let (src, sp) = fb(4, 4, 0, coord);
    let (mut out, tp) = fb(8, 8, 0, |_, _| 0);
    run(
        &mut out,
        8,
        8,
        tp,
        12,
        Rect::new(0, 0, 8, 8),
        &[(
            &src,
            4,
            4,
            sp,
            8,
            fixed(0, 0, 4, 4),
            Rect::new(0, 0, 8, 8),
            Blend::Opaque,
        )],
    )
    .unwrap();
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(px(&out, tp, x, y), coord(x / 2, y / 2));
        }
    }
    let (big, bp) = fb(8, 8, 0, coord);
    let (mut small, stp) = fb(4, 4, 0, |_, _| 0);
    run(
        &mut small,
        4,
        4,
        stp,
        4,
        Rect::new(0, 0, 4, 4),
        &[(
            &big,
            8,
            8,
            bp,
            16,
            fixed(0, 0, 8, 8),
            Rect::new(0, 0, 4, 4),
            Blend::Opaque,
        )],
    )
    .unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(px(&small, stp, x, y), coord(2 * x + 1, 2 * y + 1));
        }
    }
}

#[test]
fn the_top_layer_wins_and_premultiplied_alpha_blends() {
    let (bottom, bp) = fb(4, 4, 0, |_, _| 0xFF20_4060);
    let (top, tpp) = fb(2, 2, 0, |_, _| 0x8040_4040); // 50% alpha, premultiplied
    let (mut out, tp) = fb(4, 4, 0, |_, _| 0);
    run(
        &mut out,
        4,
        4,
        tp,
        64,
        Rect::new(0, 0, 4, 4),
        &[
            (
                &bottom,
                4,
                4,
                bp,
                64,
                fixed(0, 0, 4, 4),
                Rect::new(0, 0, 4, 4),
                Blend::Opaque,
            ),
            (
                &top,
                2,
                2,
                tpp,
                16,
                fixed(0, 0, 2, 2),
                Rect::new(1, 1, 2, 2),
                Blend::Premultiplied,
            ),
        ],
    )
    .unwrap();
    assert_eq!(px(&out, tp, 0, 0), 0xFF20_4060, "outside the top layer");
    let over = |s: u32, d: u32| s + (d * (255 - 0x80) + 127) / 255;
    let want = (over(0x80, 0xFF) << 24)
        | (over(0x40, 0x20) << 16)
        | (over(0x40, 0x40) << 8)
        | over(0x40, 0x60);
    assert_eq!(px(&out, tp, 1, 1), want);
    assert_eq!(px(&out, tp, 2, 2), want);
}

#[test]
fn only_the_damaged_pixels_are_touched() {
    let (src, sp) = fb(8, 8, 0, coord);
    let (mut out, tp) = fb(8, 8, 0, |_, _| 0x1234_5678);
    run(
        &mut out,
        8,
        8,
        tp,
        32,
        Rect::new(2, 3, 3, 2),
        &[(
            &src,
            8,
            8,
            sp,
            32,
            fixed(0, 0, 8, 8),
            Rect::new(0, 0, 8, 8),
            Blend::Opaque,
        )],
    )
    .unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let inside = (2..5).contains(&x) && (3..5).contains(&y);
            assert_eq!(
                px(&out, tp, x, y),
                if inside { coord(x, y) } else { 0x1234_5678 },
                "at ({x},{y})"
            );
        }
    }
}

#[test]
fn a_short_buffer_is_refused_and_nothing_is_written() {
    let (src, sp) = fb(8, 8, 0, coord);
    let (mut out, tp) = fb(8, 8, 0, |_, _| 0x1111_1111);
    let short = &src[..src.len() - 4];
    let before = out.clone();
    assert_eq!(
        run(
            &mut out,
            8,
            8,
            tp,
            256,
            Rect::new(0, 0, 8, 8),
            &[(
                short,
                8,
                8,
                sp,
                256,
                fixed(0, 0, 8, 8),
                Rect::new(0, 0, 8, 8),
                Blend::Opaque
            )]
        ),
        Err(Error::BufferTooSmall)
    );
    assert_eq!(out, before);
    let mut short_target = vec![0u8; 8 * 8 * 4 - 4];
    assert_eq!(
        run(&mut short_target, 8, 8, 32, 256, Rect::new(0, 0, 8, 8), &[]),
        Err(Error::BufferTooSmall)
    );
}

#[test]
fn misaligned_pages_or_pitch_are_refused() {
    let mut out = vec![0u8; 8 * 8 * 4];
    assert_eq!(
        run(&mut out, 8, 8, 32, 30, Rect::new(0, 0, 8, 8), &[]),
        Err(Error::Unaligned),
        "a page size a pixel could straddle"
    );
    let mut out = vec![0u8; 8 * 9 * 4];
    assert_eq!(
        run(&mut out, 8, 8, 34, 64, Rect::new(0, 0, 8, 8), &[]),
        Err(Error::Unaligned),
        "a pitch a pixel could straddle"
    );
    // pages must all be the page size except the last
    let mut a = vec![0u8; 128];
    let mut b = vec![0u8; 64];
    let mut c = vec![0u8; 128];
    let mut pages: Vec<&mut [u8]> = vec![&mut a, &mut b, &mut c];
    let mut t = Target {
        data: PagedMut {
            pages: &mut pages,
            page_size: 128,
        },
        width: 8,
        height: 8,
        pitch: 32,
    };
    assert_eq!(
        compose(&mut t, Rect::new(0, 0, 8, 8), &[], BG),
        Err(Error::Unaligned)
    );
}

#[test]
fn damage_outside_the_target_draws_nothing() {
    let mut out = vec![7u8; 8 * 8 * 4];
    run(&mut out, 8, 8, 32, 256, Rect::new(100, 100, 5, 5), &[]).unwrap();
    assert!(out.iter().all(|&b| b == 7));
}

/// The destination pixels whose sample lands in `region`, by enumeration --
/// the definition `source_to_dest` must match exactly.
fn dest_by_enumeration(region: Rect, src: FixedRect, dst: Rect) -> Option<Rect> {
    let inside = |v: u32, start: i32, len: u32| {
        i64::from(v) >= i64::from(start) && i64::from(v) < i64::from(start) + i64::from(len)
    };
    let xs: Vec<u32> = (0..dst.w)
        .filter(|&i| inside(sample(src.x, src.w, dst.w, i), region.x, region.w))
        .collect();
    let ys: Vec<u32> = (0..dst.h)
        .filter(|&i| inside(sample(src.y, src.h, dst.h, i), region.y, region.h))
        .collect();
    let (x0, x1) = (*xs.first()?, *xs.last()? + 1);
    let (y0, y1) = (*ys.first()?, *ys.last()? + 1);
    Some(Rect::new(
        dst.x + x0 as i32,
        dst.y + y0 as i32,
        x1 - x0,
        y1 - y0,
    ))
}

#[test]
fn source_to_dest_unscaled_and_scaled_by_hand() {
    // unscaled, moved: the region shifts with the layer
    assert_eq!(
        source_to_dest(
            Rect::new(2, 3, 4, 5),
            fixed(0, 0, 10, 10),
            Rect::new(100, 200, 10, 10)
        ),
        Some(Rect::new(102, 203, 4, 5))
    );
    // doubled: each source pixel is two destination pixels
    assert_eq!(
        source_to_dest(
            Rect::new(1, 0, 2, 1),
            fixed(0, 0, 4, 4),
            Rect::new(0, 0, 8, 8)
        ),
        Some(Rect::new(2, 0, 4, 2))
    );
    // halved: source pixels 0..2 are sampled only by destination pixel 0 (it samples 1)
    assert_eq!(
        source_to_dest(
            Rect::new(0, 0, 2, 2),
            fixed(0, 0, 8, 8),
            Rect::new(0, 0, 4, 4)
        ),
        Some(Rect::new(0, 0, 1, 1))
    );
    // halved: source pixel 0 alone is never sampled
    assert_eq!(
        source_to_dest(
            Rect::new(0, 0, 1, 1),
            fixed(0, 0, 8, 8),
            Rect::new(0, 0, 4, 4)
        ),
        None
    );
    // a region outside the source rectangle draws nothing
    assert_eq!(
        source_to_dest(
            Rect::new(20, 20, 3, 3),
            fixed(0, 0, 10, 10),
            Rect::new(0, 0, 10, 10)
        ),
        None
    );
}

/// A deterministic generator, so a failure names a reproducible case.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % u64::from(n.max(1))) as u32
    }
    fn page(&mut self, len: usize) -> usize {
        // anything from one pixel to the whole buffer, always a multiple of 4
        match self.next(3) {
            0 => 4 * (1 + self.next(8) as usize),
            1 => 4 * (1 + self.next(64) as usize),
            _ => len.div_ceil(4) * 4 + 4,
        }
    }
}

#[test]
fn many_random_configurations_match_the_reference() {
    let mut r = Lcg(0x5EED);
    for case in 0..600 {
        let (tw, th) = (1 + r.next(40), 1 + r.next(30));
        let (sw, sh) = (1 + r.next(24), 1 + r.next(24));
        let (src_a, spa) = fb(sw, sh, r.next(3) * 4, coord);
        let (src_b, spb) = fb(sw, sh, 0, |x, y| {
            ((x * 37 + y * 11) % 256) << 24 | 0x0030_2010
        });
        let pa = r.page(src_a.len());
        let pb = r.page(src_b.len());
        let mut layers: Vec<Spec<'_>> = Vec::new();
        for (i, (buf, pitch, page)) in [(&src_a, spa, pa), (&src_b, spb, pb)]
            .into_iter()
            .enumerate()
        {
            let sx = r.next(sw);
            let sy = r.next(sh);
            let w = 1 + r.next(sw - sx);
            let h = 1 + r.next(sh - sy);
            let mut src = fixed(sx, sy, w, h);
            if r.next(4) == 0 && w > 1 {
                src.x += r.next(0x10000);
                src.w -= 0x10000;
            }
            let dst = Rect::new(
                r.next(tw + 10) as i32 - 5,
                r.next(th + 10) as i32 - 5,
                (1 + r.next(3 * w)).min(16 * w),
                (1 + r.next(3 * h)).min(16 * h),
            );
            if check_layer(sw, sh, src, dst, Limits::SOFTWARE).is_err() {
                continue;
            }
            let blend = if i == 0 {
                Blend::Opaque
            } else {
                Blend::Premultiplied
            };
            layers.push((buf, sw, sh, pitch, page, src, dst, blend));
        }
        let tp = tw * 4 + r.next(3) * 4;
        let init = vec![0x5Au8; (tp * th) as usize];
        let tpage = r.page(init.len());
        let damage = Rect::new(
            r.next(tw) as i32 - 2,
            r.next(th) as i32 - 2,
            1 + r.next(tw + 4),
            1 + r.next(th + 4),
        );
        let want = reference(tw, th, tp, &init, damage, &layers);
        let mut got = init.clone();
        run(&mut got, tw, th, tp, tpage, damage, &layers).unwrap();
        assert_eq!(
            got,
            want,
            "case {case}: target {tw}x{th} page {tpage} damage {damage:?} layers {:?}",
            layers
                .iter()
                .map(|l| (l.4, l.5, l.6, l.7))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn source_to_dest_matches_enumeration() {
    let mut r = Lcg(0xD157);
    for case in 0..500 {
        let (sw, sh) = (1 + r.next(30), 1 + r.next(30));
        let sx = r.next(sw);
        let sy = r.next(sh);
        let w = 1 + r.next(sw - sx);
        let h = 1 + r.next(sh - sy);
        let src = fixed(sx, sy, w, h);
        let dst = Rect::new(
            r.next(50) as i32 - 10,
            r.next(50) as i32 - 10,
            1 + r.next(4 * w).min(16 * w - 1),
            1 + r.next(4 * h).min(16 * h - 1),
        );
        if check_layer(sw, sh, src, dst, Limits::SOFTWARE).is_err() {
            continue;
        }
        let region = Rect::new(
            r.next(sw + 4) as i32 - 2,
            r.next(sh + 4) as i32 - 2,
            1 + r.next(8),
            1 + r.next(8),
        );
        assert_eq!(
            source_to_dest(region, src, dst),
            dest_by_enumeration(region, src, dst),
            "case {case}: region {region:?} src {src:?} dst {dst:?}"
        );
    }
}

#[test]
fn pixels_refuses_rather_than_narrows() {
    // In range: exact, including the limit itself.
    assert_eq!(
        FixedRect::pixels(1, 2, 3, 4),
        Ok(FixedRect {
            x: 1 << 16,
            y: 2 << 16,
            w: 3 << 16,
            h: 4 << 16
        })
    );
    let edge = FixedRect::pixels(MAX_DIMENSION, 0, MAX_DIMENSION, 1).unwrap();
    assert_eq!(edge.x, MAX_DIMENSION << 16);
    assert!(
        edge.x.checked_add(edge.w).is_some(),
        "x + w must fit in u32 at the limit"
    );
    assert_eq!(
        FixedRect::whole(MAX_DIMENSION, MAX_DIMENSION).map(|r| r.w),
        Ok(MAX_DIMENSION << 16)
    );
    // One past, in each position: refused, never capped to the limit (a
    // capped width at the edge of a full-width framebuffer would pass
    // check_layer and show something narrower than was asked for).
    for (x, y, w, h) in [
        (MAX_DIMENSION + 1, 0, 1, 1),
        (0, MAX_DIMENSION + 1, 1, 1),
        (0, 0, MAX_DIMENSION + 1, 1),
        (0, 0, 1, MAX_DIMENSION + 1),
        (u32::MAX, 0, 1, 1),
        (0, 0, u32::MAX, u32::MAX),
    ] {
        assert_eq!(
            FixedRect::pixels(x, y, w, h),
            Err(Error::BadDimension),
            "({x}, {y}, {w}, {h})"
        );
    }
    assert_eq!(
        FixedRect::whole(MAX_DIMENSION + 1, 1),
        Err(Error::BadDimension)
    );
}
