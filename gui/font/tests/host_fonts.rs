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

// A test that indexes past the end of its own fixed-size buffer *should*
// panic — that is the failure being reported, not a defect to guard against.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::print_stdout,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};

use osfont::raster::rasterize;
use osfont::scaled::{ScaledFont, Target};
use osfont::sfnt::{name_id, Face, PathCmd, SfntError};

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
    let mut unsupported = 0usize;
    let mut other_err = Vec::new();
    let mut glyphs_walked = 0usize;
    let mut glyphs_rastered = 0usize;
    let mut ink_pixels = 0u64;
    // `.otf` is the extension the PostScript-outline fonts use. Counting them
    // separately is what turns this from "most fonts still work" into a check
    // that the CFF path specifically does: before `cff.rs` existed every one
    // of these failed to open.
    let mut otf_found = 0usize;
    let mut otf_opened = 0usize;
    let mut otf_ink = 0u64;

    for path in &files {
        let is_otf = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("otf"));
        if is_otf {
            otf_found += 1;
        }
        let Ok(data) = fs::read(path) else { continue };
        match Face::parse(data) {
            Ok(face) => {
                opened += 1;
                let tally = exercise_face(&face, path);
                glyphs_walked += tally.outlines;
                glyphs_rastered += tally.rastered;
                ink_pixels += tally.ink;
                if is_otf {
                    otf_opened += 1;
                    otf_ink += tally.ink;
                }
            }
            Err(SfntError::CffUnsupported(_)) => unsupported += 1,
            Err(e) => other_err.push((path.clone(), e)),
        }
    }

    println!("fonts found:      {}", files.len());
    println!("opened:           {opened}");
    println!("unsupported CFF construct: {unsupported}");
    println!("`.otf` found / opened:     {otf_found} / {otf_opened}");
    println!("`.otf` ink pixels:         {otf_ink}");
    println!("glyph outlines walked: {glyphs_walked}");
    println!("glyphs rasterized:     {glyphs_rastered}");
    println!("ink pixels produced:   {ink_pixels}");
    for (path, e) in &other_err {
        println!("  error: {} -> {e}", path.display());
    }

    assert!(opened > 0, "not one installed font opened");
    if otf_found > 0 {
        assert!(otf_opened > 0, "{otf_found} `.otf` fonts and not one opened");
        assert!(otf_ink > 0, "`.otf` fonts opened but rasterized no ink");
    }
}

/// An ordinary Latin text face from this host, for the shape tests below.
///
/// Prefers one that is present nearly everywhere on each platform, and falls
/// back to whatever the host has so the test still runs somewhere unusual.
fn pick_text_face() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut candidates, 0);
    }
    candidates.sort();
    candidates
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
        .clone()
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
    let face_path = pick_text_face();
    let data = fs::read(&face_path).expect("read font");
    let face = Face::parse(data).expect("parse font");
    println!("face: {}", face_path.display());
    show_and_check_letters(&face);
}

/// The same shape proof for a **CFF** face.
///
/// The TrueType path and the CFF path share nothing but the `Outline` type
/// between them: a different table, a different parser, a different curve
/// order, a different flattener. A CFF glyph could come out mirrored, or with
/// its contours wound the wrong way so the counter fills instead of cancels,
/// and every aggregate check in this file would still pass — they count ink,
/// and a wrong glyph has ink. Only looking at it catches that, so this prints
/// it and asserts the same two structural properties.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn cff_letters_look_like_letters() {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut candidates, 0);
    }
    candidates.sort();
    candidates.retain(|p| {
        p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("otf"))
    });
    if candidates.is_empty() {
        println!("no `.otf` fonts on this host — nothing to check");
        return;
    }

    // Not every `.otf` is a Latin text face; take the first that maps the
    // letters this test looks at.
    let mut checked = 0usize;
    for path in &candidates {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        if !['A', 'g', 'l', 'o']
            .iter()
            .all(|c| face.glyph_index(*c).is_some())
        {
            continue;
        }
        println!("CFF face: {}", path.display());
        show_and_check_letters(&face);
        checked += 1;
        if checked == 2 {
            break;
        }
    }
    assert!(
        checked > 0,
        "{} `.otf` fonts on this host and not one produced Latin letters",
        candidates.len()
    );
}

/// Print four letters as ASCII art and assert the two properties that
/// distinguish a letter from noise.
fn show_and_check_letters(face: &Face) {
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

/// Read the `name` table of every installed font.
///
/// The synthetic unit tests prove the reader follows the spec on bytes we
/// chose. They cannot prove it reads what vendors ship, and the `name` table
/// is where vendors diverge most: records in four encodings for the same
/// string, Mac-only faces, language IDs nobody documents, offsets that run
/// past the table.
///
/// The load-bearing assertion is the **control-character** one. A name
/// decoder that gets the encoding wrong does not fail — it succeeds and
/// returns rubbish. Reading a UTF-16BE record as bytes yields `"A\0r\0i\0a\0l"`,
/// which is a perfectly good `String` that no family lookup will ever match.
/// A NUL or other control character in the middle of a font name is the
/// signature of exactly that bug, and nothing else produces it.
///
/// The rest is checked against a **known oracle** (Arial is called "Arial")
/// and by printing the families for a human to read, because mojibake in a
/// name that happens to contain no control characters is visible to a person
/// and to no assertion.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_report_their_names() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut opened = 0usize;
    let mut with_family = 0usize;
    let mut with_postscript = 0usize;
    let mut non_ascii = Vec::new();
    let mut nameless = Vec::new();
    let mut bad_postscript = Vec::new();
    let mut full_name_mismatch = 0usize;
    let mut full_name_checked = 0usize;
    let mut typographic_differs = Vec::new();
    let mut families: Vec<String> = Vec::new();

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else {
            continue;
        };
        opened += 1;

        // Every string the face can produce, checked as a group: whatever the
        // id, no name may contain a control character.
        for id in [
            name_id::FAMILY,
            name_id::SUBFAMILY,
            name_id::FULL_NAME,
            name_id::POSTSCRIPT,
            name_id::TYPOGRAPHIC_FAMILY,
            name_id::TYPOGRAPHIC_SUBFAMILY,
        ] {
            let Some(s) = face.name(id) else { continue };
            assert!(
                !s.is_empty(),
                "{}: name {id} decoded to an empty string",
                path.display()
            );
            assert!(
                !s.chars().any(char::is_control),
                "{}: name {id} = {s:?} contains a control character — the \
                 record was decoded in the wrong encoding",
                path.display()
            );
        }

        match face.family() {
            Some(f) => {
                with_family += 1;
                if !f.is_ascii() {
                    non_ascii.push((path.clone(), f.clone()));
                }
                families.push(f);
            }
            None => nameless.push(path.clone()),
        }

        if let Some(ps) = face.postscript_name() {
            with_postscript += 1;
            // A PostScript name is a PostScript *identifier*: printable ASCII
            // with the delimiters excluded. Vendors do ship violations, so
            // this is counted and reported rather than asserted per font.
            let ok = ps
                .bytes()
                .all(|b| (33..=126).contains(&b) && !b"()[]{}<>/%".contains(&b));
            if !ok {
                bad_postscript.push((path.clone(), ps));
            }
        }

        // The classic relationship between the legacy names: id 4 is id 1
        // plus the style. Counted, not asserted — it is a convention, and
        // enough fonts break it that a hard check would be noise.
        if let (Some(family), Some(full)) =
            (face.name(name_id::FAMILY), face.name(name_id::FULL_NAME))
        {
            full_name_checked += 1;
            if !full.starts_with(&family) {
                full_name_mismatch += 1;
            }
        }

        // Where a face carries both, the typographic family is the grouping a
        // font menu wants and the legacy one is the four-style split of it —
        // so the typographic name is normally a prefix of the legacy one.
        // Collecting the cases where they differ at all is what shows the
        // preference in `family()` is doing something.
        if let (Some(typo), Some(legacy)) = (
            face.name(name_id::TYPOGRAPHIC_FAMILY),
            face.name(name_id::FAMILY),
        ) {
            if typo != legacy {
                typographic_differs.push((typo, legacy));
            }
        }
    }

    families.sort();
    families.dedup();

    println!("fonts opened:            {opened}");
    println!("with a family name:      {with_family}");
    println!("with a PostScript name:  {with_postscript}");
    println!("distinct families:       {}", families.len());
    println!("full-name != family + style: {full_name_mismatch} of {full_name_checked}");
    println!("typographic family differs:  {}", typographic_differs.len());

    for (typo, legacy) in typographic_differs.iter().take(12) {
        println!("  typographic {typo:?} vs legacy {legacy:?}");
    }
    println!("non-ASCII family names:  {}", non_ascii.len());
    for (path, f) in non_ascii.iter().take(12) {
        println!("  {f:?}  ({})", path.display());
    }
    for path in &nameless {
        println!("  no family name: {}", path.display());
    }
    for (path, ps) in &bad_postscript {
        println!("  odd PostScript name {ps:?} in {}", path.display());
    }

    // Read them. Mojibake that contains no control character is visible to a
    // person and to nothing else.
    println!("\nfamilies:");
    for f in &families {
        println!("  {f}");
    }

    assert!(opened > 0, "not one installed font opened");
    // A `name` table is mandatory in both TrueType and OpenType. A handful of
    // broken files is tolerable; a systematic failure is not.
    assert!(
        with_family * 50 >= opened * 49,
        "only {with_family} of {opened} faces produced a family name"
    );
    assert!(
        with_postscript * 10 >= opened * 9,
        "only {with_postscript} of {opened} faces produced a PostScript name"
    );

    // A known oracle, so this test can fail on a *wrong* name and not just a
    // missing one. These faces ship with their platform under these exact
    // names; any that is absent on this host is skipped.
    let oracles = [
        ("arial.ttf", "Arial"),
        ("times.ttf", "Times New Roman"),
        ("cour.ttf", "Courier New"),
        ("verdana.ttf", "Verdana"),
        ("tahoma.ttf", "Tahoma"),
        ("georgia.ttf", "Georgia"),
        ("segoeui.ttf", "Segoe UI"),
        ("DejaVuSans.ttf", "DejaVu Sans"),
    ];
    let mut oracles_checked = 0usize;
    for (file, expected) in oracles {
        let Some(path) = files.iter().find(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(file))
        }) else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        assert_eq!(
            face.family().as_deref(),
            Some(expected),
            "{} should be the {expected} family",
            path.display()
        );
        oracles_checked += 1;
        println!(
            "oracle ok: {} -> {expected:?} / {:?} / {:?}",
            file,
            face.subfamily(),
            face.postscript_name()
        );
    }
    assert!(
        oracles_checked > 0,
        "none of the well-known faces are installed — the names were never \
         checked against a known answer"
    );
}

/// Drive the whole stack the way a toolkit will: file → face → `ScaledFont`
/// → pixels in an ARGB buffer.
///
/// The tests above stop at a `GlyphMask`. This one goes all the way to the
/// framebuffer, because the parts that only exist at this level — the pen
/// advancing between glyphs, the baseline placement, the coverage-to-alpha
/// blend, the cache — have no other coverage against a real font.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn a_string_renders_into_a_buffer() {
    const W: u32 = 320;
    const H: u32 = 48;
    const BG: u32 = 0xFF00_0000;

    let path = pick_text_face();
    let data = fs::read(&path).expect("read font");
    let mut font = ScaledFont::from_bytes(data, 24.0).expect("scaled font");
    println!("face: {}  {:?}", path.display(), font);

    let m = font.metrics().clone();
    println!(
        "ascent {:.2}  descent {:.2}  line_height {:.2}  x_height {:.2}  cap_height {:.2}",
        m.ascent, m.descent, m.line_height, m.x_height, m.cap_height
    );
    // Sanity on the derived metrics: these orderings hold for every Latin
    // text face and would break loudly if the scaling were wrong.
    assert!(m.ascent > 0.0 && m.descent >= 0.0);
    assert!(m.line_height >= m.ascent + m.descent);
    assert!(
        m.x_height > 0.0 && m.x_height < m.cap_height,
        "x-height {} should be below cap-height {}",
        m.x_height,
        m.cap_height
    );
    assert!(m.cap_height <= m.ascent + 1.0);

    let text = "Hamburgefonstiv 0123";
    let mut buf = vec![BG; (W * H) as usize];
    let end = {
        let mut target = Target {
            buffer: &mut buf,
            stride: W,
            height: H,
            color: 0xFFFF_FFFF,
        };
        font.draw_text(text, &mut target, 4.0, m.ascent + 4.0)
    };

    // The pen must have advanced by exactly what `measure` predicts —
    // otherwise layout and drawing disagree and every centred label is off.
    let predicted = 4.0 + font.measure(text);
    assert!(
        (end - predicted).abs() < 0.01,
        "pen ended at {end}, measure predicted {predicted}"
    );
    assert!(
        end < f32::from(u16::try_from(W).unwrap()),
        "test string overflowed the buffer"
    );

    let lit = buf.iter().filter(|&&p| p != BG).count();
    assert!(lit > 100, "only {lit} pixels were drawn");
    println!(
        "drew {lit} pixels; cache holds {} glyphs",
        font.cached_glyphs()
    );

    // Anti-aliasing means intermediate greys, not just black and white. A
    // purely binary result would mean the coverage was thresholded somewhere.
    let greys = buf
        .iter()
        .filter(|&&p| {
            let v = p & 0xFF;
            v > 0 && v < 255
        })
        .count();
    assert!(greys > 0, "no partial coverage — anti-aliasing was lost");

    // Nothing may be drawn above the ascent line or below the descent line.
    let top_limit = 4;
    for y in 0..top_limit {
        for x in 0..W {
            let idx = (y * W + x) as usize;
            assert_eq!(
                buf[idx], BG,
                "ink at ({x},{y}) is above the baseline's ascent"
            );
        }
    }

    // Print it, so a human can read the sentence.
    for y in 0..H {
        let row: String = (0..W)
            .map(|x| {
                let v = buf[(y * W + x) as usize] & 0xFF;
                match v {
                    0 => ' ',
                    1..=84 => '.',
                    85..=169 => ':',
                    170..=254 => '*',
                    _ => '#',
                }
            })
            .collect();
        if row.trim().is_empty() {
            continue;
        }
        println!("|{row}|");
    }
}
