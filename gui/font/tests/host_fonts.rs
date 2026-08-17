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
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use std::fs;
use std::path::{Path, PathBuf};

use osfont::bidi::Base;
use osfont::gsub::SubGlyph;
use osfont::raster::rasterize;
use osfont::scaled::{ScaledFont, Target};
use osfont::script::ScriptTags;
use osfont::shape::{Affinity, TAB_WIDTH_IN_SPACES};
use osfont::sfnt::{name_id, Face, Outline, PathCmd, Point, SfntError};

/// Every glyph these tests substitute came from Latin text, so Latin is the
/// script its features must be chosen under. Spelled once, because passing the
/// wrong script here would silently disable every ligature in the file and the
/// tests would still pass — they would just stop testing anything.
/// A glyph outline boiled down for comparison: command count, bounding box,
/// and the sums of every x and every y the path touches.
type OutlineRecord = (usize, f32, f32, f32, f32, f32, f32);
/// One `the_outline_oracle_agrees_about_unvaried_glyphs` row: file, glyph, record.
type DefaultOutline = (&'static str, u16, usize, f32, f32, f32, f32, f32, f32);
/// One `installed_variable_fonts_vary_their_outlines` row, with the instance.
type VariedOutline = (&'static str, u16, usize, usize, f32, f32, f32, f32, f32, f32);

const LATIN: Option<ScriptTags> = Some(ScriptTags {
    preferred: *b"latn",
    fallback: *b"latn",
});

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

/// Run `face`'s `GSUB` over `gids` as one run, one source character each.
///
/// The face takes a whole run rather than answering about a position, because
/// its lookups apply in order across all of it; a helper is what keeps that
/// from being spelled out at each of the four call sites below.
fn substitute(face: &Face, script: Option<ScriptTags>, gids: &[u16]) -> Vec<SubGlyph> {
    let mut glyphs: Vec<SubGlyph> = gids
        .iter()
        .enumerate()
        .map(|(i, &gid)| SubGlyph::new(gid, i))
        .collect();
    face.substitute(script, None, &mut glyphs);
    glyphs
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

/// Check that every installed face reports a usable weight/slant/width, and
/// that the well-known ones report the *right* one.
///
/// This is what a family selector matches on, and a wrong answer here is
/// invisible in every other test: a face that misreports itself as regular
/// still parses, still rasterizes, still has ink. It just means asking for
/// bold text gets the regular file.
///
/// The oracle is a family's own four files. `arialbd.ttf` is Arial Bold and
/// nothing else, so it must come back at weight 700; `ariali.ttf` must come
/// back italic and *not* bold. Checking a family as a set also catches the
/// failure that a single-file check cannot — a parser that returned a
/// constant would satisfy "arial.ttf is regular" and fail here.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_report_their_style() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut opened = 0usize;
    let mut by_weight: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
    let mut italics = 0usize;
    let mut condensed = Vec::new();

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else {
            continue;
        };
        opened += 1;
        let style = face.style();
        // The invariants the fallbacks exist to guarantee. A face outside
        // these ranges would be excluded from every match it should win.
        assert!(
            (100..=1000).contains(&style.weight),
            "{}: weight {} is off the scale",
            path.display(),
            style.weight
        );
        assert!(
            (1..=9).contains(&style.width),
            "{}: width class {} is off the scale",
            path.display(),
            style.width
        );
        *by_weight.entry(style.weight).or_default() += 1;
        if style.italic {
            italics += 1;
        }
        if style.width != 5 {
            condensed.push((face.family().unwrap_or_default(), style.width));
        }
    }

    println!("faces opened: {opened}");
    println!("italic:       {italics}");
    println!("weights seen:");
    for (w, n) in &by_weight {
        println!("  {w}: {n}");
    }
    println!("non-normal widths: {}", condensed.len());
    for (family, w) in condensed.iter().take(12) {
        println!("  {family:?} width class {w}");
    }

    // A host with hundreds of fonts has both weights and some italics; if it
    // did not, the checks above would be passing vacuously.
    assert!(
        by_weight.len() > 1,
        "every face on this host reported the same weight — the field is not \
         being read"
    );
    assert!(italics > 0, "not one italic face — the slant flags are not being read");

    // The oracle: a family's own files, which differ *only* in style.
    let oracles: [(&str, u16, bool); 12] = [
        ("arial.ttf", 400, false),
        ("arialbd.ttf", 700, false),
        ("ariali.ttf", 400, true),
        ("arialbi.ttf", 700, true),
        ("times.ttf", 400, false),
        ("timesbd.ttf", 700, false),
        ("timesi.ttf", 400, true),
        ("timesbi.ttf", 700, true),
        ("cour.ttf", 400, false),
        ("courbd.ttf", 700, false),
        ("DejaVuSans.ttf", 400, false),
        ("DejaVuSans-Bold.ttf", 700, false),
    ];
    let mut checked = 0usize;
    for (file, weight, italic) in oracles {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file)))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let style = face.style();
        assert_eq!(
            (style.weight, style.italic),
            (weight, italic),
            "{} reported {style:?}",
            path.display()
        );
        checked += 1;
        println!("oracle ok: {file} -> {style:?}");
    }
    assert!(
        checked >= 2,
        "only {checked} well-known faces installed — the styles were never \
         checked against a known answer"
    );

    // Arial Narrow is the condensed member of the Arial family, and is the
    // reason width class is read at all: without it, a request for "Arial"
    // can be answered with a narrow face that measures quite differently.
    if let Some(path) = files
        .iter()
        .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("arialn.ttf")))
        && let Ok(data) = fs::read(path)
        && let Ok(face) = Face::parse(data)
    {
        assert!(
            face.style().width < 5,
            "Arial Narrow reported width class {}, which is not condensed",
            face.style().width
        );
        println!("oracle ok: arialn.ttf -> {:?}", face.style());
    }
}

/// Read the kerning of every installed face, and check the pairs whose sign
/// every Latin text font agrees on.
///
/// Kerning is the one part of the stack whose correctness cannot be seen in a
/// glyph: a wrong pair still parses, still rasterizes, and still has ink. It
/// shows up only as text spaced slightly wrong, which is exactly the kind of
/// defect that survives every other test in this file.
///
/// The oracle is the sign, not the value. `AV`, `To` and `Yo` are the textbook
/// pairs — two shapes whose nominal advances leave a visible hole — and a font
/// that kerns them at all kerns them *negative*. A parser that read the wrong
/// field, the wrong record, or the wrong endianness would almost certainly
/// produce a positive number or a wild one, so the sign plus a sanity bound
/// catches it without pinning any font's design.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_kern_the_pairs_that_need_it() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut opened = 0usize;
    let mut with_kerning = 0usize;
    let mut nonzero_pairs = 0usize;
    let mut worst = 0_i16;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        opened += 1;
        if !face.has_kerning() {
            continue;
        }
        with_kerning += 1;
        // A kerning value is a correction, so it is a small fraction of an em.
        // Anything near a whole em means a misread field, and would show on
        // screen as a letter sitting on top of its neighbour.
        let limit = i32::from(face.units_per_em()) / 2;
        for (a, b) in [('A', 'V'), ('T', 'o'), ('Y', 'o'), ('P', '.'), ('r', '.')] {
            let (Some(ga), Some(gb)) = (face.glyph_index(a), face.glyph_index(b)) else {
                continue;
            };
            let v = face.kern(ga, gb);
            if v != 0 {
                nonzero_pairs += 1;
                assert!(
                    i32::from(v).abs() <= limit,
                    "{}: kern({a},{b}) = {v}, more than half of the {} unit em",
                    path.display(),
                    face.units_per_em()
                );
                worst = worst.min(v);
            }
        }
    }

    println!("faces opened:       {opened}");
    println!("faces with kerning: {with_kerning}");
    println!("non-zero pairs:     {nonzero_pairs}");
    println!("largest pull-in:    {worst} units");

    assert!(
        with_kerning > 0,
        "not one of {opened} faces reported kerning — the tables are not \
         being found at all"
    );
    assert!(
        nonzero_pairs > 0,
        "{with_kerning} faces have kerning tables but not one kerned any of \
         the pairs that always kern — the lookups are returning nothing"
    );

    // The oracle. These faces all kern AV, and all pull it together.
    let mut checked = 0usize;
    for file in [
        "arial.ttf",
        "times.ttf",
        "segoeui.ttf",
        "DejaVuSans.ttf",
        "verdana.ttf",
    ] {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file)))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let (Some(a), Some(v)) = (face.glyph_index('A'), face.glyph_index('V')) else {
            continue;
        };
        let kern = face.kern(a, v);
        assert!(
            kern < 0,
            "{file}: kern(A,V) = {kern}, but every Latin text face pulls that \
             pair together"
        );
        println!("oracle ok: {file} kern(A,V) = {kern}");
        checked += 1;

        // And the whole point of it: the pair measures narrower than the sum
        // of its parts, at the size a UI actually draws at.
        let font = ScaledFont::from_bytes(fs::read(path).unwrap(), 14.0).expect("scaled");
        let pair = font.measure("AV");
        let apart = font.measure("A") + font.measure("V");
        assert!(
            pair < apart,
            "{file}: \"AV\" measured {pair:.3} px, no narrower than the \
             {apart:.3} px it would be unkerned — measurement is ignoring the \
             kerning the face reports"
        );
        println!("           14px \"AV\" {pair:.3} px vs {apart:.3} px unkerned");
    }
    assert!(
        checked >= 1,
        "none of the well-known faces are installed — kerning was never \
         checked against a known answer"
    );
}

/// A kerning pair with a combining mark between it, over every installed face.
///
/// Real faces flag their `kern` lookups `IgnoreMarks` precisely so that a pair
/// keeps kerning when an accent lands between the two letters — the mark takes
/// no room, so the letters are still adjacent as far as spacing is concerned.
/// An engine that walks the run strictly in order cannot see that, and quietly
/// widens every accented word by one kern.
///
/// `T` + combining acute + `o` is the probe because `T` has no precomposed
/// acute form in Unicode, so normalization cannot compose the mark away and
/// leave nothing between the pair; `A`/`V` would have become `Á` and proved
/// nothing. `To` kerns in essentially every Latin text face.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn a_mark_between_a_kerning_pair_costs_the_kern_only_if_the_flag_says_so() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut kerning_pairs = 0usize;
    let mut read_across = 0usize;
    let mut declined = 0usize;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let (Some(t), Some(o), Some(acute)) = (
            face.glyph_index('T'),
            face.glyph_index('o'),
            face.glyph_index('\u{0301}'),
        ) else {
            continue;
        };
        if face.kern(t, o) == 0 || !face.is_mark(acute) {
            continue;
        }
        kerning_pairs += 1;
        if face.kern_across(t, o, &[acute]) == face.kern(t, o) {
            read_across += 1;
        } else {
            declined += 1;
        }
    }

    println!("faces kerning (T,o):        {kerning_pairs}");
    println!("  kern read across a mark:  {read_across}");
    println!("  lookup declined to:       {declined}");

    assert!(
        kerning_pairs > 0,
        "no installed face kerns (T,o) with a combining acute available — the \
         probe never ran"
    );
    // If this is zero the flag is not being read at all: `IgnoreMarks` is on
    // virtually every real `kern` lookup, so at least the mainstream text
    // faces must come out on the reading-across side.
    assert!(
        read_across > 0,
        "not one of {kerning_pairs} kerning faces would read (T,o) across a \
         combining acute — the lookup flags are being ignored"
    );

    // And the same at the level a caller sees, against a known answer that
    // runs *both* ways. HarfBuzz shapes these five faces and makes the
    // accented string wider than the bare one by exactly:
    //
    //   arial 0, times 0, segoeui 0 — their `kern` lookups are `IgnoreMarks`
    //   DejaVuSans 348, verdana 220 — their `PairPos` lookups carry flag 0,
    //                                 so the mark really does break the pair
    //
    // Both halves matter. A test that only checked the first three would pass
    // just as well on an engine that skipped every mark unconditionally, and
    // that engine gives the wrong answer for the other two: the flag has to be
    // *read*, not assumed.
    let mut checked = 0usize;
    for (file, widens_by) in [
        ("arial.ttf", 0_u16),
        ("times.ttf", 0),
        ("segoeui.ttf", 0),
        ("DejaVuSans.ttf", 348),
        ("verdana.ttf", 220),
    ] {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file)))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let want = 14.0 * f32::from(widens_by) / f32::from(face.units_per_em());
        let font = ScaledFont::from_bytes(fs::read(path).expect("re-read"), 14.0).expect("scaled");
        let bare = font.measure("To");
        let accented = font.measure("T\u{0301}o");
        // A twentieth of a pixel: room for the scaling to round, far less than
        // the smallest of the kerns being told apart.
        assert!(
            (accented - bare - want).abs() < 0.05,
            "{file}: \"To\" measured {bare:.3} px and the accented one \
             {accented:.3} px, a difference of {:.3} px — HarfBuzz makes it \
             {widens_by} units, {want:.3} px",
            accented - bare
        );
        println!(
            "oracle ok: {file} \"To\" {bare:.3} px, accented {accented:.3} px \
             (+{widens_by} units, as HarfBuzz)"
        );
        checked += 1;
    }
    assert!(
        checked >= 2,
        "fewer than two of the well-known faces are installed — the \
         measurement was never checked against a known answer in both \
         directions"
    );
}

/// Read `GSUB` from every installed face, and check the ligature every Latin
/// text font has had since metal type.
///
/// Same reasoning as the kerning sweep: a wrong substitution still parses and
/// still has ink. `fi` is the oracle because it is the one ligature a text
/// face is essentially obliged to carry — the `f`'s hood collides with the
/// `i`'s dot otherwise — and because the answer is checkable without knowing
/// anything about a particular font's design: the substituted glyph must be
/// one glyph, must not be either input glyph, and must exist in the face.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_ligate_fi() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut opened = 0usize;
    let mut with_gsub = 0usize;
    let mut ligated_fi = 0usize;
    let mut contextual_fi = 0usize;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        opened += 1;
        if !face.has_substitutions() {
            continue;
        }
        with_gsub += 1;
        let (Some(f), Some(i), Some(l)) = (
            face.glyph_index('f'),
            face.glyph_index('i'),
            face.glyph_index('l'),
        ) else {
            continue;
        };
        for (name, pair) in [("fi", [f, i]), ("fl", [f, l]), ("ff", [f, f])] {
            let out = substitute(&face, LATIN, &pair);
            if out.len() != 1 {
                // A pair that does not ligate must come back as two glyphs
                // standing where they went in. It need not come back as the
                // *same* two: a face may resolve the collision contextually
                // instead, swapping in a short-hooked `f` before the `i`
                // rather than carrying a joined glyph. Cambria does exactly
                // that, and HarfBuzz shapes its `fi` as two glyphs too.
                assert_eq!(
                    out.len(),
                    pair.len(),
                    "{}: {name} came back as {} glyphs, neither joined into \
                     one nor left as the two it went in as",
                    path.display(),
                    out.len()
                );
                assert_eq!(
                    out.iter().map(|g| g.cluster).collect::<Vec<_>>(),
                    (0..pair.len()).collect::<Vec<_>>(),
                    "{}: {name} came back with its characters renumbered",
                    path.display()
                );
                for g in &out {
                    assert!(
                        g.gid < face.num_glyphs(),
                        "{}: {name} substituted to glyph {}, past the face's \
                         {} glyphs",
                        path.display(),
                        g.gid,
                        face.num_glyphs()
                    );
                }
                if out.iter().map(|g| g.gid).ne(pair.iter().copied()) && name == "fi" {
                    contextual_fi += 1;
                }
                continue;
            }
            let lig = out[0].gid;
            assert!(
                lig != pair[0] && lig != pair[1],
                "{}: {name} substituted to glyph {lig}, which is one of its own \
                 components — the record was misread",
                path.display()
            );
            assert!(
                lig < face.num_glyphs(),
                "{}: {name} substituted to glyph {lig}, past the face's {} glyphs",
                path.display(),
                face.num_glyphs()
            );
            assert_eq!(
                out[0].cluster,
                0,
                "{}: {name} reported cluster {}, not its first component's",
                path.display(),
                out[0].cluster
            );
            if name == "fi" {
                ligated_fi += 1;
            }
        }
        // A ligature must never form from a single glyph, whatever the tables
        // say: there is nothing to join.
        assert_eq!(
            substitute(&face, LATIN, &[f]).len(),
            1,
            "{}: one glyph on its own produced a ligature",
            path.display()
        );
    }

    println!("faces opened:        {opened}");
    println!("faces with GSUB liga:{with_gsub}");
    println!("faces ligating fi:   {ligated_fi}");
    println!("faces with a contextual fi: {contextual_fi}");

    assert!(
        with_gsub > 0,
        "not one of {opened} faces reported ligatures — GSUB is not being \
         found at all"
    );
    assert!(
        ligated_fi > 0,
        "{with_gsub} faces have ligature lookups but not one substitutes fi — \
         the lookups are returning nothing"
    );

    // The oracle: faces whose answer is known independently of this parser.
    // Not all of them ligate `fi` — Windows' own Times New Roman and Segoe UI
    // ship no `liga` covering it, which is why each file may skip and only the
    // count at the end is required. A face that *does* must turn the two
    // glyphs into one, at the first one's cluster, all the way through
    // shaping.
    let mut checked = 0usize;
    for file in ["times.ttf", "segoeui.ttf", "DejaVuSans.ttf", "calibri.ttf"] {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file)))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let (Some(f), Some(i)) = (face.glyph_index('f'), face.glyph_index('i')) else {
            continue;
        };
        let out = substitute(&face, LATIN, &[f, i]);
        if out.len() != 1 {
            println!("oracle skip: {file} has no fi ligature");
            continue;
        }
        println!("oracle ok: {file} fi -> glyph {}", out[0].gid);
        checked += 1;

        // And the whole point of it: shaping the pair yields one glyph, whose
        // width is not the two apart.
        let font = ScaledFont::from_bytes(fs::read(path).unwrap(), 14.0).expect("scaled");
        let run = font.shape("fi");
        assert_eq!(
            run.len(),
            1,
            "{file}: \"fi\" shaped to {} glyphs — substitution is not reaching \
             the layout path",
            run.len()
        );
        let joined = run.width();
        let apart = font.measure("f") + font.measure("i");
        println!("           14px \"fi\" {joined:.3} px vs {apart:.3} px unligated");
        assert!(
            joined > 0.0,
            "{file}: the fi ligature has no advance at all"
        );

        // The cluster is the pair's first byte, so a caret can sit before or
        // after the ligature and nowhere inside it.
        let clusters: Vec<usize> = run.glyphs().iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, vec![0], "{file}: fi reported clusters {clusters:?}");
    }
    assert!(
        checked >= 1,
        "none of the well-known faces are installed — ligatures were never \
         checked against a known answer"
    );
}

/// Faces whose Latin substitutions sit *past* the first few dozen subtables,
/// so that a budget too small to reach them is caught.
///
/// This is the regression test for a bug that green tests missed entirely.
/// `otl::MAX_SUBTABLES` was 64, shared across every lookup a face's features
/// reach, and 61 of this host's 365 `GSUB` faces declare more than that. The
/// budget ran out partway down the lookup list and the rest of the face was
/// silently dropped — so Amiri, whose enormous Arabic feature set is listed
/// before its Latin `liga`, produced no ligature at all, and FiraCode never
/// reached the `calt` that shortens `f` before `i`. Nothing failed: the
/// shaper returned the input unchanged, which is a legal answer for a face
/// with no ligatures, and every assertion we had was happy with it. It took
/// shaping all 556 installed faces against HarfBuzz to see it.
///
/// The lesson is in the choice of faces below: each is named because its
/// answer is known to live deep, not because it is popular. A face that is
/// not installed is skipped, but at least one must be found.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_reach_lookups_past_the_subtable_budget() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    // (file, text, what a shaper that reaches the whole face produces).
    // The lengths are HarfBuzz's, taken with kerning and mark attachment off
    // so that only substitution is compared.
    let deep: &[(&str, &str, usize)] = &[
        // Latin `liga` behind the Arabic lookups: ff, fi, fl all join.
        ("Amiri-Regular.ttf", "office", 4),
        ("Amiri-Regular.ttf", "fi", 1),
        ("Amiri-Bold.ttf", "waffle", 4),
        // A programming face: `fi` stays two glyphs, but `calt` swaps the
        // second, so the pair comes back changed rather than untouched.
        // JetBrains Mono declares even more subtables than FiraCode but is
        // *not* listed, because HarfBuzz shapes its `fi` unchanged too — a
        // deep face only makes a witness when something deep applies to it.
        ("FiraCode-Regular.ttf", "fi", 2),
        ("FiraCode-Bold.ttf", "fi", 2),
    ];

    let mut checked = 0usize;
    for (file, text, want) in deep {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file)))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let gids: Vec<u16> = text.chars().filter_map(|c| face.glyph_index(c)).collect();
        if gids.len() != text.chars().count() {
            continue;
        }
        let out = substitute(&face, LATIN, &gids);
        assert_eq!(
            out.len(),
            *want,
            "{file}: {text:?} shaped to {} glyphs, not {want} — the lookups \
             that do it are past the subtable budget and are not being reached",
            out.len()
        );
        if out.len() == gids.len() {
            // Same length, so the evidence has to be that a glyph changed.
            assert!(
                out.iter().map(|g| g.gid).ne(gids.iter().copied()),
                "{file}: {text:?} came back untouched — the contextual lookup \
                 that rewrites it was never reached"
            );
        }
        println!(
            "deep ok: {file} {text:?} -> {:?}",
            out.iter().map(|g| g.gid).collect::<Vec<_>>()
        );
        checked += 1;
    }
    assert!(
        checked >= 1,
        "none of the deep-lookup faces are installed — the subtable budget was \
         never exercised"
    );
}

/// Run every installed face's `GSUB` over ordinary Latin prose and check that
/// it leaves it alone.
///
/// This is the safety net under the substitution pass rather than a test of a
/// feature. `ccmp` and the single-substitution lookups are applied to *all*
/// text, unconditionally, which is what every shaper does — but it also means
/// a misread coverage table or a delta applied to the wrong glyph would
/// silently replace the letters of every label on the desktop with whatever
/// happened to sit at the wrong offset. The failure would be legible text of
/// the wrong letters, which is exactly the kind of thing a rasterization test
/// sails past.
///
/// The oracle is that a Latin face has nothing to do here: `ccmp` exists to
/// normalise sequences involving marks and dotted letters, and there are no
/// marks in this string. So every glyph must come out as `cmap` put it in.
/// A face that legitimately substitutes is not a failure of this test — it is
/// reported, and the assertion is on the *proportion*, because a parser fault
/// would hit fonts in bulk while a genuine `ccmp` rule hits a handful.
///
/// On the development host eight of 281 faces change it, and both causes are
/// understood:
///
/// * The six Linux Libertine files have a `Th` ligature, which "The" triggers.
///   The run comes back one glyph shorter, which is the font working.
/// * `ebrima.ttf` and `ebrimabd.ttf` substitute the *space*, from a `ccmp`
///   lookup belonging to one of the African scripts they cover. That one is a
///   genuine wrong answer, and it is the known no-script-selection limitation
///   biting — see `TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES` in
///   `known-issues.md`. It is left in the count deliberately: when script
///   selection lands, this number should drop to six.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_leave_plain_latin_alone() {
    // No `fi`, `fl` or `ff`: a ligature here would be correct, and would
    // muddy an oracle that is about single substitution.
    const PROSE: &str = "The quick brown vex jumps lazy dogs 0123456789";

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut with_gsub = 0usize;
    let mut changed = Vec::new();
    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        if !face.has_substitutions() {
            continue;
        }
        with_gsub += 1;
        let before: Vec<u16> = PROSE.chars().filter_map(|c| face.glyph_index(c)).collect();
        if before.len() != PROSE.chars().count() {
            // The face cannot spell the string; it has nothing to say here.
            continue;
        }
        let after = substitute(&face, LATIN, &before);
        let gids: Vec<u16> = after.iter().map(|g| g.gid).collect();
        if gids != before {
            // Report *what* moved, not just that something did: a ligature
            // shortening the run is a font doing its job, while a same-length
            // run of different ids is the shape a misread table would take.
            let at = after
                .iter()
                .position(|g| Some(&g.gid) != before.get(g.cluster))
                .unwrap_or(0);
            let cluster = after.get(at).map_or(0, |g| g.cluster);
            changed.push((
                path.clone(),
                format!(
                    "{} glyphs from {}, first change at char {cluster} ({:?})",
                    after.len(),
                    before.len(),
                    PROSE.chars().nth(cluster).unwrap_or('?')
                ),
            ));
            continue;
        }
        // Unchanged glyphs must still be one per character, each carrying its
        // own cluster: a pass that dropped or duplicated an entry while
        // substituting nothing would leave the ids equal and the run wrong.
        let clusters: Vec<usize> = after.iter().map(|g| g.cluster).collect();
        assert_eq!(
            clusters,
            (0..before.len()).collect::<Vec<_>>(),
            "{}: an unchanged run came back with clusters {clusters:?}",
            path.display()
        );
    }

    println!("faces with GSUB:     {with_gsub}");
    println!("faces changing prose:{}", changed.len());
    for (path, what) in changed.iter().take(20) {
        println!("  {}: {what}", path.display());
    }

    assert!(
        with_gsub > 0,
        "not one face reported substitutions — GSUB is not being found at all"
    );
    // A tenth is far above what genuine `ccmp` rules touch on Latin prose and
    // far below what a misread table would: the point is to catch a fault that
    // hits fonts in bulk, not to forbid a font from having a rule.
    assert!(
        changed.len() * 10 < with_gsub,
        "{} of {with_gsub} faces changed plain Latin prose — that is not \
         `ccmp` doing its job, that is the substitution pass misreading tables",
        changed.len()
    );
}

/// Shape text every installed face has an opinion about, and check that the
/// run and the source string still agree about where the boundaries are.
///
/// Substitution makes the glyph-to-character mapping many-to-many in both
/// directions: a ligature is one glyph for several characters, and a
/// `MultipleSubst` decomposition is several glyphs for one. Every caret, cut
/// and hit test in the desktop is a byte offset into the *string*, so the only
/// offsets any of them may name are the ones a cluster starts at. An answer
/// that falls anywhere else is a caret drawn inside a ligature, or a slice
/// taken through the middle of a character — a panic in the best case and
/// mojibake in the worst.
///
/// The unit tests pin this on runs built by hand. This pins it on whatever 281
/// real fonts actually do with text they have rules for, which is the part no
/// fixture can predict.
///
/// On the development host `cambria.ttc` is the face that exercises the
/// growing direction: `"été"` comes out as five glyphs with clusters
/// `[0, 0, 2, 3, 3]`, because its `ccmp` decomposes each precomposed `é` into
/// an `e` and an acute so that GPOS can then attach the accent. That is the
/// textbook reason `MultipleSubst` exists, and it is why the test insists at
/// the end that *something* grew: with type 2 silently not applied, every
/// assertion here would still pass while only ever seeing the ligature case.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn shaped_runs_agree_with_their_strings_about_boundaries() {
    // Strings chosen so that some face has a rule for each: Latin ligatures,
    // a combining sequence, its precomposed spelling, Devanagari (whose
    // `ccmp` decomposes), and Arabic (whose `rlig` is not optional).
    const STRINGS: &[&str] = &[
        "office",
        "e\u{0301}",
        "\u{00E9}t\u{00E9}",
        "\u{0928}\u{094D}\u{0937}",
        "\u{0644}\u{0627}",
        "waffle iron",
    ];

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut checked = 0usize;
    let mut grew = 0usize;
    let mut shrank = 0usize;
    let mut grew_examples: Vec<String> = Vec::new();

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        if !face.has_substitutions() {
            continue;
        }
        let Ok(font) = ScaledFont::from_bytes(fs::read(path).unwrap(), 16.0) else {
            continue;
        };
        checked += 1;

        for text in STRINGS {
            let run = font.shape(text);
            let end = text.len();
            let clusters: Vec<usize> = run.glyphs().iter().map(|g| g.cluster).collect();
            if run.len() > text.chars().count() {
                grew += 1;
                if grew_examples.len() < 5 {
                    grew_examples.push(format!(
                        "{}: {text:?} -> {} glyphs, clusters {clusters:?}",
                        path.display(),
                        run.len()
                    ));
                }
            } else if run.len() < text.chars().count() {
                shrank += 1;
            }

            // Non-decreasing, and every one a real character boundary. A
            // cluster that went backwards, or landed mid-character, would make
            // every query below meaningless rather than merely wrong.
            for (i, &c) in clusters.iter().enumerate() {
                assert!(
                    text.is_char_boundary(c),
                    "{}: {text:?} glyph {i} has cluster {c}, not a character \
                     boundary",
                    path.display()
                );
                if i > 0 {
                    assert!(
                        c >= clusters[i - 1],
                        "{}: {text:?} clusters go backwards: {clusters:?}",
                        path.display()
                    );
                }
            }

            // The advances must sum to the width, or measuring and drawing
            // disagree about where the string ends.
            let summed: f32 = run.glyphs().iter().map(|g| g.advance).sum();
            assert!(
                (summed - run.width()).abs() <= 0.01,
                "{}: {text:?} sums to {summed} but reports {}",
                path.display(),
                run.width()
            );

            // Every answer a query gives must be a cluster start or the end of
            // the string — the two kinds of position that exist.
            let allowed = |at: usize| at == end || clusters.contains(&at);
            let width = run.width().ceil().max(1.0);
            let mut px = 0.0f32;
            while px <= width {
                for (what, at) in [
                    ("fit", run.fit(px, end)),
                    ("fit_end", run.fit_end(px, end)),
                    ("offset_at", run.offset_at(px, end).offset),
                ] {
                    assert!(
                        allowed(at),
                        "{}: {text:?} {what}({px}) = {at}, which is not a \
                         cluster boundary; clusters are {clusters:?}",
                        path.display()
                    );
                    assert!(
                        text.is_char_boundary(at),
                        "{}: {text:?} {what}({px}) = {at}, mid-character",
                        path.display()
                    );
                }
                px += 1.0;
            }

            // The *measurement* must never go backwards as it advances through
            // the string: it is a prefix width, and a prefix only grows.
            //
            // This is deliberately asserted of `width_upto` and not of `x_of`.
            // A caret is a position on the screen, and in bidirectional text
            // walking the string forwards walks the screen backwards for the
            // length of every right-to-left run — so monotonicity is exactly
            // the property a correct caret must *not* have. Asserting it here
            // used to pass only because `x_of` was a prefix width wearing a
            // caret's name.
            let mut last = 0.0f32;
            for at in 0..=end {
                if !text.is_char_boundary(at) {
                    continue;
                }
                let w = run.width_upto(at, end);
                assert!(
                    w >= last - 0.01,
                    "{}: {text:?} width_upto({at}) = {w} after {last}",
                    path.display()
                );
                last = w;
            }

            // The caret itself has to land somewhere on the run, from either
            // side of every boundary. Both affinities, because at a direction
            // change they are two different points and both are drawn.
            for at in 0..=end {
                if !text.is_char_boundary(at) {
                    continue;
                }
                for affinity in [Affinity::Downstream, Affinity::Upstream] {
                    let x = run.x_of(at, end, affinity);
                    assert!(
                        (-0.01..=run.width() + 0.01).contains(&x),
                        "{}: {text:?} x_of({at}, {affinity:?}) = {x}, outside \
                         the run's {} px",
                        path.display(),
                        run.width()
                    );
                }
            }
        }
    }

    println!("faces checked:     {checked}");
    println!("runs that grew:    {grew}");
    for example in &grew_examples {
        println!("  {example}");
    }
    println!("runs that shrank:  {shrank}");
    assert!(
        checked > 0,
        "not one face reported substitutions — GSUB is not being found at all"
    );
    // If nothing anywhere grows, `MultipleSubst` is not reaching the layout
    // path and everything above is testing the ligature case twice.
    assert!(
        grew > 0,
        "not one of {checked} faces decomposed any of these strings — either \
         no installed face has a `ccmp` decomposition, or type 2 is not being \
         applied"
    );
}

/// A symbol-encoded face still maps plain ASCII.
///
/// Fonts like Wingdings and MT Extra ship only a platform-3 encoding-0 `cmap`,
/// which keys on the byte the character had in the font's own 8-bit encoding
/// lifted into the private-use area — `A` lives at U+F041, not U+0041. Without
/// the retry in that range every one of these faces drew every string as a row
/// of empty boxes, and nothing caught it: a face is *allowed* to have no `A`,
/// so "no glyph" is a legal answer and no self-consistency check can tell it
/// from a real one. Shaping the installed faces against HarfBuzz is what found
/// it, and this is the standing witness.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn symbol_encoded_fonts_still_map_ascii() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    // Faces known to carry a symbol `cmap` and nothing else. Named rather than
    // detected, so that a regression fails the test instead of quietly leaving
    // it with no faces to check.
    let symbol_only = ["WINGDNG2.TTF", "WINGDNG3.TTF", "MTEXTRA.TTF", "BSSYM7.TTF"];
    let mut checked = 0usize;

    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !symbol_only.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            continue;
        }
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let mut mapped = 0usize;
        for cp in 0x20u32..0x7F {
            let (Some(ch), Some(shifted)) = (char::from_u32(cp), char::from_u32(0xF000 + cp))
            else {
                continue;
            };
            // The retry is a *re-spelling*, not a guess: the two spellings of
            // one character must agree exactly, including where neither is
            // present. MT Extra is a maths font and genuinely lacks most
            // letters, so "some ASCII maps" is the strongest claim that holds
            // across all of these faces.
            assert_eq!(
                face.glyph_index(ch),
                face.glyph_index(shifted),
                "{name}: U+{cp:04X} and its U+F0xx spelling disagree"
            );
            if face.glyph_index(ch).is_some() {
                mapped += 1;
            }
        }
        assert!(
            mapped > 0,
            "{name}: no printable ASCII maps at all, so the symbol cmap was not consulted"
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "none of {symbol_only:?} are installed on this host"
    );
    println!("checked {checked} symbol-encoded faces");
}

/// Shape text containing a tab in every installed face, and check that the tab
/// survives substitution intact.
///
/// A tab is not a glyph the font knows about. It is carried through shaping as
/// the *space* glyph because that draws blank and gives a unit to multiply —
/// but it is a layout decision wearing a glyph's clothes, and a `GSUB` lookup
/// handed the whole run has no way to tell the difference. Two things can go
/// wrong, and both are silent:
///
/// * A single substitution covering the space replaces it, and the tab draws
///   as whatever that lookup meant for a real space. `ebrima.ttf` has exactly
///   such a rule (see `TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES`), so this is
///   not hypothetical on the development host.
/// * A ligature joins the space to a neighbour, the run comes out shorter, and
///   the tab flags stop lining up with the glyphs — after which every advance
///   past that point is charged to the wrong glyph.
///
/// Neither can happen while each stretch between tabs is substituted
/// separately, which is what this pins down: the lookups never see across the
/// boundary, so the tab arrives at layout as the glyph `cmap` gave.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_leave_a_tab_alone() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut with_gsub = 0usize;
    // Faces whose `GSUB` would substitute a lone space if it were shown one.
    // These are the ones that make this test more than a formality; the count
    // is printed so that a host with none of them does not look like a pass.
    let mut would_touch_space = 0usize;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        if !face.has_substitutions() {
            continue;
        }
        let (Some(_), Some(space)) = (face.glyph_index('x'), face.glyph_index(' ')) else {
            continue;
        };
        with_gsub += 1;
        let touched = substitute(&face, LATIN, &[space])
            .first()
            .is_some_and(|g| g.gid != space);
        if touched {
            would_touch_space += 1;
        }

        let Ok(mut font) = ScaledFont::from_bytes(fs::read(path).unwrap(), 14.0) else {
            continue;
        };
        let run = font.shape("x\tx");
        assert_eq!(
            run.len(),
            3,
            "{}: \"x\\tx\" shaped to {} glyphs — a substitution reached across \
             the tab",
            path.display(),
            run.len()
        );
        let clusters: Vec<usize> = run.glyphs().iter().map(|g| g.cluster).collect();
        assert_eq!(
            clusters,
            vec![0, 1, 2],
            "{}: \"x\\tx\" reported clusters {clusters:?}",
            path.display()
        );
        // The tab's width is the layout constant times the *unsubstituted*
        // space, read straight off the face — deliberately not `shape(" ")`,
        // which is a real space and so is the face's to substitute. If the tab
        // flags had slipped out of step with the glyphs, or the tab had been
        // replaced by a glyph of another width, this is where it shows.
        let scale = 14.0 / f32::from(face.units_per_em());
        let expected = f32::from(face.advance(space).unwrap_or(0)) * scale * TAB_WIDTH_IN_SPACES;
        let (tab_key, tab_advance) = (run.glyphs()[1].key, run.glyphs()[1].advance);
        assert!(
            (tab_advance - expected).abs() <= 0.01,
            "{}: the tab advanced {tab_advance} px, not the {expected} px of \
             four spaces{}",
            path.display(),
            if touched { " — the face's own space rule reached it" } else { "" }
        );
        // And it must draw nothing. The advance alone would not catch a
        // substitution to a glyph that happens to be the same width, and a tab
        // that draws ink is the most visible failure of the lot.
        let inked = font
            .glyph_mask(tab_key)
            .is_some_and(|m| m.coverage.iter().any(|&c| c != 0));
        assert!(
            !inked,
            "{}: the glyph standing in for the tab draws ink — it is not the \
             space it went in as{}",
            path.display(),
            if touched { " (this face substitutes ' ')" } else { "" }
        );
        let letter = font.shape("x");
        assert_eq!(
            letter.len(),
            1,
            "{}: \"x\" alone shaped to {} glyphs",
            path.display(),
            letter.len()
        );
        assert!(
            (run.glyphs()[0].advance - letter.glyphs()[0].advance).abs() <= 0.01,
            "{}: the letter before the tab shaped differently from the same \
             letter alone",
            path.display()
        );
    }

    println!("faces with GSUB:        {with_gsub}");
    println!("faces substituting ' ': {would_touch_space}");
    assert!(
        with_gsub > 0,
        "not one face reported substitutions — GSUB is not being found at all"
    );
}

/// Read `GPOS` mark attachment from every installed face, and check that a
/// combining acute ends up over the letter rather than beside it.
///
/// This is the one part of shaping whose failure is unmissable rather than
/// subtle — an unpositioned mark lands at the pen, which is the *left edge*
/// of the base glyph's cell, so `e` + U+0301 draws the accent in the gap
/// before the `e`. The oracle is therefore loose on purpose: the exact
/// placement is the designer's business, but the accent must move right (past
/// the pen, into the letter) and up (above the baseline).
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_place_combining_marks() {
    const ACUTE: char = '\u{0301}';
    // `f`, not `e`, and the difference matters now that the sweep goes through
    // the shaper rather than asking the face about a pair of glyph ids:
    // `e` + U+0301 is canonically U+00E9, so a face that has an `é` would get
    // one glyph back and have no mark left to place. `f` + acute has no
    // precomposed form in Unicode at all, so it survives normalization as two
    // characters on every face and still exercises attachment.
    const BASE: char = 'f';

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut opened = 0usize;
    let mut with_marks = 0usize;
    let mut placed = 0usize;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        opened += 1;
        if !face.has_marks() {
            continue;
        }
        with_marks += 1;
        let (Some(base_gid), Some(acute)) = (face.glyph_index(BASE), face.glyph_index(ACUTE))
        else {
            continue;
        };
        let em = i32::from(face.units_per_em());
        // One pixel per font unit, so that the shaper's pixel offsets come out
        // in the face's own units and can be checked against its em. Nothing
        // is rasterized here, so the nominally enormous size costs nothing.
        let Ok(font) = ScaledFont::new(face, em as f32) else {
            continue;
        };
        let run = font.shape(&format!("{BASE}{ACUTE}"));
        if run.len() != 2 {
            continue;
        }
        let mark = run.glyphs()[1];
        // A face can carry mark lookups that cover an entirely different
        // script: DejaVu Sans Mono's single `MarkBasePos` covers 15 Lao
        // glyphs and has never heard of `acutecomb`. That face has nothing
        // to say about a Latin accent, so it is skipped rather than failed —
        // the sweep's job is to check the faces that *do* answer.
        //
        // A face whose anchors legitimately coincide is skipped with them: a
        // monospace design draws its combining acute already at accent height
        // inside its cell, so its displacement really is (0, 0) — Cascadia
        // Code is exactly that — and is indistinguishable here from a face
        // that said nothing. What the mark ends up over is checked below, on
        // the run, where the glyph's own extents are available.
        let (dx, dy) = mark.offset;
        if dx == 0.0 && dy == 0.0 {
            continue;
        }
        placed += 1;
        assert!(
            font.face().is_mark(acute),
            "{}: anchors an acute onto an '{BASE}' yet does not class U+0301 \
             as a mark — the mark would be kerned and advanced like a letter",
            path.display()
        );
        assert!(
            !font.face().is_mark(base_gid),
            "{}: '{BASE}' is classed as a combining mark",
            path.display()
        );
        assert_eq!(
            mark.advance, 0.0,
            "{}: the acute advances the pen",
            path.display()
        );
        // A displacement is a placement within the glyph, so it is bounded by
        // the em; anything larger means a misread anchor and would put the
        // accent on a different letter.
        assert!(
            dx.abs() <= em as f32 && dy.abs() <= (em * 2) as f32,
            "{}: acute displaced by ({dx}, {dy}) on a {em} unit em",
            path.display()
        );
    }

    println!("faces opened:              {opened}");
    println!("faces that know marks:     {with_marks}");
    println!("faces placing the acute:   {placed}");

    assert!(
        with_marks > 0,
        "not one of {opened} faces can tell a mark from a letter — GDEF \
         classes and GPOS mark lookups are not being found at all"
    );
    assert!(
        placed > 0,
        "{with_marks} faces know about marks but not one displaces a \
         combining acute on an '{BASE}' — the anchors are returning nothing"
    );

    // The same oracle again on the well-known faces, but now with the ink: the
    // accent must land *over* the letter, not merely somewhere within an em of
    // it. This is the half that catches a placement that is the right size and
    // the wrong sign.
    let mut checked = 0usize;
    for file in ["segoeui.ttf", "DejaVuSans.ttf", "calibri.ttf", "times.ttf"] {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file)))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let mut font = ScaledFont::from_bytes(data, 32.0).expect("scaled");
        let run = font.shape(&format!("{BASE}{ACUTE}"));
        if run.len() != 2 {
            println!(
                "oracle skip: {file} shaped {BASE}+acute to {} glyphs",
                run.len()
            );
            continue;
        }
        let base = run.glyphs()[0];
        let mark = run.glyphs()[1];
        // Both, not 0 and 1: a mark is charged to the character it attaches
        // to, so the pair is one cluster and a caret cannot land between the
        // letter and its accent.
        assert_eq!(
            mark.cluster, 0,
            "{file}: the acute reported its own byte offset instead of its \
             base's"
        );

        // A combining mark takes no room: the pair measures as the bare base.
        assert!(
            (run.width() - base.advance).abs() < 0.5,
            "{file}: {BASE}+acute measures {:.3} px against the bare \
             letter's {:.3} px",
            run.width(),
            base.advance
        );

        // Where the ink actually lands. The bug this exists to catch is an
        // accent drawn at the pen — i.e. in the gap *before* the letter —
        // so the test is that the accent's ink overlaps the letter's cell
        // horizontally and sits above the baseline. Both are true of every
        // design; neither is true of an unpositioned mark in a proportional
        // face.
        let (left, width, top) = {
            let mask = font.glyph_mask(mark.key).expect("the acute must draw");
            (mask.left, mask.width, mask.top)
        };
        // The same arithmetic the draw loop does: the pen has already walked
        // past the `e` by the time the mark is drawn, and `offset` displaces
        // the ink from *there*. Leaving the pen out is exactly the mistake
        // that makes an accent look correct in a debugger and land a whole
        // advance to the left on the screen.
        let pen = base.advance;
        let ink_left = pen + mark.offset.0 + left as f32;
        let ink_right = ink_left + width as f32;
        // `top` is downward-positive from the baseline; `offset.1` is upward.
        let ink_top = top as f32 - mark.offset.1;
        println!(
            "oracle ok: {file} 32px acute offset ({:.2}, {:.2}); ink x \
             {ink_left:.2}..{ink_right:.2} within the letter's 0..{:.2}, top \
             {ink_top:.2}",
            mark.offset.0, mark.offset.1, base.advance
        );
        assert!(
            ink_right > 0.0 && ink_left < base.advance,
            "{file}: the acute's ink spans {ink_left:.2}..{ink_right:.2}, \
             which does not overlap the letter's 0..{:.2} — it is being drawn \
             beside the letter rather than on it",
            base.advance
        );
        assert!(
            ink_top < 0.0,
            "{file}: the acute's ink starts {ink_top:.2} px below the \
             baseline"
        );
        checked += 1;
    }
    assert!(
        checked >= 1,
        "none of the well-known faces placed a combining acute — mark \
         attachment was never checked against a known answer"
    );
}

/// The faces that never wrote down where an accent goes must still put it on
/// the letter.
///
/// The test above only ever asks faces that *have* `GPOS` mark anchors. A
/// large share of the host's installed faces have no `GPOS` table at all, and
/// before the fallback existed every one of them drew a combining accent at
/// the pen — in the gap *after* the letter, overprinting whatever came next.
/// This is the check that the measured placement replaces that.
///
/// The base is `f` for the same reason as above: `e` + U+0301 composes to
/// `é` and leaves no mark to place, while `f` + acute has no precomposed form
/// on any face.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_without_gpos_still_place_combining_marks() {
    const BASE: char = 'f';
    const ACUTE: char = '\u{0301}';

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut without_gpos = 0usize;
    let mut checked = 0usize;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data.clone()) else {
            continue;
        };
        if face.has_positioning() {
            continue;
        }
        without_gpos += 1;
        if face.glyph_index(BASE).is_none() || face.glyph_index(ACUTE).is_none() {
            continue;
        }
        let Ok(mut font) = ScaledFont::from_bytes(data, 32.0) else {
            continue;
        };
        let run = font.shape(&format!("{BASE}{ACUTE}"));
        if run.len() != 2 {
            continue;
        }
        let base = run.glyphs()[0];
        let mark = run.glyphs()[1];
        // A combining mark takes no room, so the pair measures as the bare
        // letter. This is the half of the fallback that matters even when the
        // accent itself draws nothing.
        assert!(
            (run.width() - base.advance).abs() < 0.5,
            "{}: {BASE}+acute measures {:.3} px against the bare letter's \
             {:.3} px — the mark's advance was not zeroed",
            path.display(),
            run.width(),
            base.advance
        );
        // A face whose combining acute is blank has nothing to place.
        let Some(mask) = font.glyph_mask(mark.key) else {
            continue;
        };
        if mask.width == 0 {
            continue;
        }
        checked += 1;
        // The same arithmetic the draw loop does: the pen has walked past the
        // letter by the time the mark is drawn, and `offset` displaces the ink
        // from there.
        let ink_left = base.advance + mark.offset.0 + mask.left as f32;
        let ink_right = ink_left + mask.width as f32;
        assert!(
            ink_right > 0.0 && ink_left < base.advance,
            "{}: the acute's ink spans {ink_left:.2}..{ink_right:.2}, which \
             does not overlap the letter's 0..{:.2} — a face with no GPOS is \
             drawing the accent beside the letter again",
            path.display(),
            base.advance
        );
        // `top` is downward-positive from the baseline; `offset.1` is upward.
        assert!(
            mask.top as f32 - mark.offset.1 < 0.0,
            "{}: the acute's ink starts below the baseline",
            path.display()
        );
    }

    println!("faces with no GPOS at all: {without_gpos}");
    println!("of those, accent placed:   {checked}");
    assert!(
        without_gpos > 0,
        "every one of {} installed faces claims a GPOS table, which cannot be \
         right — the table probe is broken",
        files.len()
    );
    assert!(
        checked > 0,
        "{without_gpos} faces have no GPOS and not one of them drew a \
         combining acute — the fallback never ran"
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

/// Shape Arabic with every installed face and check that the letters join.
///
/// Arabic drawn as a row of isolated letters is not "slightly off" — to
/// someone who reads it, it is close to unreadable, in the way English set as
/// I S O L A T E D C A P S with no word shapes would be. Yet it parses, it has
/// ink, and every other test in this file passes on it. Only an assertion
/// about the *forms* catches it, which is what this is.
///
/// The oracle needs no knowledge of any particular face's design:
///
/// * `ببب` — three behs, a dual-joining letter — must come out as three
///   glyphs, and the middle one is medial, so it must differ from the glyph
///   the same letter gets standing alone;
/// * a space breaks a word, so `بب بب` must shape the two halves identically;
/// * `اب` starts with alef, which joins only backwards, so the beh after it
///   cannot be medial — it must shape as the beh of a fresh word does.
///
/// A face with no Arabic joining rules at all fails the first check and is
/// skipped, not failed: plenty of Latin-only faces map Arabic through a
/// fallback `cmap` without carrying a single positional lookup.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_join_arabic_letters() {
    // beh, a dual-joining letter present in every face with any Arabic at all.
    const BEH: &str = "\u{628}";
    // alef, which joins only to what precedes it.
    const ALEF: &str = "\u{627}";

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut with_arabic = 0usize;
    let mut joining = 0usize;
    let mut examples = Vec::new();

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        if !face.has_substitutions() || face.glyph_index('\u{628}').is_none() {
            continue;
        }
        let Ok(font) = ScaledFont::from_bytes(fs::read(path).unwrap(), 16.0) else {
            continue;
        };
        with_arabic += 1;

        let alone = font.shape(BEH);
        let word = font.shape(&BEH.repeat(3));
        if alone.len() != 1 || word.len() != 3 {
            // A face that ligates the run has said something this oracle
            // cannot read; it is not evidence either way.
            continue;
        }
        let isolated = alone.glyphs()[0].key.raw();
        let gids: Vec<u32> = word.glyphs().iter().map(|g| g.key.raw()).collect();
        if gids.iter().all(|&g| g == isolated) {
            // No positional lookups: a Latin face that happens to have the
            // letter, which is common and is not a bug.
            continue;
        }
        joining += 1;
        if examples.len() < 5 {
            examples.push(format!(
                "{}: isolated {isolated}, word {gids:?}",
                path.file_name().unwrap().to_string_lossy()
            ));
        }

        assert_ne!(
            gids[1], isolated,
            "{}: the middle beh of a three-letter word kept its isolated \
             form — `medi` was not applied",
            path.display()
        );

        // A space ends a word, so the two halves must shape alike.
        let two = font.shape(&format!("{BEH}{BEH} {BEH}{BEH}"));
        if two.len() == 5 {
            let g: Vec<u32> = two.glyphs().iter().map(|x| x.key.raw()).collect();
            assert_eq!(
                (g[0], g[1]),
                (g[3], g[4]),
                "{}: a space did not break the word — {g:?}",
                path.display()
            );
        }

        // Alef joins only backwards, so what follows it starts a fresh join:
        // the two behs after an alef must shape as those same two behs do
        // alone. Compared against a *two*-letter word rather than against
        // `gids`, because a face with contextual alternates may legitimately
        // give the initial beh of a two-letter word a different glyph from
        // the initial beh of a three-letter one — that is the face doing its
        // job, not a joining error.
        let pair = font.shape(&BEH.repeat(2));
        let after_alef = font.shape(&format!("{ALEF}{BEH}{BEH}"));
        if pair.len() == 2 && after_alef.len() == 3 {
            let p: Vec<u32> = pair.glyphs().iter().map(|x| x.key.raw()).collect();
            let g: Vec<u32> = after_alef.glyphs().iter().map(|x| x.key.raw()).collect();
            assert_eq!(
                (g[1], g[2]),
                (p[0], p[1]),
                "{}: the beh after an alef did not start a new join — {g:?} \
                 against {p:?}",
                path.display()
            );
        }
    }

    println!("faces with Arabic and a GSUB: {with_arabic}");
    println!("faces that join:              {joining}");
    for line in &examples {
        println!("  {line}");
    }
    assert!(
        joining >= 1,
        "no installed face joined Arabic at all ({with_arabic} had the \
         letters and a GSUB) — the positional features are not being applied"
    );
}

/// Right-to-left text comes back in an order that can be drawn.
///
/// The unit tests prove the bidi algorithm against Unicode's own conformance
/// data, and prove `draw_order` against a permutation handed to it. Neither
/// proves the join: that `shape` resolves the levels of the string it was
/// given, applies them to the glyphs it produced, and hands the run a
/// permutation that matches. Only a real face can answer that, because only a
/// real face turns characters into glyphs.
///
/// The oracle needs nothing face-specific:
///
/// * three Hebrew letters must be drawn in exactly the reverse of the order
///   they were typed in;
/// * with a Latin letter on each side, only the Hebrew reverses — the run
///   splits into three, and the outer two keep their order;
/// * a plain Latin string is not reordered at all, so `draw_order` and
///   `glyphs` are the same sequence and `is_reordered` is false;
/// * the width does not depend on the order. That is the check that catches a
///   kern re-charged to the wrong glyph: `recharge_kerns` strips every kern
///   and gives them back, and if it gave back a different set the run would
///   change width when it was reversed.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_fonts_reorder_right_to_left_text() {
    // Alef, bet, gimel: three plain Hebrew letters, in a language with no
    // joining behaviour, so a face that has them shapes them one for one.
    const HEBREW: &str = "\u{5d0}\u{5d1}\u{5d2}";

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut with_hebrew = 0usize;
    let mut checked = 0usize;

    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        if !"\u{5d0}\u{5d1}\u{5d2}AB"
            .chars()
            .all(|ch| face.glyph_index(ch).is_some())
        {
            continue;
        }
        let Ok(font) = ScaledFont::from_bytes(fs::read(path).unwrap(), 16.0) else {
            continue;
        };
        with_hebrew += 1;

        let run = font.shape(HEBREW);
        if run.len() != 3 {
            // A face that ligated or decomposed has said something this
            // oracle cannot read.
            continue;
        }
        checked += 1;

        assert!(
            run.is_reordered(),
            "{}: a Hebrew word came back in logical order",
            path.display()
        );
        let logical: Vec<u32> = run.glyphs().iter().map(|g| g.key.raw()).collect();
        let drawn: Vec<u32> = run.draw_order().map(|g| g.key.raw()).collect();
        let mut backwards = logical.clone();
        backwards.reverse();
        assert_eq!(
            drawn,
            backwards,
            "{}: Hebrew was not reversed — drawn {drawn:?}, typed {logical:?}",
            path.display()
        );

        // Clusters are still sorted in the *logical* order, which is what
        // every query on the run depends on.
        assert!(
            run.glyphs().windows(2).all(|w| w[0].cluster <= w[1].cluster),
            "{}: reordering disturbed the clusters",
            path.display()
        );

        // One Latin letter each side: three runs, and only the middle one
        // turns round.
        let mixed = font.shape(&format!("A{HEBREW}B"));
        if mixed.len() == 5 {
            let ids: Vec<u32> = mixed.draw_order().map(|g| g.key.raw()).collect();
            let latin: Vec<u32> = font
                .shape("AB")
                .glyphs()
                .iter()
                .map(|g| g.key.raw())
                .collect();
            if latin.len() == 2 {
                assert_eq!(
                    ids,
                    [latin[0], logical[2], logical[1], logical[0], latin[1]],
                    "{}: the Latin either side of a Hebrew word moved — {ids:?}",
                    path.display()
                );
            }
        }

        // Latin alone is untouched, and pays for nothing.
        let latin = font.shape("The quick brown fox");
        assert!(
            !latin.is_reordered(),
            "{}: Latin came back reordered",
            path.display()
        );
        let by_order: Vec<u32> = latin.draw_order().map(|g| g.key.raw()).collect();
        let by_index: Vec<u32> = latin.glyphs().iter().map(|g| g.key.raw()).collect();
        assert_eq!(by_order, by_index, "{}: Latin draw order", path.display());

        // Width is the same whichever way the glyphs are walked. A kern given
        // back to the wrong glyph would still show here, because the sum over
        // the whole run would change.
        let mixed_width: f32 = mixed.draw_order().map(|g| g.advance).sum();
        assert!(
            (mixed_width - mixed.width()).abs() <= 0.01,
            "{}: the run is {mixed_width} px drawn and {} px measured",
            path.display(),
            mixed.width()
        );
    }

    println!("faces with Hebrew and Latin: {with_hebrew}");
    println!("faces checked:               {checked}");
    assert!(
        checked >= 1,
        "no installed face has three Hebrew letters and two Latin ones \
         ({with_hebrew} matched the cmap check)"
    );
}

/// A caller that knows its container's direction can say so, and it changes
/// the answer for text that rule P2 cannot decide for itself.
///
/// The string is `"(a"` — one neutral and one strong left-to-right character.
/// Under `Base::Auto` the `a` decides and it lays out as typed. Under
/// `Base::Rtl` the bracket takes the paragraph's direction instead, which
/// does two things at once: rule L4 mirrors it, and rule L2 moves it to the
/// other end. So a face that draws `(a` one way draws `a)` the other, from the
/// same string and the same glyphs.
#[test]
#[ignore = "reads the host's installed fonts"]
fn a_paragraph_direction_can_be_given_and_changes_the_answer() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut checked = 0usize;
    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data.clone()) else { continue };
        if !"(a)".chars().all(|ch| face.glyph_index(ch).is_some()) {
            continue;
        }
        let Ok(font) = ScaledFont::from_bytes(data, 16.0) else {
            continue;
        };

        // Every glyph id below is read out of a *whole shaped string*, never
        // out of a character shaped alone. On `Amiri-Bold.ttf` a lone `(` is
        // scriptless and comes back as glyph 3, while the `(` in `"(a"` joins
        // the Latin run and reaches a `latn` lookup that substitutes 6460 —
        // so an oracle built from isolated characters tests the itemizer, not
        // the direction.
        let auto = font.shape_with("(a", None, Base::Auto);
        let closed = font.shape_with(")a", None, Base::Auto);
        let rtl = font.shape_with("(a", None, Base::Rtl);
        if auto.len() != 2 || closed.len() != 2 || rtl.len() != 2 {
            // A face that ligated a bracket to a letter has said something
            // this oracle cannot read.
            continue;
        }
        let logical = |r: &osfont::shape::ShapedRun, i: usize| r.glyphs()[i].key.raw();
        if logical(&auto, 0) == logical(&closed, 0) {
            // Both brackets drawn with one glyph: nothing to see.
            continue;
        }
        checked += 1;

        // The default is unchanged, and naming it explicitly is the same call.
        let order: Vec<usize> = auto.draw_order().map(|g| g.cluster).collect();
        assert_eq!(
            order,
            [0, 1],
            "{}: Base::Auto reordered a left-to-right string",
            path.display()
        );
        assert_eq!(
            auto,
            font.shape("(a"),
            "{}: shape_with(.., Auto) is not shape()",
            path.display()
        );

        // Told the container runs the other way, both bidi rules fire: L2
        // moves the bracket to the far end...
        let order: Vec<usize> = rtl.draw_order().map(|g| g.cluster).collect();
        assert_eq!(
            order,
            [1, 0],
            "{}: Base::Rtl left the bracket where it was",
            path.display()
        );
        // ...and L4 draws it as the bracket that opens the other way.
        assert_eq!(
            logical(&rtl, 0),
            logical(&closed, 0),
            "{}: Base::Rtl did not mirror the bracket",
            path.display()
        );
        assert_ne!(
            logical(&rtl, 0),
            logical(&auto, 0),
            "{}: the mirrored bracket is the unmirrored one",
            path.display()
        );

        // Same glyph count. Not necessarily the same *width*: rule L4 puts a
        // different glyph in the run, and a face is free to give `)` a
        // different advance from `(` — `Amiri-Bold.ttf` measures 11.904 px
        // against 11.552 for exactly this reason. The direction decides which
        // ink is drawn and where, and the caller must re-measure after
        // changing it.
        assert_eq!(rtl.len(), auto.len(), "{}: glyph count", path.display());
        // Both are still self-consistent, which is the property that actually
        // matters: what is drawn sums to what is measured.
        for (what, r) in [("Auto", &auto), ("Rtl", &rtl)] {
            let drawn: f32 = r.draw_order().map(|g| g.advance).sum();
            assert!(
                (drawn - r.width()).abs() <= 0.01,
                "{}: {what} draws {drawn} px and measures {} px",
                path.display(),
                r.width()
            );
        }
    }

    println!("faces checked: {checked}");
    assert!(checked >= 1, "no installed face has `(`, `)` and `a` distinctly");
}

/// Whether the sfnt table directory names `tag`, read without the parser under
/// test.
///
/// Deliberately minimal and deliberately duplicated: its whole value is that it
/// shares no code with `Face::parse`, so the two cannot agree by both being
/// wrong in the same way.
fn directory_has_table(data: &[u8], tag: &[u8; 4]) -> bool {
    fn be32(data: &[u8], off: usize) -> Option<u32> {
        Some(u32::from_be_bytes(data.get(off..off + 4)?.try_into().ok()?))
    }
    fn inner(data: &[u8], tag: &[u8; 4]) -> Option<bool> {
        // A collection puts the real directory at an offset in its own header;
        // face 0 is the one `Face::parse` reads.
        let base = if be32(data, 0) == Some(0x7474_6366) {
            usize::try_from(be32(data, 12)?).ok()?
        } else {
            0
        };
        let count = u16::from_be_bytes(data.get(base + 4..base + 6)?.try_into().ok()?);
        Some((0..usize::from(count)).any(|i| {
            let rec = base + 12 + i * 16;
            data.get(rec..rec + 4).is_some_and(|t| t == tag)
        }))
    }
    inner(data, tag).unwrap_or(false)
}

/// Read the axes of every variable face installed on this host, and check them
/// against what an independent reader found in the same files.
///
/// The unit tests in `var.rs` run against fixtures whose bytes this crate also
/// wrote, so they prove the reader is self-consistent and nothing more. The
/// expectations below come from `gui/font/tools/variable_survey.py`, a separate
/// implementation in a different language that walks the same table directory —
/// so a shared misreading of the format has to be made twice, independently, to
/// go unnoticed. (It nearly did once in the other direction: the survey first
/// reported faces with fifty thousand axes, because it read `GDEF`'s
/// `ItemVariationStore` offset by counting fields instead of checking the table
/// version.)
///
/// The faces are named rather than detected. Detecting them would make the test
/// pass on a host where the reader had stopped recognising `fvar` entirely,
/// which is the exact regression it exists to catch.
///
/// Two of these are worth the space they take:
///
/// * **ReemKufi** ships `wght 400..400..700` — the below-default side has zero
///   width. The normalization formula divides by `default - min`, so this face
///   is a division by zero in any reader that implements the formula as
///   written. It is not a synthetic edge case; it is installed on this machine.
/// * **bahnschrift** ships `wdth 75..100..100`, the same degeneracy on the
///   other side.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_variable_fonts_report_their_axes() {
    use osfont::var::tags;

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    /// One axis as `fvar` describes it: tag, minimum, default, maximum.
    type AxisSpec = ([u8; 4], f32, f32, f32);
    /// One face's expected axis list, keyed by file name.
    type FaceAxes<'a> = (&'a str, &'a [AxisSpec]);

    // Exactly as the Python survey read them, in the order `fvar` stores them —
    // the order is part of the contract, since a normalized coordinate array is
    // positional and an axis list that came back permuted would apply the
    // weight to the width.
    let expected: &[FaceAxes<'_>] = &[
        ("CascadiaCode.ttf", &[(tags::WGHT, 200.0, 400.0, 700.0)]),
        ("CascadiaMono.ttf", &[(tags::WGHT, 200.0, 400.0, 700.0)]),
        ("ReemKufi.ttf", &[(tags::WGHT, 400.0, 400.0, 700.0)]),
        (
            "SegUIVar.ttf",
            &[
                (tags::WGHT, 300.0, 400.0, 700.0),
                (tags::OPSZ, 5.0, 10.5, 36.0),
            ],
        ),
        (
            "SitkaVF.ttf",
            &[
                (tags::OPSZ, 6.0, 11.0, 27.5),
                (tags::WGHT, 400.0, 400.0, 700.0),
            ],
        ),
        (
            "SitkaVF-Italic.ttf",
            &[
                (tags::OPSZ, 6.0, 11.0, 27.5),
                (tags::WGHT, 400.0, 400.0, 700.0),
            ],
        ),
        (
            "bahnschrift.ttf",
            &[
                (tags::WGHT, 300.0, 400.0, 700.0),
                (tags::WDTH, 75.0, 100.0, 100.0),
            ],
        ),
    ];

    let mut checked = 0usize;
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some((_, want)) = expected.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let vars = face
            .variation_axes()
            .unwrap_or_else(|| panic!("{name}: carries `fvar` but reports no axes"));

        let got: Vec<_> = vars
            .axes()
            .iter()
            .map(|a| (a.tag, a.min, a.default, a.max))
            .collect();
        assert_eq!(got.as_slice(), *want, "{name}: axes");

        // Every axis must be reachable by tag, and reachable at the position
        // the coordinate array uses. This is the lookup a caller asking for
        // "weight 700" actually goes through.
        for (i, (tag, _, _, _)) in want.iter().enumerate() {
            assert_eq!(vars.axis_index(tag), Some(i), "{name}: index of {tag:?}");
        }

        // The default instance normalizes to all zeros, by definition of the
        // normalized space — and `is_default` must agree with the bytes, since
        // it is what decides whether the variation machinery runs at all.
        let defaults: Vec<f32> = want.iter().map(|(_, _, d, _)| *d).collect();
        let at_default = vars.normalize(&defaults);
        assert!(
            at_default.as_slice().iter().all(|&c| c == 0),
            "{name}: default instance is not the origin: {:?}",
            at_default.as_slice()
        );
        assert!(at_default.is_default(), "{name}: is_default disagrees");
        assert_eq!(at_default, vars.default_coords(), "{name}: default_coords");

        // The ends. `avar` may bend the interior of an axis, but it is required
        // to fix the three endpoints, so -1/0/+1 survive it on every face —
        // which makes this the one cross-face assertion that can be exact.
        for (i, (_, min, _, max)) in want.iter().enumerate() {
            let mut low = defaults.clone();
            let mut high = defaults.clone();
            low[i] = *min;
            high[i] = *max;
            // A degenerate side (ReemKufi's `wght` minimum, bahnschrift's
            // `wdth` maximum) is *equal* to the default, so its end is the
            // origin rather than ±1. Reading it as ±1 would be a reader
            // inventing a design space the font does not have.
            let want_low = if *min == defaults[i] { 0 } else { -16384 };
            let want_high = if *max == defaults[i] { 0 } else { 16384 };
            assert_eq!(
                vars.normalize(&low).get(i),
                Some(want_low),
                "{name}: axis {i} at its minimum"
            );
            assert_eq!(
                vars.normalize(&high).get(i),
                Some(want_high),
                "{name}: axis {i} at its maximum"
            );
        }

        // Named instances must land inside the design space. A coordinate
        // outside [-1, 1] means the instance record was read at the wrong
        // stride — the failure mode a `Fixed` misread produces, and one that
        // otherwise shows up only as a garbled glyph much later.
        assert!(
            !vars.instances().is_empty(),
            "{name}: a shipping variable face with no named instances"
        );
        for (n, inst) in vars.instances().iter().enumerate() {
            assert_eq!(
                inst.coords.len(),
                want.len(),
                "{name}: instance {n} has the wrong coordinate count"
            );
            let coords = vars
                .instance_coords(n)
                .unwrap_or_else(|| panic!("{name}: instance {n} does not normalize"));
            assert_eq!(coords.len(), want.len(), "{name}: instance {n} normalized");
            for (axis, &c) in coords.as_slice().iter().enumerate() {
                assert!(
                    (-16384..=16384).contains(&c),
                    "{name}: instance {n} axis {axis} normalizes to {c}, outside the design space"
                );
            }
        }

        println!(
            "{name}: {} axes, {} named instances, avar {}",
            vars.axes().len(),
            vars.instances().len(),
            if vars.has_avar() { "yes" } else { "no" }
        );
        checked += 1;
    }

    assert_eq!(
        checked,
        expected.len(),
        "expected all {} named variable faces to be installed; found {checked}",
        expected.len()
    );
}

/// No non-variable face may claim to be variable, and every variable one must
/// survive being asked for a nonsense instance.
///
/// This is the other half of the named-face test above: that one proves the
/// seven known faces read correctly, this one proves the other 549 are not
/// being handed a `Variations` built out of whatever happened to sit at the
/// `fvar` offset. A reader that mistakes an unrelated table for `fvar` fails
/// here and nowhere else — the face still draws, it just quietly acquires axes,
/// and the first symptom is a glyph deformed by a delta applied to a font that
/// has none.
///
/// The converse is *not* asserted. A face with an `fvar` this reader declines
/// is allowed to come back as `None`: a malformed variation table costs the
/// face its variability, not its ability to draw, and that is the whole point
/// of `parse` returning an `Option` rather than an error.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn no_installed_face_invents_variation_axes() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    assert!(!files.is_empty(), "no fonts found on this host");
    files.sort();

    let mut variable = 0usize;
    let mut total = 0usize;
    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        // Whether the file has an `fvar` at all, read independently of the
        // parser under test: four bytes in the table directory, which is the
        // one thing both readers must agree on for the comparison to mean
        // anything.
        let has_fvar = directory_has_table(&data, b"fvar");
        let Ok(face) = Face::parse(data) else { continue };
        total += 1;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        let Some(vars) = face.variation_axes() else {
            continue;
        };
        assert!(has_fvar, "{name}: reports axes but has no `fvar` table");
        assert!(!vars.axes().is_empty(), "{name}: an `fvar` with no axes");
        // A design space of more than a handful of axes on a shipping desktop
        // font is a misread, not a font. The largest ever published carries
        // around eight.
        assert!(
            vars.axes().len() <= 8,
            "{name}: {} axes is a misread, not a font",
            vars.axes().len()
        );
        // Nonsense in must not be nonsense out. These are the values a caller
        // reaches by arithmetic on a slider, and every one of them must land
        // inside the design space.
        for probe in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e30, 1e30] {
            let user = vec![probe; vars.axes().len()];
            let coords = vars.normalize(&user);
            for &c in coords.as_slice() {
                assert!(
                    (-16384..=16384).contains(&c),
                    "{name}: {probe} normalizes to {c}"
                );
            }
        }
        // Too few user values leaves the rest at their defaults; too many drops
        // the extras. Both are the caller's mistake and neither may panic or
        // resize the coordinate array, which is indexed positionally by
        // everything downstream.
        assert_eq!(vars.normalize(&[]).len(), vars.axes().len(), "{name}: empty");
        assert!(vars.normalize(&[]).is_default(), "{name}: empty is default");
        let too_many = vec![400.0f32; vars.axes().len() + 5];
        assert_eq!(
            vars.normalize(&too_many).len(),
            vars.axes().len(),
            "{name}: over-long user coordinates resized the array"
        );
        variable += 1;
    }

    println!("{variable} variable of {total} faces");
    // The survey counted seven on this host. Fewer means the reader has stopped
    // recognising something it used to.
    assert!(
        variable >= 7,
        "only {variable} variable faces found; the survey counted 7"
    );
}

/// Every named instance of every installed variable face, normalized, against
/// an independent implementation.
///
/// This is the test that can tell whether `avar` was read at all.
/// [`installed_variable_fonts_report_their_axes`] cannot: the format requires
/// `avar` to fix the three identity points, so an axis's minimum, default and
/// maximum normalize to -1, 0 and +1 whether the correction is applied or not.
/// The bend is entirely in the interior -- and the interior is exactly where
/// the designer put the named instances.
///
/// Concretely, on this host: Cascadia's "SemiBold" is user weight 600, which a
/// reader with no `avar` normalizes to 10923 and one that applies it to 9830.
/// Segoe UI Variable's 600 is 8192 against a linear 10923, a third of the way
/// off. Both faces still draw, still report the right axes, and still hit
/// -1/0/+1 at the ends. A test built only from the ends is green for both
/// readers.
///
/// The expected values come from `variable_survey.py --normalize`, written from
/// the specification rather than transcribed from `var.rs` -- including its
/// truncating fixed-point division, so a disagreement here is a real bug and
/// not a rounding preference. Regenerate with:
///
/// ```text
/// python gui/font/tools/variable_survey.py --normalize
/// ```
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_variable_fonts_normalize_named_instances() {
    // (file name, one normalized coordinate array per named instance, in the
    // order `fvar` stores them). The trailing comment on each row is the
    // user-space position it came from.
    let expected: &[(&str, &[&[i16]])] = &[
        // CascadiaCode.ttf: wght, avar yes
        ("CascadiaCode.ttf", &[
            &[-16384],  // 200
            &[-10923],  // 300
            &[-5461],  // 350
            &[0],  // 400
            &[9830],  // 600
            &[16384],  // 700
        ]),
        // CascadiaMono.ttf: wght, avar yes
        ("CascadiaMono.ttf", &[
            &[-16384],  // 200
            &[-10923],  // 300
            &[-5461],  // 350
            &[0],  // 400
            &[9830],  // 600
            &[16384],  // 700
        ]),
        // ReemKufi.ttf: wght, avar no
        ("ReemKufi.ttf", &[
            &[0],  // 400
            &[5461],  // 500
            &[10923],  // 600
            &[16384],  // 700
        ]),
        // SegUIVar.ttf: wght/opsz, avar yes
        ("SegUIVar.ttf", &[
            &[-16384, -7447],  // 300, 8
            &[-8192, -7447],  // 350, 8
            &[0, -7447],  // 400, 8
            &[8192, -7447],  // 600, 8
            &[16384, -7447],  // 700, 8
            &[-16384, 0],  // 300, 10.5
            &[-8192, 0],  // 350, 10.5
            &[0, 0],  // 400, 10.5
            &[8192, 0],  // 600, 10.5
            &[16384, 0],  // 700, 10.5
            &[-16384, 16384],  // 300, 36
            &[-8192, 16384],  // 350, 36
            &[0, 16384],  // 400, 36
            &[8192, 16384],  // 600, 36
            &[16384, 16384],  // 700, 36
        ]),
        // SitkaVF-Italic.ttf: opsz/wght, avar yes
        ("SitkaVF-Italic.ttf", &[
            &[-16384, 0],  // 6, 400
            &[-16384, 9885],  // 6, 600
            &[-16384, 16384],  // 6, 700
            &[0, 0],  // 11, 400
            &[0, 9885],  // 11, 600
            &[0, 16384],  // 11, 700
            &[5699, 0],  // 16.7391, 400
            &[5699, 9885],  // 16.7391, 600
            &[5699, 16384],  // 16.7391, 700
            &[9973, 0],  // 21.0434, 400
            &[9973, 9885],  // 21.0434, 600
            &[9973, 16384],  // 21.0434, 700
            &[13534, 0],  // 24.6303, 400
            &[13534, 9885],  // 24.6303, 600
            &[13534, 16384],  // 24.6303, 700
            &[16384, 0],  // 27.5, 400
            &[16384, 9885],  // 27.5, 600
            &[16384, 16384],  // 27.5, 700
        ]),
        // SitkaVF.ttf: opsz/wght, avar yes
        ("SitkaVF.ttf", &[
            &[-16384, 0],  // 6, 400
            &[-16384, 9885],  // 6, 600
            &[-16384, 16384],  // 6, 700
            &[0, 0],  // 11, 400
            &[0, 9885],  // 11, 600
            &[0, 16384],  // 11, 700
            &[5699, 0],  // 16.7391, 400
            &[5699, 9885],  // 16.7391, 600
            &[5699, 16384],  // 16.7391, 700
            &[9973, 0],  // 21.0434, 400
            &[9973, 9885],  // 21.0434, 600
            &[9973, 16384],  // 21.0434, 700
            &[13534, 0],  // 24.6303, 400
            &[13534, 9885],  // 24.6303, 600
            &[13534, 16384],  // 24.6303, 700
            &[16384, 0],  // 27.5, 400
            &[16384, 9885],  // 27.5, 600
            &[16384, 16384],  // 27.5, 700
        ]),
        // bahnschrift.ttf: wght/wdth, avar yes
        ("bahnschrift.ttf", &[
            &[-16384, 0],  // 300, 100
            &[-8192, 0],  // 350, 100
            &[0, 0],  // 400, 100
            &[8192, 0],  // 600, 100
            &[16384, 0],  // 700, 100
            &[-16384, -8192],  // 300, 87.5
            &[-8192, -8192],  // 350, 87.5
            &[0, -8192],  // 400, 87.5
            &[8192, -8192],  // 600, 87.5
            &[16384, -8192],  // 700, 87.5
            &[-16384, -16384],  // 300, 75
            &[-8192, -16384],  // 350, 75
            &[0, -16384],  // 400, 75
            &[8192, -16384],  // 600, 75
            &[16384, -16384],  // 700, 75
        ]),
    ];

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    let mut checked = 0usize;
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some((_, want)) = expected.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let vars = face
            .variation_axes()
            .unwrap_or_else(|| panic!("{name}: carries `fvar` but reports no axes"));

        // The count first, and separately: a reader that lost an instance to a
        // stride error would otherwise fail on whichever row happened to shift,
        // which reads as an arithmetic bug rather than the truncation it is.
        assert_eq!(
            vars.instances().len(),
            want.len(),
            "{name}: named instance count"
        );
        for (n, want_coords) in want.iter().enumerate() {
            let got = vars
                .instance_coords(n)
                .unwrap_or_else(|| panic!("{name}: instance {n} does not normalize"));
            assert_eq!(
                got.as_slice(),
                *want_coords,
                "{name}: instance {n} (user {:?})",
                vars.instances()[n].coords
            );
        }
        checked += 1;
    }

    assert_eq!(
        checked,
        expected.len(),
        "expected all {} named variable faces to be installed; found {checked}",
        expected.len()
    );
    println!(
        "{checked} faces, {} named instances",
        expected.iter().map(|(_, i)| i.len()).sum::<usize>()
    );
}


/// A glyph outline reduced to the few numbers a test can compare across two
/// independent implementations.
///
/// The command count catches a structural change, the bounding box catches a
/// shift or a scale, and the two coordinate sums catch a single wrong point
/// anywhere -- including an interior one that never touches the box. Storing
/// every coordinate instead would be thousands of numbers per face and no more
/// conclusive; a hash would be as compact but could only ever say "wrong",
/// never "wrong how".
fn outline_record(o: &Outline) -> OutlineRecord {
    let mut sum_x = 0.0f32;
    let mut sum_y = 0.0f32;
    let mut add = |p: Point| {
        sum_x += p.x;
        sum_y += p.y;
    };
    for cmd in &o.commands {
        match *cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => add(p),
            PathCmd::QuadTo(a, b) => {
                add(a);
                add(b);
            }
            PathCmd::CurveTo(a, b, c) => {
                add(a);
                add(b);
                add(c);
            }
            PathCmd::Close => {}
        }
    }
    let b = o.bbox().expect("a non-empty outline has a bounding box");
    (
        o.commands.len(),
        b.x_min,
        b.y_min,
        b.x_max,
        b.y_max,
        sum_x,
        sum_y,
    )
}

/// Compare one glyph against the oracle, naming the field that disagreed.
///
/// The tolerances exist because the two sides accumulate in different
/// precisions -- `f32` here, float64 in the Python -- and the expectations are
/// printed to one decimal. A test that demanded bit equality would be reporting
/// on IEEE 754 rather than on `gvar`.
fn assert_outline_matches(what: &str, got: OutlineRecord, want: OutlineRecord) {
    assert_eq!(got.0, want.0, "{what}: path command count");
    let near = |a: f32, b: f32, tol: f32, field: &str| {
        assert!(
            (a - b).abs() <= tol,
            "{what}: {field} is {a}, oracle says {b}"
        );
    };
    near(got.1, want.1, 0.5, "x_min");
    near(got.2, want.2, 0.5, "y_min");
    near(got.3, want.3, 0.5, "x_max");
    near(got.4, want.4, 0.5, "y_max");
    // The sums run to tens of thousands of font units, where `f32` itself has
    // only about four fractional bits left, so they get a proportional slack.
    let slack = |v: f32| (v.abs() * 1e-4).max(0.5);
    near(got.5, want.5, slack(want.5), "sum of x");
    near(got.6, want.6, slack(want.6), "sum of y");
}

/// Every glyph of every variable face, drawn at its own default coordinates,
/// is the glyph `glyf` already held.
///
/// This is the one property of variable-font rendering that admits no
/// tolerance: the default instance *is* the outline in `glyf`, and every
/// tuple's scalar is zero there by construction, so the varying path and the
/// plain one must agree exactly. `Face::outline_at` takes a short cut at the
/// default coordinates and this pins that the short cut tells the truth --
/// which is also why the sweep is over every glyph rather than a handful: the
/// cheap assertion is the one that can afford to be exhaustive.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn a_variable_face_at_its_default_instance_draws_the_default_outline() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    let mut faces = 0usize;
    let mut glyphs = 0usize;
    for path in &files {
        let Ok(data) = fs::read(path) else { continue };
        if !directory_has_table(&data, b"gvar") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let Ok(face) = Face::parse(data) else { continue };
        let Some(vars) = face.variation_axes() else {
            continue;
        };
        let default = vars.default_coords();
        assert!(default.is_default(), "{name}: default coords are not default");

        // A sample rather than the whole face: 600 glyphs reaches composites and
        // unusual contours in every one of these fonts, and Sitka alone is 3000
        // glyphs across two faces.
        for gid in 0..face.num_glyphs().min(600) {
            let Ok(plain) = face.outline(gid) else { continue };
            let varied = face
                .outline_at(gid, &default)
                .unwrap_or_else(|e| panic!("{name}: glyph {gid} at default: {e:?}"));
            assert_eq!(
                plain.commands, varied.commands,
                "{name}: glyph {gid} differs at the default instance"
            );
            glyphs += 1;
        }
        faces += 1;
    }
    assert!(faces >= 7, "expected at least 7 faces with `gvar`; saw {faces}");
    println!("{faces} faces, {glyphs} glyphs identical at the default instance");
}

/// The oracle's own `glyf` reader agrees with the crate's.
///
/// `gvar_oracle.py` has to read `glyf` and turn points into a path before it
/// can say anything about variation, and those two parts necessarily follow the
/// same rules this crate does -- a slip in either would look exactly like a
/// variation bug. So they are pinned separately, here, at the default instance
/// where `gvar` provably contributes nothing. A failure in this test is a
/// transcription error in the tool; a failure in
/// [`installed_variable_fonts_vary_their_outlines`] while this one passes is a
/// real disagreement about variation.
///
/// Regenerate with:
///
/// ```text
/// python gui/font/tools/gvar_oracle.py
/// ```
#[test]
#[ignore = "depends on the host's installed fonts"]
fn the_outline_oracle_agrees_about_unvaried_glyphs() {
    // (file, glyph id, command count, x_min, y_min, x_max, y_max, sum x, sum y)
    let expected: &[DefaultOutline] = &[
    ("CascadiaCode.ttf", 0, 24, 128.0, 0.0, 1072.0, 1420.0, 10844.0, 12590.0),
    ("CascadiaCode.ttf", 1, 16, 45.0, 0.0, 1155.0, 1420.0, 7460.0, 7446.0),
    ("CascadiaCode.ttf", 2, 22, 45.0, 0.0, 1155.0, 1830.0, 10759.0, 15846.0),
    ("CascadiaMono.ttf", 0, 24, 128.0, 0.0, 1072.0, 1420.0, 10844.0, 12590.0),
    ("CascadiaMono.ttf", 1, 16, 45.0, 0.0, 1155.0, 1420.0, 7460.0, 7446.0),
    ("CascadiaMono.ttf", 2, 22, 45.0, 0.0, 1155.0, 1830.0, 10759.0, 15846.0),
    ("ReemKufi.ttf", 2, 16, 28.0, 0.0, 672.0, 755.0, 4454.0, 3921.0),
    ("ReemKufi.ttf", 27, 28, 11.0, 0.0, 890.0, 725.0, 14189.5, 10469.0),
    ("ReemKufi.ttf", 3, 22, 28.0, 0.0, 672.0, 918.0, 6564.0, 8243.0),
    ("SegUIVar.ttf", 0, 12, 166.0, 0.0, 1156.0, 1398.0, 5768.0, 5740.0),
    ("SegUIVar.ttf", 3, 16, 175.0, -18.0, 407.0, 1434.0, 6475.0, 6628.0),
    ("SegUIVar.ttf", 110, 6, 144.0, 506.0, 690.0, 635.0, 1812.0, 2788.0),
    ("SitkaVF-Italic.ttf", 0, 12, 0.0, 0.0, 1024.0, 1434.0, 4174.0, 7245.0),
    ("SitkaVF-Italic.ttf", 4, 30, -107.0, 0.0, 1362.0, 1456.0, 20710.0, 12304.5),
    ("SitkaVF-Italic.ttf", 30, 41, -107.0, 0.0, 1362.0, 1904.0, 33682.0, 41008.0),
    ("SitkaVF.ttf", 0, 12, 0.0, 0.0, 1024.0, 1434.0, 5035.0, 5818.0),
    ("SitkaVF.ttf", 4, 29, -1.0, 0.0, 1449.0, 1456.0, 21197.5, 12052.5),
    ("SitkaVF.ttf", 30, 40, -1.0, 0.0, 1449.0, 1904.0, 30794.0, 40489.5),
    ("bahnschrift.ttf", 0, 12, 180.0, 0.0, 1288.0, 1454.0, 7515.0, 7095.0),
    ("bahnschrift.ttf", 2, 15, 40.0, 0.0, 1286.0, 1454.0, 7646.0, 6064.0),
    ("bahnschrift.ttf", 3, 21, 40.0, 0.0, 1286.0, 1917.0, 11401.0, 14541.0),
    ];

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    let mut checked = 0usize;
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !expected.iter().any(|r| r.0.eq_ignore_ascii_case(name)) {
            continue;
        }
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        for &(_, gid, cmds, x0, y0, x1, y1, sx, sy) in
            expected.iter().filter(|r| r.0.eq_ignore_ascii_case(name))
        {
            let outline = face
                .outline(gid)
                .unwrap_or_else(|e| panic!("{name}: glyph {gid}: {e:?}"));
            assert_outline_matches(
                &format!("{name} glyph {gid}"),
                outline_record(&outline),
                (cmds, x0, y0, x1, y1, sx, sy),
            );
            checked += 1;
        }
    }
    assert_eq!(checked, expected.len(), "not every named face was installed");
    println!("{checked} unvaried glyphs agree with the oracle");
}

/// `gvar` moves the points where an independent reader says it should.
///
/// This is the test the whole `gvar` module exists to pass. The expectations
/// come from `gvar_oracle.py`, written from the OpenType specification rather
/// than transcribed from `gui/font/src/gvar.rs`, so agreement means the format
/// was read correctly twice, independently -- a shared misreading has to be
/// made twice, the same way, to survive.
///
/// Three named instances per face (the first, the middle and the last) rather
/// than all of them: the ends of each axis are where a tuple is at full
/// strength, and the middle is where it tapers and where `avar` bends the
/// coordinate, which between them reach every branch of the scalar.
///
/// Regenerate with:
///
/// ```text
/// python gui/font/tools/gvar_oracle.py
/// ```
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_variable_fonts_vary_their_outlines() {
    // (file, glyph id, named instance, command count, bbox, sum x, sum y).
    // The trailing comment is the instance's user-space position.
    let expected: &[VariedOutline] = &[
    ("CascadiaCode.ttf", 0, 0, 24, 154.0, 0.0, 1046.0, 1420.0, 11018.0, 12692.0),  // 200
    ("CascadiaCode.ttf", 1, 0, 16, 69.0, 0.0, 1131.0, 1420.0, 7484.0, 7687.0),  // 200
    ("CascadiaCode.ttf", 2, 0, 22, 69.0, 0.0, 1131.0, 1830.0, 10888.0, 16087.0),  // 200
    ("CascadiaCode.ttf", 0, 3, 24, 128.0, 0.0, 1072.0, 1420.0, 10844.0, 12590.0),  // 400
    ("CascadiaCode.ttf", 1, 3, 16, 45.0, 0.0, 1155.0, 1420.0, 7460.0, 7446.0),  // 400
    ("CascadiaCode.ttf", 2, 3, 22, 45.0, 0.0, 1155.0, 1830.0, 10759.0, 15846.0),  // 400
    ("CascadiaCode.ttf", 0, 5, 24, 111.0, 0.0, 1089.0, 1420.0, 10756.0, 12537.0),  // 700
    ("CascadiaCode.ttf", 1, 5, 16, 28.0, 0.0, 1172.0, 1420.0, 7443.0, 6819.0),  // 700
    ("CascadiaCode.ttf", 2, 5, 22, 28.0, 0.0, 1172.0, 1855.0, 10793.0, 15269.0),  // 700
    ("CascadiaMono.ttf", 0, 0, 24, 154.0, 0.0, 1046.0, 1420.0, 11018.0, 12692.0),  // 200
    ("CascadiaMono.ttf", 1, 0, 16, 69.0, 0.0, 1131.0, 1420.0, 7484.0, 7687.0),  // 200
    ("CascadiaMono.ttf", 2, 0, 22, 69.0, 0.0, 1131.0, 1830.0, 10888.0, 16087.0),  // 200
    ("CascadiaMono.ttf", 0, 3, 24, 128.0, 0.0, 1072.0, 1420.0, 10844.0, 12590.0),  // 400
    ("CascadiaMono.ttf", 1, 3, 16, 45.0, 0.0, 1155.0, 1420.0, 7460.0, 7446.0),  // 400
    ("CascadiaMono.ttf", 2, 3, 22, 45.0, 0.0, 1155.0, 1830.0, 10759.0, 15846.0),  // 400
    ("CascadiaMono.ttf", 0, 5, 24, 111.0, 0.0, 1089.0, 1420.0, 10756.0, 12537.0),  // 700
    ("CascadiaMono.ttf", 1, 5, 16, 28.0, 0.0, 1172.0, 1420.0, 7443.0, 6819.0),  // 700
    ("CascadiaMono.ttf", 2, 5, 22, 28.0, 0.0, 1172.0, 1855.0, 10793.0, 15269.0),  // 700
    ("ReemKufi.ttf", 2, 0, 16, 28.0, 0.0, 672.0, 755.0, 4454.0, 3921.0),  // 400
    ("ReemKufi.ttf", 27, 0, 28, 11.0, 0.0, 890.0, 725.0, 14189.5, 10469.0),  // 400
    ("ReemKufi.ttf", 3, 0, 22, 28.0, 0.0, 672.0, 918.0, 6564.0, 8243.0),  // 400
    ("ReemKufi.ttf", 2, 2, 16, 17.3, 0.0, 680.0, 761.7, 4438.7, 3816.3),  // 600
    ("ReemKufi.ttf", 27, 2, 28, 7.0, 0.0, 904.7, 732.3, 14551.5, 10359.7),  // 600
    ("ReemKufi.ttf", 3, 2, 22, 17.3, 0.0, 680.0, 932.0, 6594.7, 8207.0),  // 600
    ("ReemKufi.ttf", 2, 3, 16, 12.0, 0.0, 684.0, 765.0, 4431.0, 3764.0),  // 700
    ("ReemKufi.ttf", 27, 3, 28, 5.0, 0.0, 912.0, 736.0, 14732.5, 10305.0),  // 700
    ("ReemKufi.ttf", 3, 3, 22, 12.0, 0.0, 684.0, 939.0, 6610.0, 8189.0),  // 700
    ("SegUIVar.ttf", 0, 0, 12, 176.0, 0.0, 1166.0, 1398.0, 5868.0, 5740.0),  // 300, 8
    ("SegUIVar.ttf", 3, 0, 16, 206.9, -18.0, 397.1, 1434.0, 6697.4, 6302.4),  // 300, 8
    ("SegUIVar.ttf", 110, 0, 6, 154.5, 541.8, 701.1, 647.3, 1865.8, 2920.0),  // 300, 8
    ("SegUIVar.ttf", 0, 7, 12, 166.0, 0.0, 1156.0, 1398.0, 5768.0, 5740.0),  // 400, 10.5
    ("SegUIVar.ttf", 3, 7, 16, 175.0, -18.0, 407.0, 1434.0, 6475.0, 6628.0),  // 400, 10.5
    ("SegUIVar.ttf", 110, 7, 6, 144.0, 506.0, 690.0, 635.0, 1812.0, 2788.0),  // 400, 10.5
    ("SegUIVar.ttf", 0, 14, 12, 156.0, 0.0, 1146.0, 1398.0, 5668.0, 5740.0),  // 700, 36
    ("SegUIVar.ttf", 3, 14, 16, 142.0, -23.0, 508.0, 1434.0, 7321.0, 7742.0),  // 700, 36
    ("SegUIVar.ttf", 110, 14, 6, 133.0, 436.0, 680.0, 657.0, 1759.0, 2622.0),  // 700, 36
    ("SitkaVF-Italic.ttf", 0, 0, 12, 0.0, 0.0, 1024.0, 1484.0, 4185.0, 7506.0),  // 6, 400
    ("SitkaVF-Italic.ttf", 4, 0, 30, -91.0, 0.0, 1449.0, 1506.0, 22435.0, 12897.0),  // 6, 400
    ("SitkaVF-Italic.ttf", 30, 0, 41, -91.0, 0.0, 1449.0, 1997.0, 36432.0, 42958.0),  // 6, 400
    ("SitkaVF-Italic.ttf", 0, 9, 12, 0.0, 0.0, 1024.0, 1399.9, 4174.0, 7074.6),  // 21.0434, 400
    ("SitkaVF-Italic.ttf", 4, 9, 30, -144.1, 0.0, 1251.8, 1421.3, 18260.9, 11676.0),  // 21.0434, 400
    ("SitkaVF-Italic.ttf", 30, 9, 41, -144.1, 0.0, 1251.8, 1840.7, 30079.7, 39488.1),  // 21.0434, 400
    ("SitkaVF-Italic.ttf", 0, 17, 12, 0.0, 0.0, 1024.0, 1378.0, 4179.0, 6970.0),  // 27.5, 700
    ("SitkaVF-Italic.ttf", 4, 17, 30, -179.0, 0.0, 1282.0, 1399.0, 17639.5, 11438.5),  // 27.5, 700
    ("SitkaVF-Italic.ttf", 30, 17, 41, -179.0, 0.0, 1282.0, 1864.0, 28250.0, 39451.0),  // 27.5, 700
    ("SitkaVF.ttf", 0, 0, 12, 0.0, 0.0, 1024.0, 1484.0, 5035.0, 6018.0),  // 6, 400
    ("SitkaVF.ttf", 4, 0, 29, 16.0, 0.0, 1534.0, 1506.0, 22748.5, 12614.5),  // 6, 400
    ("SitkaVF.ttf", 30, 0, 40, 16.0, 0.0, 1534.0, 1997.0, 33293.0, 42365.5),  // 6, 400
    ("SitkaVF.ttf", 0, 9, 12, 0.0, 0.0, 1024.0, 1399.9, 5035.0, 5681.7),  // 21.0434, 400
    ("SitkaVF.ttf", 4, 9, 29, -35.1, 0.0, 1346.1, 1421.3, 19098.7, 11475.4),  // 21.0434, 400
    ("SitkaVF.ttf", 30, 9, 40, -35.1, 0.0, 1346.1, 1840.7, 27925.5, 39063.9),  // 21.0434, 400
    ("SitkaVF.ttf", 0, 17, 12, 0.0, 0.0, 1024.0, 1378.0, 5035.0, 5594.0),  // 27.5, 700
    ("SitkaVF.ttf", 4, 17, 29, -73.0, 0.0, 1358.0, 1399.0, 18252.5, 11091.0),  // 27.5, 700
    ("SitkaVF.ttf", 30, 17, 40, -73.0, 0.0, 1358.0, 1858.0, 26836.5, 38771.5),  // 27.5, 700
    ("bahnschrift.ttf", 0, 0, 12, 180.0, 0.0, 1288.0, 1454.0, 7480.0, 7130.0),  // 300, 100
    ("bahnschrift.ttf", 2, 0, 15, 40.0, 0.0, 1286.0, 1454.0, 7646.0, 6244.0),  // 300, 100
    ("bahnschrift.ttf", 3, 0, 21, 40.0, 0.0, 1286.0, 1877.0, 11351.0, 14611.0),  // 300, 100
    ("bahnschrift.ttf", 0, 7, 12, 140.0, 0.0, 1088.0, 1454.0, 6312.5, 7100.0),  // 400, 87.5
    ("bahnschrift.ttf", 2, 7, 15, 35.0, 0.0, 1081.0, 1454.0, 6467.0, 6035.5),  // 400, 87.5
    ("bahnschrift.ttf", 3, 7, 21, 35.0, 0.0, 1081.0, 1914.0, 9649.0, 14487.0),  // 400, 87.5
    ("bahnschrift.ttf", 0, 14, 12, 89.0, 0.0, 899.0, 1454.0, 5168.0, 7042.0),  // 700, 75
    ("bahnschrift.ttf", 2, 14, 15, 19.0, 0.0, 887.0, 1454.0, 5257.0, 5455.0),  // 700, 75
    ("bahnschrift.ttf", 3, 14, 21, 19.0, 0.0, 887.0, 1957.0, 7844.0, 13978.0),  // 700, 75
    ];

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    let mut checked = 0usize;
    let mut moved = 0usize;
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !expected.iter().any(|r| r.0.eq_ignore_ascii_case(name)) {
            continue;
        }
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let vars = face
            .variation_axes()
            .unwrap_or_else(|| panic!("{name}: carries `fvar` but reports no axes"));

        for &(_, gid, instance, cmds, x0, y0, x1, y1, sx, sy) in
            expected.iter().filter(|r| r.0.eq_ignore_ascii_case(name))
        {
            let coords = vars
                .instance_coords(instance)
                .unwrap_or_else(|| panic!("{name}: no named instance {instance}"));
            let outline = face
                .outline_at(gid, &coords)
                .unwrap_or_else(|e| panic!("{name}: glyph {gid} at instance {instance}: {e:?}"));
            assert_outline_matches(
                &format!("{name} glyph {gid} at instance {instance}"),
                outline_record(&outline),
                (cmds, x0, y0, x1, y1, sx, sy),
            );
            // A row at a non-default instance must actually differ from the
            // unvaried glyph. Without this the whole comparison could be
            // satisfied by a reader that ignores `gvar` and an oracle that
            // agrees with it about nothing.
            if !coords.is_default() {
                let plain = face
                    .outline(gid)
                    .unwrap_or_else(|e| panic!("{name}: glyph {gid}: {e:?}"));
                assert_ne!(
                    plain.commands, outline.commands,
                    "{name}: glyph {gid} did not move at instance {instance}"
                );
                moved += 1;
            }
            checked += 1;
        }
    }
    assert_eq!(checked, expected.len(), "not every named face was installed");
    assert!(
        moved * 4 >= checked * 3,
        "only {moved} of {checked} rows actually varied; the sample is too weak"
    );
    println!("{checked} varied glyphs agree with the oracle ({moved} moved)");
}

/// A weight axis reaches the pixels, on real files.
///
/// `installed_variable_fonts_vary_their_outlines` proves the deltas are read
/// correctly; this proves they are *plumbed* — that a caller who says "weight
/// 700" gets a heavier 'H' out of `ScaledFont` rather than the default one.
/// Those are different failures: an unwired `ScaledFont` would still pass every
/// outline test in this file.
///
/// Rasterizing rather than comparing outlines is the point. It is the only
/// check that survives the whole pipeline — instance to coordinates to deltas
/// to path to coverage — and the only one a user could have noticed.
#[test]
#[ignore = "depends on the host's installed fonts"]
fn a_weight_axis_reaches_the_rasterized_glyph() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    let mut checked = 0usize;
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let Some(vars) = face.variation_axes() else {
            continue;
        };
        let Some(axis) = vars.axes().iter().find(|a| a.tag == *b"wght") else {
            continue;
        };
        // A degenerate axis — one whose ends coincide — has nothing to prove
        // and would fail for the right reason in the wrong test.
        if axis.min >= axis.max {
            continue;
        }
        let (heaviest, default_weight) = (axis.max, axis.default);
        let face = std::sync::Arc::new(face);
        // 64 pixels: large enough that a weight difference is several pixels of
        // ink rather than one of antialiasing, so this is not a test of the
        // rasterizer's rounding.
        let mut font = ScaledFont::shared(std::sync::Arc::clone(&face), 64.0).unwrap();
        let gid = font.glyph_id('H');
        if gid == 0 {
            continue;
        }
        let light = font.glyph(gid).unwrap().mask.clone();
        font.set_axes(&[(*b"wght", heaviest)]);
        let heavy = font.glyph(gid).unwrap().mask.clone();

        // Heavier means more ink. Comparing total coverage rather than the
        // bitmaps says *which way* it moved, which is what distinguishes a
        // wired-up axis from one that is merely different.
        let ink = |m: &osfont::raster::GlyphMask| -> u64 {
            m.coverage.iter().map(|&c| u64::from(c)).sum()
        };
        assert!(
            ink(&heavy) > ink(&light),
            "{name}: weight {} should put more ink on the page than {}: {} vs {}",
            heaviest,
            default_weight,
            ink(&heavy),
            ink(&light)
        );

        // And going back to the default reproduces the original exactly, which
        // is the check that the instance is a *view* and nothing was mutated.
        font.set_axes(&[(*b"wght", default_weight)]);
        let again = font.glyph(gid).unwrap().mask.clone();
        assert_eq!(
            again.coverage, light.coverage,
            "{name}: returning to the default weight did not reproduce the default glyph"
        );
        checked += 1;
    }
    assert!(
        checked >= 4,
        "only {checked} faces with a usable weight axis were found; expected the \
         host's variable fonts"
    );
    println!("{checked} faces put more ink on the page at their heaviest weight");
}

/// Every named instance of every variable face, advance by advance, against an
/// independent reader of `HVAR`.
///
/// This is the test that can tell whether `HVAR` was read at all. Until it
/// existed a varied glyph was drawn at its varied shape and spaced at its
/// *default* width, and nothing in the suite noticed: the outline tests all
/// compare shapes, and the shape is right. What is wrong is the gap after it,
/// which accumulates along a line -- a heavy instance of Segoe UI Variable is
/// up to 15% wider per glyph than its default, so a line of forty characters
/// ends several glyphs short of where it belongs.
///
/// Two of the seven faces here (Cascadia Code and Mono) are in the table with
/// every row identical, and that is not padding: their `HVAR` has a null
/// advance index map, which means the *implicit* map (outer 0, inner = glyph
/// id) rather than "no variation". A reader that treated the null offset as an
/// absent feature would still pass this, because their store happens to vary
/// nothing -- which is exactly why `varstore.rs`'s unit tests carry a synthetic
/// fixture for that path. What these two rows do pin is that the null offset is
/// not read as an *error* that costs the face its advances entirely.
///
/// The glyphs sampled are the eight the oracle takes, spread across the whole
/// glyph order rather than clustered in the Latin block at the front.
/// Regenerate with:
///
/// ```text
/// python gui/font/tools/varstore_oracle.py --hvar
/// ```
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_variable_fonts_vary_their_advances() {
    // (file name, one advance row per instance: the default first, then the
    // named instances in `fvar` order).
    let expected: &[(&str, &[&[u16]])] = &[
        ("CascadiaCode.ttf", &[
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // default
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 200
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 300
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 350
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 400
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 600
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 700
        ]),  // gids [0, 222, 444, 666, 888, 1110, 1332, 1554]
        ("CascadiaMono.ttf", &[
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // default
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 200
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 300
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 350
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 400
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 600
            &[1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200],  // 700
        ]),  // gids [0, 222, 444, 666, 888, 1110, 1332, 1554]
        ("ReemKufi.ttf", &[
            &[500, 806, 591, 549, 850, 720, 460, 0],  // default
            &[500, 806, 591, 549, 850, 720, 460, 0],  // 400
            &[500, 805, 599, 556, 850, 720, 466, 0],  // 500
            &[500, 804, 606, 564, 850, 720, 473, 0],  // 600
            &[500, 803, 614, 571, 850, 720, 479, 0],  // 700
        ]),  // gids [0, 101, 202, 303, 404, 505, 606, 707]
        ("SegUIVar.ttf", &[
            &[1322, 1168, 813, 1200, 1073, 1042, 256, 861],  // default
            &[1342, 1187, 786, 1209, 1027, 1061, 256, 906],  // 300, 8
            &[1342, 1190, 816, 1218, 1064, 1072, 206, 908],  // 350, 8
            &[1342, 1195, 846, 1246, 1098, 1085, 256, 915],  // 400, 8
            &[1342, 1224, 902, 1272, 1123, 1108, 256, 970],  // 600, 8
            &[1342, 1257, 965, 1302, 1150, 1135, 256, 1026],  // 700, 8
            &[1322, 1167, 734, 1149, 962, 1012, 256, 868],  // 300, 10.5
            &[1322, 1167, 773, 1155, 1017, 1026, 164, 864],  // 350, 10.5
            &[1322, 1168, 813, 1200, 1073, 1042, 256, 861],  // 400, 10.5
            &[1322, 1202, 877, 1230, 1101, 1069, 256, 926],  // 600, 10.5
            &[1322, 1243, 955, 1266, 1135, 1102, 256, 992],  // 700, 10.5
            &[1302, 1109, 746, 1109, 939, 971, 256, 886],  // 300, 36
            &[1302, 1129, 756, 1146, 961, 996, 256, 916],  // 350, 36
            &[1302, 1112, 804, 1195, 1053, 1014, 256, 920],  // 400, 36
            &[1302, 1146, 867, 1226, 1081, 1045, 256, 981],  // 600, 36
            &[1302, 1187, 943, 1263, 1115, 1082, 256, 1055],  // 700, 36
        ]),  // gids [0, 316, 632, 948, 1264, 1580, 1896, 2212]
        ("SitkaVF-Italic.ttf", &[
            &[1024, 1651, 1225, 763, 1024, 1310, 1342, 1321],  // default
            &[1024, 1771, 1320, 836, 1047, 1408, 1437, 1432],  // 6, 400
            &[1024, 1807, 1366, 873, 1047, 1439, 1509, 1462],  // 6, 600
            &[1024, 1830, 1396, 898, 1047, 1459, 1556, 1482],  // 6, 700
            &[1024, 1651, 1225, 763, 1024, 1310, 1342, 1321],  // 11, 400
            &[1024, 1682, 1268, 802, 1024, 1343, 1405, 1362],  // 11, 600
            &[1024, 1703, 1296, 828, 1024, 1364, 1447, 1389],  // 11, 700
            &[1024, 1559, 1150, 701, 1024, 1234, 1269, 1230],  // 16.7391, 400
            &[1024, 1586, 1190, 743, 1024, 1269, 1326, 1284],  // 16.7391, 600
            &[1024, 1604, 1217, 771, 1024, 1293, 1363, 1320],  // 16.7391, 700
            &[1024, 1490, 1093, 654, 1024, 1177, 1215, 1162],  // 21.0434, 400
            &[1024, 1514, 1132, 698, 1024, 1215, 1266, 1226],  // 21.0434, 600
            &[1024, 1530, 1158, 728, 1024, 1239, 1300, 1268],  // 21.0434, 700
            &[1024, 1433, 1046, 615, 1024, 1130, 1169, 1105],  // 24.6303, 400
            &[1024, 1454, 1084, 661, 1024, 1169, 1216, 1177],  // 24.6303, 600
            &[1024, 1468, 1109, 692, 1024, 1195, 1247, 1225],  // 24.6303, 700
            &[1024, 1387, 1008, 584, 1024, 1092, 1133, 1060],  // 27.5, 400
            &[1024, 1406, 1045, 632, 1024, 1132, 1176, 1138],  // 27.5, 600
            &[1024, 1419, 1070, 663, 1024, 1159, 1205, 1190],  // 27.5, 700
        ]),  // gids [0, 148, 296, 444, 592, 740, 888, 1036]
        ("SitkaVF.ttf", &[
            &[1024, 1650, 1255, 764, 1024, 1318, 1442, 1321],  // default
            &[1024, 1770, 1354, 835, 1047, 1416, 1553, 1432],  // 6, 400
            &[1024, 1799, 1400, 873, 1047, 1468, 1602, 1462],  // 6, 600
            &[1024, 1818, 1430, 898, 1047, 1502, 1634, 1482],  // 6, 700
            &[1024, 1650, 1255, 764, 1024, 1318, 1442, 1321],  // 11, 400
            &[1024, 1681, 1298, 803, 1024, 1367, 1491, 1362],  // 11, 600
            &[1024, 1701, 1327, 829, 1024, 1399, 1524, 1389],  // 11, 700
            &[1024, 1558, 1177, 705, 1024, 1242, 1355, 1230],  // 16.7391, 400
            &[1024, 1591, 1218, 746, 1024, 1289, 1406, 1284],  // 16.7391, 600
            &[1024, 1613, 1245, 773, 1024, 1319, 1439, 1320],  // 16.7391, 700
            &[1024, 1489, 1118, 661, 1024, 1185, 1290, 1162],  // 21.0434, 400
            &[1024, 1524, 1158, 703, 1024, 1230, 1342, 1226],  // 21.0434, 600
            &[1024, 1547, 1184, 730, 1024, 1260, 1376, 1268],  // 21.0434, 700
            &[1024, 1432, 1069, 624, 1024, 1137, 1236, 1105],  // 24.6303, 400
            &[1024, 1468, 1108, 667, 1024, 1181, 1289, 1177],  // 24.6303, 600
            &[1024, 1492, 1133, 695, 1024, 1210, 1323, 1225],  // 24.6303, 700
            &[1024, 1386, 1030, 595, 1024, 1099, 1193, 1060],  // 27.5, 400
            &[1024, 1423, 1067, 638, 1024, 1142, 1246, 1138],  // 27.5, 600
            &[1024, 1448, 1092, 667, 1024, 1170, 1281, 1190],  // 27.5, 700
        ]),  // gids [0, 148, 296, 444, 592, 740, 888, 1036]
        ("bahnschrift.ttf", &[
            &[1468, 1162, 1139, 1208, 1464, 756, 1442, 1136],  // default
            &[1468, 1162, 1139, 1208, 1464, 756, 1442, 1136],  // 300, 100
            &[1468, 1162, 1139, 1208, 1464, 756, 1442, 1136],  // 350, 100
            &[1468, 1162, 1139, 1208, 1464, 756, 1442, 1136],  // 400, 100
            &[1468, 1162, 1139, 1208, 1464, 756, 1442, 1136],  // 600, 100
            &[1468, 1162, 1139, 1208, 1464, 756, 1442, 1136],  // 700, 100
            &[1228, 982, 986, 1038, 1297, 661, 1268, 964],  // 300, 87.5
            &[1228, 982, 986, 1038, 1297, 661, 1268, 964],  // 350, 87.5
            &[1228, 982, 986, 1038, 1297, 661, 1268, 964],  // 400, 87.5
            &[1228, 982, 986, 1038, 1297, 661, 1268, 964],  // 600, 87.5
            &[1228, 982, 986, 1038, 1297, 661, 1268, 964],  // 700, 87.5
            &[988, 802, 834, 868, 1130, 566, 1094, 792],  // 300, 75
            &[988, 802, 834, 868, 1130, 566, 1094, 792],  // 350, 75
            &[988, 802, 834, 868, 1130, 566, 1094, 792],  // 400, 75
            &[988, 802, 834, 868, 1130, 566, 1094, 792],  // 600, 75
            &[988, 802, 834, 868, 1130, 566, 1094, 792],  // 700, 75
        ]),  // gids [0, 120, 240, 360, 480, 600, 720, 840]
    ];

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    let mut checked = 0usize;
    let mut faces_that_move = 0usize;
    for (name, want) in expected {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(*name))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let vars = face
            .variation_axes()
            .unwrap_or_else(|| panic!("{name}: opened but reports no axes"));
        assert_eq!(
            want.len(),
            vars.instances().len() + 1,
            "{name}: named instance count"
        );

        let n = face.num_glyphs();
        let step = usize::from((n / 8).max(1));
        let gids: Vec<u16> = (0..n).step_by(step).take(8).collect();

        let mut rows: Vec<Vec<u16>> = Vec::new();
        for (row, want_row) in want.iter().enumerate() {
            let coords = if row == 0 {
                vars.default_coords()
            } else {
                vars.instance_coords(row - 1)
                    .unwrap_or_else(|| panic!("{name}: instance {row} does not normalize"))
            };
            let got: Vec<u16> = gids
                .iter()
                .map(|&g| {
                    face.advance_at(g, &coords)
                        .unwrap_or_else(|e| panic!("{name}: glyph {g} advance: {e:?}"))
                })
                .collect();
            assert_eq!(got.as_slice(), *want_row, "{name}: instance row {row}");
            rows.push(got);
        }
        // A differential test needs a liveness assertion, or the null
        // implementation -- "the advance never varies" -- passes it.
        if rows[1..].iter().any(|r| *r != rows[0]) {
            faces_that_move += 1;
        }
        checked += 1;
    }

    assert_eq!(
        checked,
        expected.len(),
        "expected all {} faces with `HVAR` to be installed; found {checked}",
        expected.len()
    );
    assert!(
        faces_that_move >= 5,
        "only {faces_that_move} faces actually changed an advance; five of the \
         seven should (both Cascadias vary nothing)"
    );
    println!("{checked} faces read through HVAR, {faces_that_move} of them varying");
}

/// The face-wide metrics `MVAR` corrects, per named instance, against the same
/// independent reader.
///
/// `MVAR` is the third way into one `ItemVariationStore` and the only one whose
/// index is a *tag* rather than an ordinal, so this is where a mistake in the
/// sorted-record binary search would show. It is also the only one of the three
/// whose store hangs off an **Offset16** -- `HVAR` and `GDEF` both use an
/// Offset32 -- and a reader that used four bytes here would land in the record
/// array and compute an offset in the tens of thousands.
///
/// Sitka is the interesting face: it varies on optical size as well as weight,
/// and its x-height correction reaches -166 units at the small-text end. A
/// scalar product that smeared the two axes together would still produce
/// *something* on every row, which is why the expectations are per-instance
/// rather than a range.
///
/// Regenerate with:
///
/// ```text
/// python gui/font/tools/varstore_oracle.py --mvar
/// ```
#[test]
#[ignore = "depends on the host's installed fonts"]
fn installed_variable_fonts_vary_their_global_metrics() {
    /// (file name, the tags that face carries, one correction row per
    /// instance: the default first, then the named instances in `fvar` order).
    type Expectation<'a> = (&'a str, &'a [[u8; 4]], &'a [&'a [i16]]);

    let expected: &[Expectation<'_>] = &[
        ("ReemKufi.ttf", &[*b"xhgt", *b"cpht"], &[
            &[0, 0],  // default
            &[0, 0],  // 400
            &[6, 4],  // 500
            &[13, 7],  // 600
            &[19, 11],  // 700
        ]),
        ("SegUIVar.ttf", &[*b"xhgt"], &[
            &[0],  // default
            &[25],  // 300, 8
            &[25],  // 350, 8
            &[25],  // 400, 8
            &[25],  // 600, 8
            &[25],  // 700, 8
            &[0],  // 300, 10.5
            &[0],  // 350, 10.5
            &[0],  // 400, 10.5
            &[0],  // 600, 10.5
            &[0],  // 700, 10.5
            &[0],  // 300, 36
            &[0],  // 350, 36
            &[0],  // 400, 36
            &[0],  // 600, 36
            &[0],  // 700, 36
        ]),
        ("SitkaVF-Italic.ttf", &[*b"xhgt", *b"cpht", *b"undo", *b"unds"], &[
            &[0, 0, 0, 0],  // default
            &[0, 0, 0, 0],  // 6, 400
            &[0, 0, 0, 0],  // 6, 600
            &[0, 0, 0, 0],  // 6, 700
            &[0, 0, 0, 0],  // 11, 400
            &[-35, -30, 5, -2],  // 11, 600
            &[-58, -50, 8, -4],  // 11, 700
            &[-56, -37, 3, -1],  // 16.7391, 400
            &[-80, -57, 6, -3],  // 16.7391, 600
            &[-96, -69, 8, -4],  // 16.7391, 700
            &[-99, -65, 5, -2],  // 21.0434, 400
            &[-114, -76, 7, -3],  // 21.0434, 600
            &[-124, -84, 8, -4],  // 21.0434, 700
            &[-134, -88, 7, -3],  // 24.6303, 400
            &[-142, -93, 7, -4],  // 24.6303, 600
            &[-147, -96, 8, -4],  // 24.6303, 700
            &[-162, -106, 8, -4],  // 27.5, 400
            &[-164, -106, 8, -4],  // 27.5, 600
            &[-166, -106, 8, -4],  // 27.5, 700
        ]),
        ("SitkaVF.ttf", &[*b"xhgt", *b"cpht", *b"undo", *b"unds"], &[
            &[0, 0, 0, 0],  // default
            &[57, 50, -4, 2],  // 6, 400
            &[57, 50, 44, -29],  // 6, 600
            &[57, 50, 76, -50],  // 6, 700
            &[0, 0, 0, 0],  // 11, 400
            &[-1, 0, 48, -31],  // 11, 600
            &[-1, 0, 79, -52],  // 11, 700
            &[-37, -19, 0, 0],  // 16.7391, 400
            &[-38, -19, 48, -31],  // 16.7391, 600
            &[-38, -19, 79, -52],  // 16.7391, 700
            &[-65, -34, 0, 0],  // 21.0434, 400
            &[-65, -34, 48, -31],  // 21.0434, 600
            &[-66, -34, 79, -52],  // 21.0434, 700
            &[-88, -46, 0, 0],  // 24.6303, 400
            &[-88, -46, 48, -31],  // 24.6303, 600
            &[-89, -46, 79, -52],  // 24.6303, 700
            &[-107, -56, 0, 0],  // 27.5, 400
            &[-107, -56, 48, -31],  // 27.5, 600
            &[-107, -56, 79, -52],  // 27.5, 700
        ]),
    ];

    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, &mut files, 0);
    }
    files.sort();

    let mut checked = 0usize;
    let mut faces_that_move = 0usize;
    for (name, tags, want) in expected {
        let Some(path) = files
            .iter()
            .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(*name))
        else {
            continue;
        };
        let Ok(data) = fs::read(path) else { continue };
        let Ok(face) = Face::parse(data) else { continue };
        let vars = face
            .variation_axes()
            .unwrap_or_else(|| panic!("{name}: opened but reports no axes"));
        assert_eq!(
            want.len(),
            vars.instances().len() + 1,
            "{name}: named instance count"
        );

        let mut rows: Vec<Vec<i16>> = Vec::new();
        for (row, want_row) in want.iter().enumerate() {
            let coords = if row == 0 {
                vars.default_coords()
            } else {
                vars.instance_coords(row - 1)
                    .unwrap_or_else(|| panic!("{name}: instance {row} does not normalize"))
            };
            let got: Vec<i16> = tags.iter().map(|&t| face.metric_delta(t, &coords)).collect();
            assert_eq!(got.as_slice(), *want_row, "{name}: instance row {row}");
            rows.push(got);
        }
        // The default instance's metrics *are* the stored metrics, so row 0
        // must be all zeroes -- on every face, with no exceptions to carve out.
        assert!(
            rows[0].iter().all(|&d| d == 0),
            "{name}: the default instance corrects its own metrics"
        );
        if rows[1..].iter().any(|r| r.iter().any(|&d| d != 0)) {
            faces_that_move += 1;
        }
        checked += 1;
    }

    assert_eq!(
        checked,
        expected.len(),
        "expected all {} faces with a readable `MVAR` to be installed; found {checked}",
        expected.len()
    );
    assert!(
        faces_that_move >= 4,
        "only {faces_that_move} faces actually corrected a metric"
    );
    println!("{checked} faces read through MVAR, {faces_that_move} of them varying");
}
