//! Drawing an icon stays cheap enough for a desktop that draws dozens of them.
//!
//! Every icon the shell shows is an SVG drawn by `guitk::svg` -- the start
//! menu's, the taskbar's, a folder of files in the file manager -- at login,
//! at every theme change, and whenever one is asked for at a new size. The
//! renderer learned gradients, `<use>`, clip paths, viewport clipping and
//! masks on 2026-10-01, each of which puts work on every pixel it covers: a
//! gradient colour per pixel, a mask byte per pixel per clip, a mask's content
//! drawn a second time. This measures the five shapes of icon that work
//! produces, at a taskbar's size and a large one.
//!
//! # What it cost when it was written
//!
//! A debug build on a developer machine busy with a boot test, 2026-10-01,
//! microseconds a draw:
//!
//! | icon | at 48 px | at 256 px |
//! |---|---|---|
//! | flat colours | 1 196 | 20 470 |
//! | gradients | 1 418 | 34 475 |
//! | clipped | 2 126 | 43 269 |
//! | reused | 1 666 | 21 269 |
//! | masked (measured with masks, later the same day) | 2 444 | 56 312 |
//!
//! # Why the ceilings are where they are
//!
//! Twenty times the measured figure, as `gui/appearance`'s
//! `resolve_cost.rs` sets its own, for the same reasons: this is wall-clock
//! on a loaded developer machine in a debug build, and the regressions worth
//! catching are orders of magnitude -- a mask rebuilt per shape instead of per
//! clip, a gradient's stops searched per pixel per stop, a `<use>` expanded
//! afresh each time it is drawn. The measurement is the best of three runs,
//! since load can only push a sample up.
// A benchmark divides and asserts on the result; the defensive lints that
// forbid that in production code are off here, as `CLAUDE.md` prescribes for
// test code.
#![allow(clippy::arithmetic_side_effects, clippy::panic, clippy::unwrap_used)]

use guitk::svg::SvgDocument;
use std::time::Instant;

/// Microseconds to draw `svg` at `size` by `size`, the best of three runs of
/// `n` draws each, after one draw to warm up.
fn per_draw(svg: &str, size: u32, n: u32) -> f64 {
    let doc = SvgDocument::parse(svg).unwrap();
    std::hint::black_box(doc.render(size, size));
    (0..3)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..n {
                std::hint::black_box(doc.render(size, size));
            }
            start.elapsed().as_secs_f64() * 1e6 / f64::from(n)
        })
        .fold(f64::INFINITY, f64::min)
}

/// A folder icon as icon themes draw one: a dozen paths, flat colours.
const FLAT: &str = r##"<svg viewBox="0 0 48 48"><path d="M4 10 h14 l4 4 h22 v28 h-40 z" fill="#3a6ea5"/>
<path d="M4 16 h40 v26 h-40 z" fill="#5b8fd1"/><path d="M8 20 h32 v2 h-32 z" fill="#8bb3e3"/>
<circle cx="24" cy="30" r="6" fill="#ffffff" opacity="0.4"/><rect x="10" y="36" width="28" height="2" rx="1" fill="#2b5280"/>
<path d="M6 12 q2 -2 4 0 t4 0" stroke="#1d3c60" fill="none"/><ellipse cx="24" cy="44" rx="18" ry="2" fill="#000" opacity="0.2"/>
<path d="M12 24 l4 4 l8 -8" stroke="#fff" stroke-width="2" fill="none" stroke-linecap="round"/>
<polygon points="30,24 36,24 33,30" fill="#d4e4f7"/><polyline points="14,32 20,34 26,32" stroke="#fff" fill="none"/>
<line x1="8" y1="40" x2="40" y2="40" stroke="#1d3c60"/><rect x="18" y="8" width="12" height="4" fill="#2b5280"/></svg>"##;

/// The same icon shaded with gradients, linear and radial, as full-colour
/// themes draw theirs.
const GRADIENTS: &str = r##"<svg viewBox="0 0 48 48"><defs>
<linearGradient id="back" x2="0" y2="1"><stop stop-color="#3a6ea5"/><stop offset="1" stop-color="#1d3c60"/></linearGradient>
<linearGradient id="front" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#8bb3e3"/><stop offset="0.5" stop-color="#5b8fd1"/><stop offset="1" stop-color="#3a6ea5"/></linearGradient>
<radialGradient id="glow" fx="0.3" fy="0.3"><stop stop-color="#fff" stop-opacity="0.8"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient>
</defs><path d="M4 10 h14 l4 4 h22 v28 h-40 z" fill="url(#back)"/>
<path d="M4 16 h40 v26 h-40 z" fill="url(#front)" stroke="url(#back)"/>
<circle cx="24" cy="30" r="12" fill="url(#glow)"/><rect x="10" y="36" width="28" height="4" rx="2" fill="url(#back)"/></svg>"##;

/// Shapes cut by clip paths, both units, one clip inside another.
const CLIPPED: &str = r##"<svg viewBox="0 0 48 48"><defs>
<clipPath id="round"><circle cx="24" cy="24" r="20"/></clipPath>
<clipPath id="half" clipPathUnits="objectBoundingBox"><rect width="1" height="0.5"/></clipPath>
</defs><g clip-path="url(#round)"><rect width="48" height="48" fill="#3a6ea5"/>
<rect y="24" width="48" height="24" fill="#5b8fd1" clip-path="url(#half)"/>
<path d="M0 30 q24 -12 48 0 v18 h-48 z" fill="#8bb3e3"/></g></svg>"##;

/// A symbol drawn twenty times by `<use>`, each in a viewport of its own.
const REUSED: &str = r##"<svg viewBox="0 0 48 48"><symbol id="dot" viewBox="0 0 10 10">
<circle cx="5" cy="5" r="4" fill="#3a6ea5"/><circle cx="5" cy="5" r="2" fill="#fff"/></symbol>
<use href="#dot" x="0" y="0" width="9" height="9"/><use href="#dot" x="10" y="0" width="9" height="9"/>
<use href="#dot" x="20" y="0" width="9" height="9"/><use href="#dot" x="30" y="0" width="9" height="9"/>
<use href="#dot" x="0" y="10" width="9" height="9"/><use href="#dot" x="10" y="10" width="9" height="9"/>
<use href="#dot" x="20" y="10" width="9" height="9"/><use href="#dot" x="30" y="10" width="9" height="9"/>
<use href="#dot" x="0" y="20" width="9" height="9"/><use href="#dot" x="10" y="20" width="9" height="9"/>
<use href="#dot" x="20" y="20" width="9" height="9"/><use href="#dot" x="30" y="20" width="9" height="9"/>
<use href="#dot" x="0" y="30" width="9" height="9"/><use href="#dot" x="10" y="30" width="9" height="9"/>
<use href="#dot" x="20" y="30" width="9" height="9"/><use href="#dot" x="30" y="30" width="9" height="9"/>
<use href="#dot" x="38" y="38" width="9" height="9"/><use href="#dot" x="38" y="0" width="9" height="9"/>
<use href="#dot" x="0" y="38" width="9" height="9"/><use href="#dot" x="19" y="38" width="9" height="9"/></svg>"##;

/// A group faded through a gradient mask, as glossy icons draw their shine:
/// its content drawn once into a scratch surface and once through the mask.
const MASKED: &str = r##"<svg viewBox="0 0 48 48"><defs>
<linearGradient id="fade" x2="0" y2="1"><stop stop-color="#fff"/><stop offset="1" stop-color="#000"/></linearGradient>
<mask id="shine"><rect width="48" height="48" fill="url(#fade)"/></mask></defs>
<rect width="48" height="48" rx="8" fill="#3a6ea5"/>
<g mask="url(#shine)"><ellipse cx="24" cy="12" rx="20" ry="10" fill="#fff"/><path d="M4 10 h40 v6 h-40 z" fill="#8bb3e3"/></g></svg>"##;

/// **Drawing an icon stays cheap**: each shape of icon, at a taskbar's 48
/// pixels and at 256, under its ceiling.
#[test]
fn drawing_an_icon_stays_cheap() {
    // Ceilings in microseconds: twenty times what was measured.
    let cases: [(&str, &str, u32, u32, f64); 10] = [
        ("masked", MASKED, 48, 20, 50_000.0),
        ("masked", MASKED, 256, 5, 1_100_000.0),
        ("flat", FLAT, 48, 20, 25_000.0),
        ("flat", FLAT, 256, 5, 400_000.0),
        ("gradients", GRADIENTS, 48, 20, 30_000.0),
        ("gradients", GRADIENTS, 256, 5, 700_000.0),
        ("clipped", CLIPPED, 48, 20, 45_000.0),
        ("clipped", CLIPPED, 256, 5, 900_000.0),
        ("reused", REUSED, 48, 20, 35_000.0),
        ("reused", REUSED, 256, 5, 450_000.0),
    ];
    for (name, svg, size, n, ceiling) in cases {
        let us = per_draw(svg, size, n);
        println!("{name} at {size}: {us:.0} us (ceiling {ceiling:.0})");
        assert!(
            us < ceiling,
            "{name} at {size}: {us:.0} us, over {ceiling:.0}"
        );
    }
}
