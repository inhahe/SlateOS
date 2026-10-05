// A test module's job is to fail loudly the instant the code under test is
// wrong, so the defensive lints that forbid exactly that in production code
// are off here -- as `CLAUDE.md` prescribes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::fs;

use scratchdir::ScratchDir;

use super::*;

// ---------------------------------------------------------------------------
// Building XCursor files
// ---------------------------------------------------------------------------

/// One image of a file under construction: `side` pixels square, every pixel
/// `pixel`.
#[derive(Clone, Copy)]
pub(crate) struct Img {
    nominal: u32,
    side: u32,
    hot: (u32, u32),
    delay: u32,
    pixel: u32,
}

/// An image of `nominal` drawn `nominal` square with every pixel `pixel`.
pub(crate) fn img(nominal: u32, pixel: u32) -> Img {
    Img {
        nominal,
        side: nominal,
        hot: (1, 2),
        delay: 50,
        pixel,
    }
}

fn le(out: &mut Vec<u8>, words: &[u32]) {
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
}

/// An XCursor file: a comment, then `images` in this order -- each its own
/// chunk, named by its own table entry.
pub(crate) fn xcursor(images: &[Img]) -> Vec<u8> {
    const COMMENT: &[u8] = b"made for a test";
    let count = u32::try_from(images.len() + 1).unwrap();
    let table_end = 16 + 12 * count;
    let mut out = Vec::new();
    le(
        &mut out,
        &[u32::from_le_bytes(*b"Xcur"), 16, 0x0001_0000, count],
    );
    // Where each chunk will start: the comment first.
    let mut at = table_end;
    let comment_at = at;
    at += 20 + u32::try_from(COMMENT.len()).unwrap();
    let mut positions = Vec::new();
    for image in images {
        positions.push(at);
        at += 36 + 4 * image.side * image.side;
    }
    le(&mut out, &[0xfffe_0001, 1, comment_at]);
    for (image, position) in images.iter().zip(&positions) {
        le(&mut out, &[0xfffd_0002, image.nominal, *position]);
    }
    le(
        &mut out,
        &[20, 0xfffe_0001, 1, 1, u32::try_from(COMMENT.len()).unwrap()],
    );
    out.extend_from_slice(COMMENT);
    for image in images {
        le(
            &mut out,
            &[
                36,
                0xfffd_0002,
                image.nominal,
                1,
                image.side,
                image.side,
                image.hot.0,
                image.hot.1,
                image.delay,
            ],
        );
        for _ in 0..image.side * image.side {
            le(&mut out, &[image.pixel]);
        }
    }
    out
}

/// Overwrite the word at byte `at` of `file`.
fn poke(file: &mut [u8], at: usize, word: u32) {
    file[at..at + 4].copy_from_slice(&word.to_le_bytes());
}

/// The byte where image `i`'s chunk starts in a file built by [`xcursor`].
fn chunk_at(file: &[u8], i: usize) -> usize {
    // The table: the comment's entry, then one per image.
    let entry = 16 + 12 * (i + 1);
    u32::from_le_bytes(file[entry + 8..entry + 12].try_into().unwrap()) as usize
}

const OPAQUE_RED: u32 = 0xFFFF_0000;

// ---------------------------------------------------------------------------
// Reading one
// ---------------------------------------------------------------------------

/// **The nearest nominal size is taken, the first of the nearest on a tie**,
/// as libXcursor takes it -- and only that size's pictures.
#[test]
fn the_nearest_size_is_read() {
    let file = xcursor(&[img(24, 0xFF00_0001), img(32, 0xFF00_0002)]);
    let nominal = |size| xcursor::read(&file, size).unwrap().nominal;
    assert_eq!(nominal(24), 24);
    assert_eq!(nominal(1), 24);
    assert_eq!(nominal(30), 32);
    assert_eq!(nominal(500), 32);
    // 28 is as near 24 as 32: the first in the table.
    assert_eq!(nominal(28), 24);
    let first_last = xcursor(&[img(32, 1), img(24, 2)]);
    assert_eq!(xcursor::read(&first_last, 28).unwrap().nominal, 32);
    // Each size's own picture.
    let at = |size: u32| xcursor::read(&file, size).unwrap().frames[0].pixels[0];
    assert_eq!(at(24), 0xFF00_0001);
    assert_eq!(at(32), 0xFF00_0002);
}

/// **A picture is read whole**: its sides, its hot spot, its delay and every
/// pixel, from the right place.
#[test]
fn a_picture_is_read_whole() {
    let file = xcursor(&[Img {
        nominal: 24,
        side: 3,
        hot: (2, 3),
        delay: 75,
        pixel: OPAQUE_RED,
    }]);
    let images = xcursor::read(&file, 24).unwrap();
    assert_eq!(images.nominal, 24);
    assert_eq!(
        images.frames,
        [CursorFrame {
            width: 3,
            height: 3,
            hot_x: 2,
            hot_y: 3,
            delay_ms: 75,
            pixels: vec![OPAQUE_RED; 9],
        }]
    );
}

/// **Every picture of the size is a frame, in the file's order**, with its
/// own delay: an animated cursor.
#[test]
fn every_picture_of_the_size_is_a_frame_in_order() {
    let frame = |pixel, delay| Img {
        delay,
        ..img(24, pixel)
    };
    let file = xcursor(&[
        frame(0xFF00_0001, 16),
        img(32, 0xFF00_00AA),
        frame(0xFF00_0002, 17),
        frame(0xFF00_0003, 18),
    ]);
    let images = xcursor::read(&file, 24).unwrap();
    let shown: Vec<(u32, u32)> = images
        .frames
        .iter()
        .map(|f| (f.pixels[0], f.delay_ms))
        .collect();
    assert_eq!(
        shown,
        [(0xFF00_0001, 16), (0xFF00_0002, 17), (0xFF00_0003, 18)]
    );
}

/// **A colour brighter than its alpha is held to it**: premultiplied colour
/// cannot be brighter, and a blend that trusted it would overflow.
#[test]
fn a_colour_brighter_than_its_alpha_is_held_to_it() {
    let file = xcursor(&[img(24, 0x80FF_4020)]);
    let pixel = xcursor::read(&file, 24).unwrap().frames[0].pixels[0];
    assert_eq!(pixel, 0x8080_4020);
    // Within its alpha, a colour is left as it is.
    let file = xcursor(&[img(24, 0x8070_6050)]);
    assert_eq!(
        xcursor::read(&file, 24).unwrap().frames[0].pixels[0],
        0x8070_6050
    );
}

/// **A file that is not an XCursor file, or lies about itself, is no
/// cursor** -- each way a theme's file can be wrong.
#[test]
fn a_bad_file_is_no_cursor() {
    let good = xcursor(&[img(24, OPAQUE_RED)]);
    assert!(xcursor::read(&good, 24).is_some());
    let image = chunk_at(&good, 0);
    let bad = |at: usize, word: u32| {
        let mut file = good.clone();
        poke(&mut file, at, word);
        xcursor::read(&file, 24)
    };
    // Not "Xcur".
    assert_eq!(bad(0, u32::from_le_bytes(*b"Xcux")), None);
    // A header shorter than a header.
    assert_eq!(bad(4, 12), None);
    // A table longer than the file, or than any file's.
    assert_eq!(bad(12, 1000), None);
    assert_eq!(bad(12, 5000), None);
    // A position past the end.
    assert_eq!(bad(16 + 12 + 8, u32::MAX - 2), None);
    // A chunk that is not what the table says: another type, another size.
    assert_eq!(bad(image + 4, 0xfffe_0001), None);
    assert_eq!(bad(image + 8, 32), None);
    // No sides, or sides past the bound.
    assert_eq!(bad(image + 16, 0), None);
    assert_eq!(bad(image + 20, 0), None);
    assert_eq!(bad(image + 16, xcursor::MAX_SIDE + 1), None);
    // A hot spot outside the picture.
    assert_eq!(bad(image + 24, 25), None);
    assert_eq!(bad(image + 28, 25), None);
    // Pixels the file does not hold.
    assert_eq!(xcursor::read(&good[..good.len() - 1], 24), None);
    // A header that says it ends inside itself, though the table it then
    // describes -- starting at byte 4, three long -- reaches the real image
    // entry at byte 28 as its third.
    let mut short = good.clone();
    poke(&mut short, 4, 4);
    poke(&mut short, 12, 3);
    assert_eq!(xcursor::read(&short, 24), None);
    // Nothing but bytes.
    assert_eq!(xcursor::read(b"Xcu", 24), None);
    assert_eq!(xcursor::read(&[], 24), None);
    // A file whose table names no picture at all.
    let mut empty = Vec::new();
    le(
        &mut empty,
        &[u32::from_le_bytes(*b"Xcur"), 16, 0x0001_0000, 0],
    );
    assert_eq!(xcursor::read(&empty, 24), None);
}

/// **A table past 4096 entries is refused** even when every entry is good:
/// the bound is on the scan, not only on files too short to hold one.
#[test]
fn a_table_past_the_bound_is_refused() {
    let file = |comments: u32| {
        let count = comments + 1;
        let table_end = 16 + 12 * count;
        let mut out = Vec::new();
        le(
            &mut out,
            &[u32::from_le_bytes(*b"Xcur"), 16, 0x0001_0000, count],
        );
        // Every comment entry names the same small comment, after the image.
        let image_at = table_end;
        let comment_at = image_at + 36 + 4;
        for _ in 0..comments {
            le(&mut out, &[0xfffe_0001, 1, comment_at]);
        }
        le(&mut out, &[0xfffd_0002, 1, image_at]);
        le(
            &mut out,
            &[36, 0xfffd_0002, 1, 1, 1, 1, 0, 0, 0, OPAQUE_RED],
        );
        le(&mut out, &[20, 0xfffe_0001, 1, 1, 0]);
        out
    };
    assert!(xcursor::read(&file(4095), 1).is_some());
    assert_eq!(xcursor::read(&file(4096), 1), None);
}

/// **A picture has sides, and none past 1024** -- each refused by that rule
/// alone: an empty picture whose hot spot is at its corner, and a good
/// 1025-pixel strip whose pixels are all there.
#[test]
fn a_picture_needs_sides_within_the_bound() {
    let strip = |width: u32, height: u32| {
        let mut out = Vec::new();
        le(
            &mut out,
            &[u32::from_le_bytes(*b"Xcur"), 16, 0x0001_0000, 1],
        );
        le(&mut out, &[0xfffd_0002, 24, 28]);
        le(&mut out, &[36, 0xfffd_0002, 24, 1, width, height, 0, 0, 0]);
        for _ in 0..width * height {
            le(&mut out, &[OPAQUE_RED]);
        }
        out
    };
    assert_eq!(xcursor::read(&strip(0, 24), 24), None);
    assert_eq!(xcursor::read(&strip(24, 0), 24), None);
    let side = xcursor::MAX_SIDE;
    assert!(xcursor::read(&strip(side, 1), 24).is_some());
    assert_eq!(xcursor::read(&strip(side + 1, 1), 24), None);
    assert_eq!(xcursor::read(&strip(1, side + 1), 24), None);
}

/// **A hot spot on the far edge is allowed**, as libXcursor allows it.
#[test]
fn a_hot_spot_on_the_edge_is_allowed() {
    let file = xcursor(&[Img {
        hot: (24, 24),
        ..img(24, OPAQUE_RED)
    }]);
    assert!(xcursor::read(&file, 24).is_some());
}

/// **Table entries that name one picture many times cannot multiply it past
/// the bound**: sixteen of a 1024-pixel picture is the budget exactly, and a
/// seventeenth refuses the cursor rather than decoding gigabytes.
#[test]
fn naming_one_picture_again_and_again_stops_at_the_bound() {
    let side = xcursor::MAX_SIDE;
    let one = xcursor(&[Img {
        nominal: side,
        side,
        hot: (0, 0),
        delay: 0,
        pixel: 0,
    }]);
    let chunk = u32::try_from(chunk_at(&one, 0)).unwrap();
    let file = |copies: u32| {
        // The same chunk, `copies` times over, from a table of its own.
        let mut out = Vec::new();
        le(
            &mut out,
            &[u32::from_le_bytes(*b"Xcur"), 16, 0x0001_0000, copies],
        );
        let table_end = 16 + 12 * copies;
        for _ in 0..copies {
            le(&mut out, &[0xfffd_0002, side, table_end]);
        }
        out.extend_from_slice(&one[chunk as usize..]);
        out
    };
    let pixels = (side * side) as usize;
    assert_eq!(xcursor::MAX_DECODED_PIXELS / pixels, 16);
    assert_eq!(xcursor::read(&file(16), side).unwrap().frames.len(), 16);
    assert_eq!(xcursor::read(&file(17), side), None);
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// **Every cursor the compositor draws has its older names**, and a name is
/// never a path.
#[test]
fn names_and_their_older_spellings() {
    for css in [
        "default",
        "text",
        "pointer",
        "ns-resize",
        "ew-resize",
        "nesw-resize",
        "nwse-resize",
        "move",
        "wait",
        "help",
        "crosshair",
        "not-allowed",
    ] {
        assert!(!aliases(css).is_empty(), "{css} has no older name");
    }
    assert_eq!(aliases("pointer")[0], "hand2");
    // X11's diagonals: `fd` is the one that leans like `/`.
    assert_eq!(aliases("nesw-resize")[0], "fd_double_arrow");
    assert_eq!(aliases("nwse-resize")[0], "bd_double_arrow");
    assert!(aliases("no-such-cursor").is_empty());
    for name in [
        "default",
        "left_ptr",
        "X_cursor",
        "9d800788f1b08800ae810202380a0822",
    ] {
        assert!(is_valid_name(name), "{name}");
    }
    for name in [
        "",
        "..",
        "a/b",
        "a\\b",
        "dot.ted",
        "space d",
        &"x".repeat(65),
    ] {
        assert!(!is_valid_name(name), "{name:?}");
    }
}

// ---------------------------------------------------------------------------
// Themes
// ---------------------------------------------------------------------------

/// SlateOS theme roots and cursor-theme roots inside a scratch directory.
struct Fixture {
    scratch: ScratchDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-cursors-{tag}")),
        }
    }

    fn root(&self, root: &str) -> PathBuf {
        self.scratch.dir().join(root)
    }

    /// The theme `id`, read from this fixture's roots: `user` and `system`
    /// for SlateOS themes, then `icons` and `share` for other desktops'.
    fn theme(&self, id: &str) -> CursorTheme {
        CursorTheme::named(
            OsStr::new(id),
            ThemeDirs {
                user: Some(self.root("user")),
                system: self.root("system"),
            },
            vec![self.root("icons"), self.root("share")],
        )
    }

    /// Put the cursor `name` of `theme` under `root`: one opaque picture,
    /// its colour `mark` -- which [`colour`] reads back.
    fn put(&self, root: &str, theme: &str, name: &str, mark: u32) {
        let dir = self.root(root).join(theme).join(CURSORS_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(name),
            xcursor(&[img(24, 0xFF00_0000 | (mark & 0x00FF_FFFF))]),
        )
        .unwrap();
    }

    /// Give `theme` under `root` an `index.theme` of `body`.
    fn index(&self, root: &str, theme: &str, body: &str) {
        let dir = self.root(root).join(theme);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(INDEX_FILE), body).unwrap();
    }
}

/// The colour -- the mark [`Fixture::put`] gave it -- that `theme` draws the
/// cursor `name` in.
fn colour(theme: &CursorTheme, name: &str) -> Option<u32> {
    theme
        .cursor(name, 24)
        .map(|images| images.frames[0].pixels[0] & 0x00FF_FFFF)
}

/// **A theme's cursor is its file of that name**, the user's copy before the
/// system's, and SlateOS's roots before other desktops'.
#[test]
fn a_cursor_is_found_in_the_first_root_that_has_it() {
    let f = Fixture::new("roots");
    f.put("share", "Breeze", "default", 1);
    assert_eq!(colour(&f.theme("Breeze"), "default"), Some(1));
    f.put("icons", "Breeze", "default", 2);
    assert_eq!(colour(&f.theme("Breeze"), "default"), Some(2));
    f.put("system", "Breeze", "default", 3);
    assert_eq!(colour(&f.theme("Breeze"), "default"), Some(3));
    f.put("user", "Breeze", "default", 4);
    assert_eq!(colour(&f.theme("Breeze"), "default"), Some(4));
    // Another theme, another cursor, nothing.
    assert_eq!(colour(&f.theme("Adwaita"), "default"), None);
    assert_eq!(colour(&f.theme("Breeze"), "text"), None);
}

/// **A cursor is found under its older names** when the theme has no file of
/// its CSS name -- and the CSS name wins where it has both.
#[test]
fn a_cursor_is_found_under_its_older_names() {
    let f = Fixture::new("aliases");
    f.put("share", "Old", "left_ptr", 5);
    f.put("share", "Old", "hand2", 6);
    let old = f.theme("Old");
    assert_eq!(colour(&old, "default"), Some(5));
    assert_eq!(colour(&old, "pointer"), Some(6));
    // An older name is only ever tried for the name it belongs to.
    assert_eq!(colour(&old, "text"), None);
    f.put("share", "Old", "default", 7);
    assert_eq!(colour(&old, "default"), Some(7));
}

/// **A theme without a cursor reads the themes it inherits from, in order**,
/// after every name of its own -- so its own look stays together.
#[test]
fn a_cursor_is_found_in_the_themes_inherited_from() {
    let f = Fixture::new("inherit");
    f.index(
        "share",
        "Mine",
        "[Icon Theme]\nName=Mine\nInherits=Second, First\n",
    );
    f.put("share", "Second", "text", 8);
    f.put("share", "First", "text", 9);
    f.put("share", "First", "pointer", 10);
    let mine = f.theme("Mine");
    assert_eq!(colour(&mine, "text"), Some(8));
    assert_eq!(colour(&mine, "pointer"), Some(10));
    // Its own older name before another theme's CSS one.
    f.put("share", "Mine", "xterm", 11);
    assert_eq!(colour(&mine, "text"), Some(11));
    // Semicolons separate as well as commas.
    f.index("share", "Semi", "[Icon Theme]\nInherits=First;Second\n");
    assert_eq!(colour(&f.theme("Semi"), "text"), Some(9));
}

/// **Inheritance cannot loop**: a theme inheriting from itself, or two from
/// each other, is a cursor not found -- not a hang.
#[test]
fn inheritance_that_loops_ends() {
    let f = Fixture::new("loops");
    f.index("share", "A", "[Icon Theme]\nInherits=B\n");
    f.index("share", "B", "[Icon Theme]\nInherits=A,B\n");
    f.index("share", "Self", "[Icon Theme]\nInherits=Self\n");
    assert_eq!(colour(&f.theme("A"), "default"), None);
    assert_eq!(colour(&f.theme("Self"), "default"), None);
    // A long chain ends at the bound rather than wherever it ends.
    for i in 0..40 {
        f.index(
            "share",
            &format!("t{i}"),
            &format!("[Icon Theme]\nInherits=t{}\n", i + 1),
        );
    }
    f.put("share", "t40", "default", 12);
    assert_eq!(colour(&f.theme("t0"), "default"), None);
    assert_eq!(colour(&f.theme("t30"), "default"), Some(12));
    // A loop in one parent does not use up the search before the next
    // parent is reached: each theme is visited once.
    f.index("share", "Mine", "[Icon Theme]\nInherits=A,Real\n");
    f.put("share", "Real", "default", 15);
    assert_eq!(colour(&f.theme("Mine"), "default"), Some(15));
}

/// **A file that is not a cursor is passed over** for the next place, and a
/// name or theme that is not one is never looked up.
#[test]
fn what_is_not_a_cursor_is_passed_over() {
    let f = Fixture::new("passed");
    f.put("share", "T", "left_ptr", 13);
    let dir = f.root("user").join("T").join(CURSORS_DIR);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("default"), b"not a cursor").unwrap();
    assert_eq!(colour(&f.theme("T"), "default"), Some(13));
    // A file past the size bound is not read at all -- though what it starts
    // with is a good cursor, which would be read if it were.
    f.put("user", "T", "left_ptr", 16);
    assert_eq!(colour(&f.theme("T"), "default"), Some(16));
    let big = fs::OpenOptions::new()
        .write(true)
        .open(dir.join("left_ptr"))
        .unwrap();
    big.set_len(MAX_CURSOR_BYTES + 1).unwrap();
    drop(big);
    assert_eq!(colour(&f.theme("T"), "default"), Some(13));
    // Names and themes that are not names, though each would reach a file
    // if it were joined to a path: `T`'s own `left_ptr` -- `share`'s copy,
    // `cursors/../cursors/left_ptr` -- and a cursor put where a theme named
    // `..` would find it.
    assert_eq!(colour(&f.theme("T"), "left_ptr"), Some(13));
    assert_eq!(colour(&f.theme("T"), "../cursors/left_ptr"), None);
    let above = f.scratch.dir().join(CURSORS_DIR);
    fs::create_dir_all(&above).unwrap();
    fs::write(above.join("default"), xcursor(&[img(24, 0xFF00_0011)])).unwrap();
    assert_eq!(colour(&f.theme(".."), "default"), None);
    assert_eq!(f.theme("T").cursor("default", 0), None);
}

/// **With no theme drawing a cursor, there is no picture** -- the built-in
/// theme has none, so the compositor draws its own.
#[test]
fn the_built_in_theme_draws_nothing_of_its_own() {
    let f = Fixture::new("builtin");
    let theme = f.theme(themes::BUILT_IN);
    assert!(theme.is_built_in());
    assert!(CursorTheme::default().is_built_in());
    assert_eq!(colour(&theme, "default"), None);
    // Given cursors, it reads them.
    f.put("system", themes::BUILT_IN, "default", 14);
    assert_eq!(colour(&theme, "default"), Some(14));
}

/// **The installed themes, for a picker**: the built-in one first, then every
/// folder with cursors by the name it gives itself, a theme in two roots once.
#[test]
fn the_installed_themes_are_listed() {
    let f = Fixture::new("listed");
    f.put("share", "breeze_cursors", "default", 1);
    f.index("share", "breeze_cursors", "[Icon Theme]\nName=Breeze\n");
    f.put("share", "Adwaita", "default", 1);
    f.put("icons", "Adwaita", "default", 1);
    f.index("icons", "Adwaita", "[Icon Theme]\nName=Adwaita (mine)\n");
    // An icon theme with no cursors is not a cursor theme.
    f.index("share", "hicolor", "[Icon Theme]\nName=Hicolor\n");
    // Sorted by the name shown, not the folder's: `aaa` is called Zulu.
    f.put("share", "aaa", "default", 1);
    f.index("share", "aaa", "[Icon Theme]\nName=Zulu\n");
    // The built-in theme given cursors is still the built-in one, once.
    f.put("system", themes::BUILT_IN, "default", 1);
    let roots = f.theme("x").roots();
    let listed: Vec<(String, bool)> = available_in(&roots)
        .into_iter()
        .map(|info| (info.name, info.built_in))
        .collect();
    assert_eq!(
        listed,
        [
            (themes::BUILT_IN_NAME.to_owned(), true),
            ("Adwaita (mine)".to_owned(), false),
            ("Breeze".to_owned(), false),
            ("Zulu".to_owned(), false),
        ]
    );
    let breeze = available_in(&roots).remove(2);
    assert_eq!(breeze.id, "breeze_cursors");
}

/// **Other desktops' roots are where the environment says**: the user's own
/// data directory and `~/.icons` before the system's.
#[test]
fn the_cursor_theme_roots_follow_the_environment() {
    // An absolute path on the host the tests run on.
    let abs = |tail: &str| {
        if cfg!(windows) {
            PathBuf::from(format!("C:\\{tail}"))
        } else {
            PathBuf::from(format!("/{tail}"))
        }
    };
    let env = |pairs: Vec<(&'static str, OsString)>| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
        }
    };
    let home = abs("home");
    assert_eq!(
        icon_dirs_from(env(vec![("HOME", home.clone().into_os_string())])),
        [
            home.join(".local").join("share").join("icons"),
            home.join(".icons"),
            PathBuf::from("/usr/local/share").join("icons"),
            PathBuf::from("/usr/share").join("icons"),
        ]
    );
    let list = std::env::join_paths([abs("a"), abs("b")]).unwrap();
    assert_eq!(
        icon_dirs_from(env(vec![
            ("HOME", home.clone().into_os_string()),
            ("XDG_DATA_HOME", abs("data").into_os_string()),
            ("XDG_DATA_DIRS", list),
        ])),
        [
            abs("data").join("icons"),
            home.join(".icons"),
            abs("a").join("icons"),
            abs("b").join("icons"),
        ]
    );
    // No user at all: the system's alone.
    assert_eq!(
        icon_dirs_from(env(vec![])),
        [
            PathBuf::from("/usr/local/share").join("icons"),
            PathBuf::from("/usr/share").join("icons"),
        ]
    );
    // A relative home is no home.
    assert_eq!(
        icon_dirs_from(env(vec![("HOME", OsString::from("relative"))])).len(),
        3
    );
}
