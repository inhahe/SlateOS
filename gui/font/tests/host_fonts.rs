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

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        match Face::parse(data) {
            Ok(face) => {
                opened += 1;
                // Every glyph must decode without panicking. Cap the count so
                // a CJK face with 60k glyphs doesn't dominate the runtime.
                let cap = face.num_glyphs().min(2000);
                let mut drew_something = false;
                for gid in 0..cap {
                    let Ok(outline) = face.outline(gid) else {
                        continue;
                    };
                    glyphs_walked += 1;
                    if outline
                        .commands
                        .iter()
                        .any(|c| matches!(c, PathCmd::MoveTo(_)))
                    {
                        drew_something = true;
                    }
                    let _ = face.advance(gid);
                    let _ = face.left_side_bearing(gid);
                }
                assert!(
                    drew_something || face.num_glyphs() <= 1,
                    "{}: opened but produced no ink for any of {cap} glyphs",
                    path.display()
                );
                // A face with a character map must resolve at least one of
                // the most common letters in any Latin-script font.
                if face.has_cmap() {
                    let hits = ['A', 'a', 'e', 'o', '0', ' ']
                        .iter()
                        .filter(|c| face.glyph_index(**c).is_some())
                        .count();
                    if hits == 0 {
                        // Symbol and icon fonts legitimately map nothing in
                        // ASCII; only report, don't fail.
                        println!("  note: {} maps no ASCII letters", path.display());
                    }
                }
            }
            Err(SfntError::CffOutlinesUnsupported) => cff += 1,
            Err(e) => other_err.push((path.clone(), e)),
        }
    }

    println!("fonts found:      {}", files.len());
    println!("opened:           {opened}");
    println!("CFF (unsupported): {cff}");
    println!("glyph outlines walked: {glyphs_walked}");
    for (path, e) in &other_err {
        println!("  error: {} -> {e}", path.display());
    }

    assert!(opened > 0, "not one installed font opened");
}
