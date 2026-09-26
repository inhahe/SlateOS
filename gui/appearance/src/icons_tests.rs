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

/// A user and a system theme directory inside a scratch directory, and a way
/// to put an icon file in either.
struct Fixture {
    scratch: ScratchDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-icons-{tag}")),
        }
    }

    fn dirs(&self) -> ThemeDirs {
        ThemeDirs {
            user: Some(self.scratch.dir().join("user")),
            system: self.scratch.dir().join("system"),
        }
    }

    fn put(&self, root: &str, theme: &str, name: &str, bytes: &[u8]) {
        let dir = self.scratch.dir().join(root).join(theme).join(ICONS_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.svg")), bytes).unwrap();
    }
}

/// A square filling its box, in the colour the icon is drawn in.
const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="0" y="0" width="10" height="10" fill="currentColor"/></svg>"#;
/// A disc in a fixed red of its own.
const RED_DOT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#ff0000"/></svg>"##;

const INK: Color = Color::rgb(0x20, 0x80, 0xc0);

/// Every built-in icon parses, draws something legible at the sizes the
/// desktop uses, and draws it in the colour asked for -- they are all
/// `currentColor`.
///
/// Legible is a pixel at least three-quarters covered. Not a wholly solid one:
/// a line two units wide in a 24-unit icon is a pixel and a third wide at 16
/// pixels, and whether any pixel lies wholly inside it depends on where it
/// falls. This asked for a solid pixel while strokes were drawn two pixels wide
/// at every size, and a curved icon drawn to scale at 16 has none. An icon
/// with nothing even three-quarters covered is a smear.
#[test]
fn every_built_in_icon_draws_in_the_colour_it_is_asked_for() {
    let theme = IconTheme::named(OsStr::new("no-such-theme"), Fixture::new("builtin").dirs());
    for name in built_in_names() {
        for size in [16, 24, 48] {
            let icon = theme
                .render(name, size, INK)
                .unwrap_or_else(|| panic!("{name} at {size} draws nothing"));
            assert_eq!(icon.size, size);
            assert_eq!(icon.argb.len(), (size * size) as usize);
            let strong: Vec<u32> = icon
                .argb
                .iter()
                .copied()
                .filter(|px| px >> 24 >= 0xC0)
                .collect();
            assert!(
                !strong.is_empty(),
                "{name} at {size} has no pixel three-quarters covered"
            );
            for px in strong {
                assert_eq!(
                    px & 0x00FF_FFFF,
                    0x0020_80C0,
                    "{name} draws a colour it was not given"
                );
            }
            // An outline icon leaves its corners and most of its box clear.
            assert_eq!(icon.argb[0] >> 24, 0, "{name} fills its corner");
        }
    }
}

/// The renderer's byte order, pinned: a red disc comes out red, not blue.
/// `SvgDocument::render`'s pixels are `[r, g, b, a]`, whatever its field
/// comment says, and an icon that swapped red for blue would say so here.
#[test]
fn an_icons_own_colours_are_kept_and_come_out_the_right_way_round() {
    let icon = render_svg(RED_DOT, 20, INK).expect("draws");
    let centre = icon.argb[(10 * 20 + 10) as usize];
    assert_eq!(centre, 0xFFFF_0000, "{centre:08x}");
}

/// `currentColor` becomes the colour asked for, and a square that fills its
/// box fills it with that colour, edge to edge.
#[test]
fn current_color_is_the_colour_asked_for() {
    let icon = render_svg(SQUARE, 8, INK).expect("draws");
    assert!(
        icon.argb.iter().all(|px| *px == 0xFF20_80C0),
        "{:08x?}",
        &icon.argb[..4]
    );
}

/// The user's copy of a theme's icon is drawn before the system's, and the
/// system's before the built-in one.
#[test]
fn the_users_icon_comes_before_the_systems_and_both_before_the_built_in() {
    let fx = Fixture::new("order");
    let theme = IconTheme::named(OsStr::new("mine"), fx.dirs());
    let built_in = theme.source("folder").expect("built in");
    assert!(matches!(built_in, Cow::Borrowed(_)));

    fx.put("system", "mine", "folder", RED_DOT.as_bytes());
    assert_eq!(theme.source("folder").unwrap(), RED_DOT);
    fx.put("user", "mine", "folder", SQUARE.as_bytes());
    assert_eq!(theme.source("folder").unwrap(), SQUARE);
}

/// **An icon written as Breeze and Inkscape write them draws in the colour
/// asked for**: its paint in a `style` attribute, a stylesheet in `<defs>`
/// that sets the colour `currentColor` means. The renderer read presentation
/// attributes only, so such an icon drew as a solid black square -- the
/// default fill -- and a set drawn for another desktop did not drop in.
#[test]
fn an_icon_styled_as_breeze_writes_them_draws_in_the_colour_asked_for() {
    const BREEZE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <defs id="defs3051">
    <style type="text/css" id="current-color-scheme">
      .ColorScheme-Text {
        color:#232629;
      }
    </style>
  </defs>
  <path style="fill:currentColor;fill-opacity:1;stroke:none" d="M2 2h12v12H2z" class="ColorScheme-Text"/>
</svg>"#;
    let fx = Fixture::new("breeze");
    fx.put("system", "breeze", "folder", BREEZE.as_bytes());
    let theme = IconTheme::named(OsStr::new("breeze"), fx.dirs());
    let icon = theme.render("folder", 16, INK).expect("draws");
    // The middle of the square, in the colour asked for.
    assert_eq!(icon.argb[(8 * 16 + 8) as usize], 0xFF20_80C0);
    // And its corner clear: the square is inset by two units.
    assert_eq!(icon.argb[0] >> 24, 0);
}

/// **An icon named in a frame is uploaded once, before it**: each icon id the
/// commands name is drawn and handed to the upload once, a picture's own id is
/// left alone, and an id nobody asked for, or one nothing draws, is passed
/// over -- and all three recorded, so none is looked at again.
#[test]
fn a_frames_icons_are_uploaded_once_and_nothing_else_is() {
    use guitk::render::RenderCommand;
    let registry = IconRegistry::default();
    let folder = registry.icon("folder", 16, INK);
    let nothing = registry.icon("no-such-icon-anywhere", 16, INK);
    assert_ne!(folder & ICON_ID_TAG, 0, "an icon's id carries the tag");
    assert_eq!(
        registry.icon("folder", 16, INK),
        folder,
        "the same icon, the same id"
    );
    assert_ne!(
        registry.icon("folder", 24, INK),
        folder,
        "another size, another id"
    );
    let unknown = ICON_ID_TAG | 7;
    let picture = 42_u64;
    let image = |image_id: u64| RenderCommand::Image {
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 16.0,
        image_id,
    };
    let frame = [
        image(folder),
        image(picture),
        image(nothing),
        image(unknown),
        image(folder),
    ];
    let theme = IconTheme::named(OsStr::new("no-such-theme"), Fixture::new("upload").dirs());
    let mut sent = std::collections::BTreeSet::new();
    let mut uploaded: Vec<(u64, u32)> = Vec::new();
    let result: Result<(), ()> = upload_missing(
        &frame,
        &theme,
        |id| registry.request(id),
        |id| sent.insert(id),
        |id, icon| {
            uploaded.push((id, icon.size));
            Ok(())
        },
    );
    assert!(result.is_ok());
    assert_eq!(uploaded, [(folder, 16)], "only the folder, once");
    assert!(sent.contains(&nothing) && sent.contains(&unknown));
    assert!(
        !sent.contains(&picture),
        "a picture's own id is not an icon's"
    );

    // The next frame naming it sends nothing again.
    uploaded.clear();
    let again: Result<(), ()> = upload_missing(
        &frame,
        &theme,
        |id| registry.request(id),
        |id| sent.insert(id),
        |id, icon| {
            uploaded.push((id, icon.size));
            Ok(())
        },
    );
    assert!(again.is_ok());
    assert!(uploaded.is_empty());

    // An upload's error stops the walk and comes back.
    let failing: Result<(), &str> = upload_missing(
        &[image(registry.icon("folder", 32, INK))],
        &theme,
        |id| registry.request(id),
        |_| true,
        |_, _| Err("refused"),
    );
    assert_eq!(failing, Err("refused"));

    // Clearing forgets what was drawn.
    registry.clear();
    assert!(registry.request(folder).is_none());
}

/// A name the theme lacks falls back to shorter names -- in the theme first,
/// then built in -- as the naming specification says.
#[test]
fn a_missing_name_falls_back_to_a_shorter_one() {
    let fx = Fixture::new("fallback");
    let theme = IconTheme::named(OsStr::new("mine"), fx.dirs());
    // Built in: folder-documents is its own picture; folder-documents-work is
    // not drawn anywhere and falls back to it.
    assert_eq!(
        theme.source("folder-documents-work"),
        theme.source("folder-documents")
    );
    assert_ne!(theme.source("folder-documents"), theme.source("folder"));
    // A theme that draws only `folder` gives that for every folder, before any
    // built-in picture: the theme's own look is kept together.
    fx.put("user", "mine", "folder", SQUARE.as_bytes());
    assert_eq!(theme.source("folder-documents").unwrap(), SQUARE);
    assert_eq!(theme.source("no-such-icon"), None);
}

/// Something that is not a name is not looked for anywhere: it would be a
/// path out of the theme's folder.
#[test]
fn a_path_is_not_a_name() {
    let fx = Fixture::new("names");
    let theme = IconTheme::named(OsStr::new("mine"), fx.dirs());
    for bad in [
        "",
        "../folder",
        "a/b",
        "folder.svg",
        "Folder",
        "-folder",
        "fol der",
    ] {
        assert!(!is_valid_name(bad), "{bad:?}");
        assert_eq!(theme.source(bad), None, "{bad:?}");
    }
    assert!(is_valid_name("folder-documents"));
    assert!(is_valid_name("x_2"));
    // Nor is a theme id with a path in it looked in.
    fx.put("user", "mine", "folder", SQUARE.as_bytes());
    let escaping = IconTheme::named(OsStr::new("../user/mine"), fx.dirs());
    assert!(matches!(escaping.source("folder"), Some(Cow::Borrowed(_))));
}

/// A theme's file that is too big, not an SVG, or not text is passed over
/// for the next place to look -- here, the built-in icon.
#[test]
fn a_file_that_is_not_an_icon_is_passed_over() {
    let fx = Fixture::new("bad-files");
    let theme = IconTheme::named(OsStr::new("mine"), fx.dirs());
    let built_in = theme.source("folder").unwrap();

    fx.put("user", "mine", "folder", b"this is not an svg");
    assert_eq!(theme.source("folder"), Some(built_in.clone()));

    fx.put("user", "mine", "folder", &[0xff, 0xfe, 0x00, 0x80]);
    assert_eq!(theme.source("folder"), Some(built_in.clone()));

    let mut huge = SQUARE.as_bytes().to_vec();
    huge.resize(usize::try_from(MAX_ICON_BYTES).unwrap() + 1, b' ');
    fx.put("user", "mine", "folder", &huge);
    assert_eq!(theme.source("folder"), Some(built_in));
}

/// An SVG that draws nothing is not an icon, and a size of nothing draws
/// nothing; a size past the cap is drawn at the cap.
#[test]
fn nothing_drawn_is_no_icon_and_sizes_are_capped() {
    let empty = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"></svg>"#;
    assert_eq!(render_svg(empty, 16, INK), None);
    let theme = IconTheme::named(OsStr::new("x"), Fixture::new("sizes").dirs());
    assert_eq!(theme.render("folder", 0, INK), None);
    let big = theme
        .render("folder", 100_000, INK)
        .expect("drawn at the cap");
    assert_eq!(big.size, MAX_ICON_PX);
}

/// The places the start menu lists each have a picture of their own, and the
/// places are drawn as what they hold.
#[test]
fn the_start_menus_places_each_have_an_icon() {
    let theme = IconTheme::named(OsStr::new("x"), Fixture::new("places").dirs());
    let places = [
        "user-home",
        "folder-documents",
        "folder-pictures",
        "folder-music",
        "folder-download",
        "preferences-system",
        "utilities-terminal",
        "system-shutdown",
    ];
    let sources: Vec<_> = places.iter().map(|p| theme.source(p).expect(p)).collect();
    for (i, a) in sources.iter().enumerate() {
        for (j, b) in sources.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "{} and {} are the same picture", places[i], places[j]);
            }
        }
    }
    assert_eq!(
        theme.source("folder-documents"),
        theme.source("text-x-generic")
    );
}

/// A translucent colour draws a translucent icon: the whole icon faded by the
/// colour's alpha, its own colours included.
#[test]
fn a_translucent_colour_draws_a_translucent_icon() {
    let half = Color::rgba(0x20, 0x80, 0xC0, 128);
    let icon = render_svg(SQUARE, 8, half).expect("draws");
    assert!(
        icon.argb.iter().all(|px| *px == 0x8020_80C0),
        "{:08x?}",
        &icon.argb[..2]
    );
    let red = render_svg(RED_DOT, 20, half).expect("draws");
    assert_eq!(red.argb[(10 * 20 + 10) as usize], 0x80FF_0000);
}
