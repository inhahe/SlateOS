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

use osfont::gsub::SubGlyph;
use osfont::raster::rasterize;
use osfont::scaled::{ScaledFont, Target};
use osfont::shape::TAB_WIDTH_IN_SPACES;
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

/// Run `face`'s `GSUB` over `gids` as one run, one source character each.
///
/// The face takes a whole run rather than answering about a position, because
/// its lookups apply in order across all of it; a helper is what keeps that
/// from being spelled out at each of the four call sites below.
fn substitute(face: &Face, gids: &[u16]) -> Vec<SubGlyph> {
    let mut glyphs: Vec<SubGlyph> = gids
        .iter()
        .enumerate()
        .map(|(i, &gid)| SubGlyph { gid, cluster: i })
        .collect();
    face.substitute(&mut glyphs);
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
            let out = substitute(&face, &pair);
            if out.len() != 1 {
                // Not every face ligates every pair, and one that leaves a
                // pair alone must leave it *whole*.
                assert_eq!(
                    out.iter().map(|g| g.gid).collect::<Vec<_>>(),
                    pair.to_vec(),
                    "{}: {name} came back as neither one glyph nor the two it \
                     went in as",
                    path.display()
                );
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
            substitute(&face, &[f]).len(),
            1,
            "{}: one glyph on its own produced a ligature",
            path.display()
        );
    }

    println!("faces opened:        {opened}");
    println!("faces with GSUB liga:{with_gsub}");
    println!("faces ligating fi:   {ligated_fi}");

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
        let out = substitute(&face, &[f, i]);
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
/// On the development host eight of 275 faces change it, and both causes are
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
        let after = substitute(&face, &before);
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
        let touched = substitute(&face, &[space])
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
        let (Some(e), Some(acute)) = (face.glyph_index('e'), face.glyph_index(ACUTE)) else {
            continue;
        };
        // A face can carry mark lookups that cover an entirely different
        // script: DejaVu Sans Mono's single `MarkBasePos` covers 15 Lao
        // glyphs and has never heard of `acutecomb`. That face has nothing
        // to say about a Latin accent, so it is skipped rather than failed —
        // the sweep's job is to check the faces that *do* answer.
        let Some((dx, dy)) = face.mark_on_base(e, acute) else {
            continue;
        };
        placed += 1;
        assert!(
            face.is_mark(acute),
            "{}: anchors an acute onto an 'e' yet does not class U+0301 as a \
             mark — the mark would be kerned and advanced like a letter",
            path.display()
        );
        assert!(
            !face.is_mark(e),
            "{}: 'e' is classed as a combining mark",
            path.display()
        );
        // A displacement is a placement within the glyph, so it is bounded by
        // the em; anything larger means a misread anchor and would put the
        // accent on a different letter.
        //
        // The *sign* is not an oracle here, unlike kerning's. A monospace face
        // draws its combining acute already at accent height inside its cell,
        // so its anchors coincide with the base's and the displacement is
        // legitimately (0, 0) — Cascadia Code is exactly that. What the mark
        // ends up over is checked below, on the run, where the glyph's own
        // extents are available to check it against.
        let em = i32::from(face.units_per_em());
        assert!(
            i32::from(dx).abs() <= em && i32::from(dy).abs() <= em * 2,
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
        "{with_marks} faces know about marks but not one places a combining \
         acute on an 'e' — the anchors are returning nothing"
    );

    // The oracle, driven all the way through shaping: the accent must be a
    // zero-width glyph displaced onto the letter before it.
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
        let run = font.shape(&format!("e{ACUTE}"));
        if run.len() != 2 {
            println!("oracle skip: {file} shaped e+acute to {} glyphs", run.len());
            continue;
        }
        let base = run.glyphs()[0];
        let mark = run.glyphs()[1];
        assert_eq!(mark.cluster, 1, "{file}: the acute starts at byte 1");

        // A combining mark takes no room: the pair measures as the bare `e`.
        assert!(
            (run.width() - base.advance).abs() < 0.5,
            "{file}: e+acute measures {:.3} px against the bare e's {:.3} px",
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
             {ink_left:.2}..{ink_right:.2} within the e's 0..{:.2}, top \
             {ink_top:.2}",
            mark.offset.0, mark.offset.1, base.advance
        );
        assert!(
            ink_right > 0.0 && ink_left < base.advance,
            "{file}: the acute's ink spans {ink_left:.2}..{ink_right:.2}, \
             which does not overlap the e's 0..{:.2} — it is being drawn \
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
