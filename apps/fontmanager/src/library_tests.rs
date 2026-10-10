//! Tests of the font library, over font files built byte by byte.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use super::*;

/// A TrueType face named `family`: three glyphs, `.notdef` empty and `A` and
/// `B` squares, 1000 units to the em, 800 up and 200 down; bold and italic as
/// `head.macStyle` says, fixed pitch as `post.isFixedPitch` says. The
/// smallest font the index takes -- it reads the family from `name`, the
/// style from `head` where there is no `OS/2`, and the pitch from `post` --
/// laid out as `osfont::testing::colour_face` lays out its tables.
pub(crate) fn test_font(family: &str, bold: bool, italic: bool, fixed: bool) -> Vec<u8> {
    named_font(&[(name_id::FAMILY, family)], bold, italic, fixed)
}

/// [`font`] with the names given: `(name id, name)` each.
fn named_font(names: &[(u16, &str)], bold: bool, italic: bool, fixed: bool) -> Vec<u8> {
    let square = {
        let mut g = Vec::new();
        for v in [1i16, 100, 0, 200, 100] {
            g.extend_from_slice(&v.to_be_bytes());
        }
        g.extend_from_slice(&3u16.to_be_bytes());
        g.extend_from_slice(&0u16.to_be_bytes());
        g.extend_from_slice(&[0x01; 4]);
        for v in [100i16, 100, 0, -100, 0, 0, 100, 0] {
            g.extend_from_slice(&v.to_be_bytes());
        }
        g
    };
    let mut glyf = square.clone();
    glyf.extend_from_slice(&square);
    let mut loca = Vec::new();
    for offset in [0u32, 0, square.len() as u32, 2 * square.len() as u32] {
        loca.extend_from_slice(&offset.to_be_bytes());
    }
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mac_style = u16::from(bold) | (u16::from(italic) << 1);
    head[44..46].copy_from_slice(&mac_style.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    hhea[10..12].copy_from_slice(&300u16.to_be_bytes());
    hhea[34..36].copy_from_slice(&3u16.to_be_bytes());
    let mut maxp = vec![0u8; 6];
    maxp[4..6].copy_from_slice(&3u16.to_be_bytes());
    let mut hmtx = Vec::new();
    for lsb in [0i16, 100, 100] {
        hmtx.extend_from_slice(&300u16.to_be_bytes());
        hmtx.extend_from_slice(&lsb.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    for v in [4u16, 32, 0, 4, 4, 1, 0, 0x42, 0xFFFF, 0, 0x41, 0xFFFF] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }
    for v in [1u16.wrapping_sub(0x41), 1, 0, 0] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }
    // `name`, format 0: each name as a Windows Unicode record in UTF-16BE.
    let strings: Vec<Vec<u8>> = names
        .iter()
        .map(|(_, name)| name.encode_utf16().flat_map(u16::to_be_bytes).collect())
        .collect();
    let mut name = Vec::new();
    let count = names.len() as u16;
    for v in [0u16, count, 6 + 12 * count] {
        name.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 0u16;
    for ((id, _), string) in names.iter().zip(&strings) {
        for v in [3u16, 1, 0x0409, *id, string.len() as u16, offset] {
            name.extend_from_slice(&v.to_be_bytes());
        }
        offset += string.len() as u16;
    }
    for string in &strings {
        name.extend_from_slice(string);
    }
    // `post`, version 3: no glyph names, and whether it is fixed pitch.
    let mut post = vec![0u8; 32];
    post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
    post[12..16].copy_from_slice(&u32::from(fixed).to_be_bytes());

    assemble(&[
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"name", name),
        (*b"post", post),
    ])
}

/// An sfnt file around `tables`, as `osfont::testing` assembles one.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.extend(std::iter::repeat_n(0u8, pad));
        offset += data.len() + pad;
    }
    out.extend_from_slice(&body);
    out
}

/// A machine's fonts in a scratch folder: the system's -- Alpha in two
/// styles, fixed-pitch Beta -- and the user's own folder, holding Gamma.
pub(crate) struct ScratchFonts {
    _dir: scratchdir::ScratchDir,
    /// The system's fonts folder.
    pub system: PathBuf,
    /// The user's own.
    pub own: PathBuf,
    /// A folder of files to install from.
    pub downloads: PathBuf,
}

impl ScratchFonts {
    pub(crate) fn new(tag: &str) -> Self {
        let dir = scratchdir::ScratchDir::new(tag);
        let system = dir.dir().join("system");
        let own = dir.dir().join("home").join("fonts");
        let downloads = dir.dir().join("downloads");
        for folder in [&system, &own, &downloads] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(
            system.join("Alpha-Regular.ttf"),
            test_font("Alpha", false, false, false),
        )
        .unwrap();
        std::fs::write(
            system.join("Alpha-Bold.ttf"),
            test_font("Alpha", true, false, false),
        )
        .unwrap();
        std::fs::write(
            system.join("Beta-Mono.ttf"),
            test_font("Beta", false, false, true),
        )
        .unwrap();
        std::fs::write(
            own.join("Gamma.ttf"),
            test_font("Gamma", false, true, false),
        )
        .unwrap();
        Self {
            _dir: dir,
            system,
            own,
            downloads,
        }
    }

    pub(crate) fn places(&self) -> FontPlaces {
        FontPlaces {
            dirs: vec![self.system.clone(), self.own.clone()],
            own: Some(self.own.clone()),
        }
    }

    /// `bytes` as a file named `name` in the downloads folder.
    pub(crate) fn download(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.downloads.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

fn names(library: &Library) -> Vec<String> {
    library.families().iter().map(|f| f.name.clone()).collect()
}

/// **The fonts listed are the ones in the folders** -- each family with its
/// styles, whether it is fixed-pitch, and whose it is -- and nothing else.
#[test]
fn the_fonts_listed_are_the_ones_in_the_folders() {
    let machine = ScratchFonts::new("fontmanager-listed");
    let library = Library::scan(machine.places());
    assert_eq!(names(&library), ["Alpha", "Beta", "Gamma"]);
    let alpha = library.family("alpha").expect("found in any case");
    assert_eq!(alpha.style_names(), ["Regular", "Bold"]);
    assert!(!alpha.is_fixed_pitch() && !alpha.has_own());
    assert!(library.family("Beta").unwrap().is_fixed_pitch());
    let gamma = library.family("Gamma").unwrap();
    assert!(gamma.all_own() && gamma.has_own());
    assert_eq!(gamma.style_names(), ["Italic"]);
    assert_eq!(gamma.own_files(), [machine.own.join("Gamma.ttf")]);
    assert!(
        library.load("Alpha").is_some(),
        "a family listed cannot be drawn"
    );
}

/// **A font file is installed into the user's fonts folder** -- made if it
/// is not there yet -- and is listed as theirs at once.
#[test]
fn a_font_file_is_installed_into_your_fonts_folder() {
    let machine = ScratchFonts::new("fontmanager-install");
    let fresh = machine.own.parent().unwrap().join("new-fonts");
    let mut places = machine.places();
    places.own = Some(fresh.clone());
    places.dirs.push(fresh.clone());
    let mut library = Library::scan(places);
    let from = machine.download("Delta-Bold.otf", &test_font("Delta", true, false, false));
    let installed = library.install(&from).expect("installed");
    assert_eq!(installed.family, "Delta");
    assert_eq!(installed.style, "Bold");
    assert_eq!(installed.path, fresh.join("Delta-Bold.otf"));
    assert_eq!(
        std::fs::read(&installed.path).unwrap(),
        std::fs::read(&from).unwrap(),
        "the file written is not the file chosen"
    );
    let delta = library.family("Delta").expect("listed at once");
    assert!(delta.all_own());
    assert!(from.exists(), "the file installed from was moved");
}

/// **A file is never written over**: a second file of the same name is
/// written beside the first, as `Name (2)`.
#[test]
fn a_second_file_of_the_same_name_is_written_beside_the_first() {
    let machine = ScratchFonts::new("fontmanager-same-name");
    let mut library = Library::scan(machine.places());
    let first = machine.download("Delta.ttf", &test_font("Delta", false, false, false));
    library.install(&first).unwrap();
    std::fs::remove_file(&first).unwrap();
    let second = machine.download("Delta.ttf", &test_font("Delta", true, false, false));
    let installed = library
        .install(&second)
        .expect("the bold one is another style");
    assert_eq!(installed.path, machine.own.join("Delta (2).ttf"));
    assert!(
        machine.own.join("Delta.ttf").exists(),
        "the first was written over"
    );
    assert_eq!(
        library.family("Delta").unwrap().style_names(),
        ["Regular", "Bold"]
    );
}

/// **The same family in the same style is not installed twice** -- but one
/// the system has is the user's to install over it.
#[test]
fn the_same_family_and_style_is_not_installed_twice() {
    let machine = ScratchFonts::new("fontmanager-twice");
    let mut library = Library::scan(machine.places());
    let gamma = machine.download("Gamma-copy.ttf", &test_font("Gamma", false, true, false));
    let refused = library.install(&gamma).unwrap_err();
    assert!(
        matches!(&refused, InstallError::AlreadyInstalled { family, style } if family == "Gamma" && style == "Italic"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "you have Gamma Italic installed already"
    );
    let alpha = machine.download("Alpha-mine.ttf", &test_font("Alpha", false, false, false));
    library
        .install(&alpha)
        .expect("the system's Alpha is not the user's");
    assert!(library.family("Alpha").unwrap().has_own());
}

/// **What is not a font is refused, and nothing is written**: a name that
/// is not a font's, bytes that are not a font, a font with no family name,
/// and a machine with no home folder.
#[test]
fn what_is_not_a_font_is_refused_and_nothing_is_written() {
    let machine = ScratchFonts::new("fontmanager-refused");
    let mut library = Library::scan(machine.places());
    let before: Vec<_> = std::fs::read_dir(&machine.own).unwrap().collect();

    let text = machine.download("notes.txt", &test_font("Delta", false, false, false));
    assert!(matches!(
        library.install(&text),
        Err(InstallError::NotAFontName)
    ));
    let junk = machine.download("junk.ttf", b"not a font at all");
    assert!(matches!(
        library.install(&junk),
        Err(InstallError::NotAFont)
    ));
    let nameless = machine.download("nameless.ttf", &named_font(&[], false, false, false));
    assert!(matches!(
        library.install(&nameless),
        Err(InstallError::NoFamilyName)
    ));
    let missing = machine.downloads.join("gone.ttf");
    assert!(matches!(
        library.install(&missing),
        Err(InstallError::Read(_))
    ));

    assert_eq!(
        std::fs::read_dir(&machine.own).unwrap().count(),
        before.len(),
        "a refused file was written"
    );

    let mut homeless = machine.places();
    homeless.own = None;
    let mut library = Library::scan(homeless);
    let delta = machine.download("Delta.ttf", &test_font("Delta", false, false, false));
    assert!(matches!(
        library.install(&delta),
        Err(InstallError::NoFontsFolder)
    ));
}

/// **The user's own fonts are removed, and the system's are not.**
#[test]
fn your_fonts_are_removed_and_the_systems_are_not() {
    let machine = ScratchFonts::new("fontmanager-remove");
    let mut library = Library::scan(machine.places());
    assert_eq!(library.remove("Gamma").expect("removed"), 1);
    assert!(!machine.own.join("Gamma.ttf").exists());
    assert_eq!(names(&library), ["Alpha", "Beta"]);
    assert!(matches!(
        library.remove("Alpha"),
        Err(RemoveError::SystemFonts)
    ));
    assert!(machine.system.join("Alpha-Regular.ttf").exists());
    assert!(matches!(
        library.remove("Nope"),
        Err(RemoveError::NotInstalled)
    ));

    // A family that is partly the user's loses only their faces.
    let mine = machine.download("Alpha-mine.ttf", &test_font("Alpha", true, true, false));
    library.install(&mine).unwrap();
    assert_eq!(library.remove("Alpha").expect("the user's face"), 1);
    assert_eq!(
        library.family("Alpha").unwrap().style_names(),
        ["Regular", "Bold"]
    );
}

/// **A file holding two families is said to take the other with it.**
#[test]
fn a_file_another_family_shares_is_named() {
    let machine = ScratchFonts::new("fontmanager-sharing");
    std::fs::write(
        machine.own.join("Pair.ttf"),
        named_font(
            &[
                (name_id::FAMILY, "Pair Bold"),
                (name_id::TYPOGRAPHIC_FAMILY, "Pair"),
            ],
            true,
            false,
            false,
        ),
    )
    .unwrap();
    let library = Library::scan(machine.places());
    let pair = library
        .family("Pair")
        .expect("listed by its typographic name");
    assert_eq!(
        library.sharing("Pair", &pair.own_files()),
        ["Pair Bold"],
        "removing Pair takes Pair Bold with it, and does not say so"
    );
    assert!(
        library
            .sharing("Gamma", &[machine.own.join("Gamma.ttf")])
            .is_empty()
    );
}

/// **Styles are named as people name them.**
#[test]
fn styles_are_named_as_people_name_them() {
    let style = |weight, italic, width| Style {
        weight,
        italic,
        width,
    };
    assert_eq!(style_name(style(400, false, 5)), "Regular");
    assert_eq!(style_name(style(400, true, 5)), "Italic");
    assert_eq!(style_name(style(700, true, 5)), "Bold Italic");
    assert_eq!(style_name(style(300, false, 3)), "Condensed Light");
    assert_eq!(style_name(style(400, false, 3)), "Condensed");
    assert_eq!(style_name(style(900, false, 7)), "Expanded Black");
    assert_eq!(style_name(style(100, false, 5)), "Thin");
}
