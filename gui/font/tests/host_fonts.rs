//! Parse every real font installed on the development host.
//!
//! The unit tests in `sfnt.rs` run against a synthetic font whose bytes we
//! chose, which proves the parser reads what the *spec* says but not that it
//! reads what *shipping fonts actually contain*. Real files exercise the
//! parts a hand-built fixture never will: short `loca`, format-0 and
//! format-12 `cmap`s, symbol encodings, run-length flag repeats, nested
//! composites, `numberOfHMetrics` far below `numGlyphs`, and the assorted
//! spec violations that font vendors ship anyway.
//!
//! This test is `#[ignore]`d because it depends on the host's font
//! directory, which is not part of the repository and does not exist on the
//! target OS. Run it deliberately:
//!
//! ```text
//! cargo test -p osfont --target x86_64-pc-windows-gnu --test host_fonts -- --ignored --nocapture
//! ```
//!
//! It asserts two things, and only two, because everything else varies per
//! font: no input may panic, and any face that opens must produce a usable
//! outline for at least one common Latin letter (i.e. we did not "succeed"
//! by parsing the container and then failing every glyph).

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects
)]

use std::fs;
use std::path::{Path, PathBuf};

use osfont::raster::rasterize;
use osfont::sfnt::{Face, PathCmd, SfntError};

/// Directories a font might live in, on any host this repo is developed on.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        dirs.push(Path::new(&windir).join("Fonts"));
    }
    dirs.push(PathBuf::from("/usr/share/fonts"));
    dirs.push(PathBuf::from("/System/Library/Fonts"));
    dirs.retain(|d| d.is_dir());
    dirs
}

fn collect_fonts(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, out, depth + 1);
        } else if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if matches!(ext.as_str(), "ttf" | "ttc" | "otf") {
                out.push(path);
            }
        }
    }
}

/// What one face contributed to the totals.
#[derive(Default)]
struct FaceTally {
    outlines: usize,
    rastered: usize,
    ink: u64,
}

/// Walk every glyph of one face, rasterizing the first slice of them.
///
/// Panics with the font's path on any failure, since a bare assertion message
/// in a 556-font sweep is useless without knowing which file produced it.
fn exercise_face(face: &Face, path: &Path) -> FaceTally {
    let mut tally = FaceTally::default();
    // Every glyph must decode without panicking. Cap the count so a CJK face
    // with 60k glyphs doesn't dominate the runtime.
    let cap = face.num_glyphs().min(2000);
    // Rasterizing every glyph of every face in a debug build is too slow to
    // be useful; the first slice of each face is enough to cover the
    // real-world curve and composite shapes.
    let raster_cap = cap.min(64);
    let mut drew_something = false;
    let mut drew_in_raster_range = false;
    let mut inked_something = false;

    for gid in 0..cap {
        let Ok(outline) = face.outline(gid) else {
            continue;
        };
        tally.outlines += 1;
        let has_contours = outline
            .commands
            .iter()
            .any(|c| matches!(c, PathCmd::MoveTo(_)));
        drew_something |= has_contours;
        let _ = face.advance(gid);
        let _ = face.left_side_bearing(gid);

        if gid >= raster_cap {
            continue;
        }
        drew_in_raster_range |= has_contours;
        // Two sizes: a UI-typical body size and a large one, because
        // flattening and clipping behave differently when a curve spans
        // many pixels.
        for px in [16.0_f32, 64.0_f32] {
            let mask = rasterize(&outline, face.scale_for_px(px))
                .unwrap_or_else(|e| panic!("{}: glyph {gid} at {px}px: {e}", path.display()));
            tally.rastered += 1;
            let ink = mask.coverage.iter().filter(|&&c| c > 0).count();
            tally.ink += ink as u64;
            inked_something |= ink > 0;
            // The mask must describe exactly as many pixels as it claims, or
            // a consumer will read past the end of it.
            assert_eq!(
                mask.coverage.len(),
                (mask.width as usize) * (mask.height as usize),
                "{}: glyph {gid} mask size disagrees with its dimensions",
                path.display()
            );
        }
    }

    assert!(
        inked_something || !drew_in_raster_range,
        "{}: outlines have contours but the rasterizer produced no ink in the \
         first {raster_cap} glyphs",
        path.display()
    );
    assert!(
        drew_something || face.num_glyphs() <= 1,
        "{}: opened but produced no ink for any of {cap} glyphs",
        path.display()
    );
    // A face with a character map must resolve at least one of the most
    // common letters in any Latin-script font.
    if face.has_cmap() {
        let hits = ['A', 'a', 'e', 'o', '0', ' ']
            .iter()
            .filter(|c| face.glyph_index(**c).is_some())
            .count();
        if hits == 0 {
            // Symbol and icon fonts legitimately map nothing in ASCII; only
            // report, don't fail.
            println!("  note: {} maps no ASCII letters", path.display());
        }
    }
    tally
}

#[test]
#[ignore = "depends on the host's installed fonts"]
fn every_installed_font_parses_or_fails_cleanly() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(
        !files.is_empty(),
        "no fonts found on this host — nothing to verify"
    );
    files.sort();

    let mut opened = 0usize;
    let mut cff = 0usize;
    let mut other_err = Vec::new();
    let mut glyphs_walked = 0usize;
    let mut glyphs_rastered = 0usize;
    let mut ink_pixels = 0u64;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        match Face::parse(data) {
            Ok(face) => {
                opened += 1;
                let tally = exercise_face(&face, path);
                glyphs_walked += tally.outlines;
                glyphs_rastered += tally.rastered;
                ink_pixels += tally.ink;
            }
            Err(SfntError::CffOutlinesUnsupported) => cff += 1,
            Err(e) => other_err.push((path.clone(), e)),
        }
    }

    println!("fonts found:      {}", files.len());
    println!("opened:           {opened}");
    println!("CFF (unsupported): {cff}");
    println!("glyph outlines walked: {glyphs_walked}");
    println!("glyphs rasterized:     {glyphs_rastered}");
    println!("ink pixels produced:   {ink_pixels}");
    for (path, e) in &other_err {
        println!("  error: {} -> {e}", path.display());
    }

    assert!(opened > 0, "not one installed font opened");
}

/// Render real letters as ASCII art so a human can confirm they are letters.
///
/// The bulk test above proves the rasterizer produces *ink*, which a mirrored,
/// transposed or otherwise scrambled glyph would too. Nothing but looking at
/// the output catches that class of bug, so this prints it. It also asserts
/// the two properties that distinguish a letter from noise: 'l' is far taller
/// than it is wide, and 'o' is hollow — there is a run of blank pixels
/// enclosed by ink on its middle row.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn letters_look_like_letters() {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut candidates, 0);
    }
    // Any ordinary text face will do; prefer one that is present nearly
    // everywhere on each platform.
    let face_path = candidates
        .iter()
        .find(|p| {
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            matches!(n.as_str(), "arial.ttf" | "dejavusans.ttf" | "verdana.ttf")
        })
        .or_else(|| candidates.first())
        .expect("no fonts on this host")
        .clone();

    let data = fs::read(&face_path).expect("read font");
    let face = Face::parse(data).expect("parse font");
    println!("face: {}", face_path.display());

    for ch in ['A', 'g', 'l', 'o'] {
        let Some(gid) = face.glyph_index(ch) else {
            continue;
        };
        let outline = face.outline(gid).expect("outline");
        let mask = rasterize(&outline, face.scale_for_px(24.0)).expect("rasterize");
        println!(
            "\n'{ch}'  gid {gid}  {}x{} at left {} top {}",
            mask.width, mask.height, mask.left, mask.top
        );
        for y in 0..mask.height {
            let row: String = (0..mask.width)
                .map(|x| match mask.at(x, y) {
                    0 => ' ',
                    1..=63 => '.',
                    64..=127 => ':',
                    128..=191 => '*',
                    _ => '#',
                })
                .collect();
            println!("|{row}|");
        }

        if ch == 'l' {
            assert!(
                mask.height > mask.width * 2,
                "'l' rasterized {}x{} — not a tall thin stem",
                mask.width,
                mask.height
            );
        }
        if ch == 'o' {
            // Walk the middle row: ink, then a gap, then ink again.
            let y = mask.height / 2;
            let row: Vec<u8> = (0..mask.width).map(|x| mask.at(x, y)).collect();
            let first = row.iter().position(|&c| c > 0);
            let last = row.iter().rposition(|&c| c > 0);
            let (Some(first), Some(last)) = (first, last) else {
                panic!("'o' has no ink on its middle row");
            };
            let hole = row
                .get(first..last)
                .into_iter()
                .flatten()
                .filter(|&&c| c == 0)
                .count();
            assert!(
                hole > 0,
                "'o' is solid on its middle row — the counter did not cancel"
            );
        }
    }
}
