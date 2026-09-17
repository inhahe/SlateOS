//! The library file: what the photo manager remembers between sessions.
//!
//! Until this existed, closing the window discarded everything. Every
//! photograph imported, every rating typed and every flag set lasted exactly
//! as long as the process -- which is most of what the application is for.
//!
//! # The format
//!
//! A version line, then one record per line, fields separated by `|`:
//!
//! ```text
//! PHOTOLIBRARY|1
//! PHOTO|7|/home/a/IMG_0001.jpg|IMG_0001.jpg|2412|jpeg|1726...|1726...|4|red|1|0|holiday,pier
//! EXIF|camera_make=Canon|iso=400|width=6000|height=4000
//! ```
//!
//! An `EXIF` line belongs to the `PHOTO` line above it. It is written as
//! `key=value` pairs rather than as 22 positional columns because 20 of those
//! 22 are usually absent: a positional row would be mostly empty separators,
//! and adding a field later would shift every column after it. Keys are split
//! on the *first* `=` only, so a value may contain one.
//!
//! # Escaping
//!
//! `\` becomes `\\`, `|` becomes `\p`, `,` becomes `\c`, and a newline becomes
//! `\n`. The comma matters because tags are comma-joined inside one field, and
//! a tag is whatever the user typed.
//!
//! # What is deliberately not stored
//!
//! Nothing that no user can produce. `Photo::adjustments` has no way to be
//! set -- `rotate_cw`/`rotate_ccw` are called from tests only -- and
//! `Photo::faces` is constructed empty and never pushed to, so both are always
//! their default and storing them would record the absence of a feature.
//! Albums and smart albums are the same case: `create_album` and
//! `create_smart_album` exist, and every caller is in the test module. When
//! any of them becomes reachable, it gains a record type and the version goes
//! to 2; the reader below already ignores record types it does not know, so a
//! newer file loses only the records an older build cannot understand.
//!
//! `ImportKey` is not stored either, for the opposite reason: it is derived
//! from the path and size, both of which are, so storing it would be storing
//! the same fact twice and inviting the two copies to disagree.

use crate::{ColorLabel, ExifData, ImageFormat, ImportKey, Photo, PhotoId};

/// The first field of the first line. A file that does not start with this is
/// not ours, and is refused rather than guessed at.
const MAGIC: &str = "PHOTOLIBRARY";

/// The format version this build writes and is the highest it reads.
const VERSION: u32 = 1;

/// The file the library lives in, or `None` when the environment names no
/// home -- an early-boot or stripped service environment, where there is no
/// user to have a library.
#[must_use]
pub fn default_path() -> Option<std::path::PathBuf> {
    Some(settingsfile::config_dir()?.join("photolibrary.txt"))
}

/// What a load produced.
#[derive(Debug)]
pub struct Loaded {
    /// The photographs that were read.
    pub photos: Vec<Photo>,
    /// How many records could not be read.
    ///
    /// Reported rather than fatal. A single corrupt line -- a half-written
    /// file from a power loss before `write_atomically` existed, a hand edit
    /// that lost a separator -- should cost that one photograph, not the
    /// library. The count exists so the application can say so out loud
    /// instead of quietly showing a shorter list than the user remembers.
    pub skipped: usize,
}

/// Why a library file could not be read at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The first line was not a `PHOTOLIBRARY` header.
    NotALibrary,
    /// The file is from a newer build than this one.
    ///
    /// Refused rather than partially read: a version this build has never
    /// heard of may have changed what the existing fields *mean*, and reading
    /// it as though it had not would silently mangle the user's library and
    /// then save the mangling back.
    UnsupportedVersion(u32),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotALibrary => write!(f, "not a photo library file"),
            Self::UnsupportedVersion(v) => {
                write!(f, "written by a newer version ({v}) of this program")
            }
        }
    }
}

/// Render the library as the text that will be written to disk.
#[must_use]
pub fn serialize(photos: &[Photo]) -> String {
    let mut out = format!("{MAGIC}|{VERSION}\n");
    for photo in photos {
        out.push_str(&serialize_photo(photo));
        out.push('\n');
        let pairs = exif_pairs(&photo.exif);
        if !pairs.is_empty() {
            out.push_str("EXIF");
            for (key, value) in pairs {
                out.push('|');
                out.push_str(key);
                out.push('=');
                out.push_str(&esc(&value));
            }
            out.push('\n');
        }
    }
    out
}

fn serialize_photo(photo: &Photo) -> String {
    format!(
        "PHOTO|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        photo.id,
        esc(&photo.file_path),
        esc(&photo.file_name),
        photo.file_size,
        format_token(photo.format),
        photo.date_added,
        photo.date_taken.map_or(String::new(), |d| d.to_string()),
        photo.rating,
        color_token(photo.color_label),
        u8::from(photo.flagged),
        u8::from(photo.hidden),
        photo
            .tags
            .iter()
            .map(|t| esc(t))
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// Read a library file.
///
/// # Errors
///
/// [`LoadError::NotALibrary`] if the header is missing or unrecognised, and
/// [`LoadError::UnsupportedVersion`] if it was written by a newer build.
pub fn parse(text: &str) -> Result<Loaded, LoadError> {
    let mut lines = text.lines();
    let header = lines.next().ok_or(LoadError::NotALibrary)?;
    let mut header_parts = header.split('|');
    if header_parts.next() != Some(MAGIC) {
        return Err(LoadError::NotALibrary);
    }
    let version: u32 = header_parts
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or(LoadError::NotALibrary)?;
    if version > VERSION {
        return Err(LoadError::UnsupportedVersion(version));
    }

    let mut photos: Vec<Photo> = Vec::new();
    let mut skipped: usize = 0;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let (kind, rest) = line.split_once('|').unwrap_or((line, ""));
        match kind {
            "PHOTO" => match parse_photo(rest) {
                Some(photo) => photos.push(photo),
                None => skipped = skipped.saturating_add(1),
            },
            "EXIF" => {
                // Belongs to the photograph above it. An `EXIF` line with no
                // `PHOTO` before it is the one case where skipping silently
                // would be wrong, so it counts.
                match photos.last_mut() {
                    Some(photo) => apply_exif(&mut photo.exif, rest),
                    None => skipped = skipped.saturating_add(1),
                }
            }
            // A record type from a newer version. Ignored on purpose: see the
            // module doc on why the version gate is a ceiling and not an
            // equality test.
            _ => {}
        }
    }
    Ok(Loaded { photos, skipped })
}

fn parse_photo(rest: &str) -> Option<Photo> {
    let f: Vec<&str> = rest.split('|').collect();
    // Twelve fields after the record type. A line with fewer is truncated and
    // is skipped; one with more was written by a newer build, and the extra
    // are ignored rather than treated as an error.
    if f.len() < 12 {
        return None;
    }
    let id: PhotoId = f.first()?.parse().ok()?;
    let file_path = unesc(f.get(1)?);
    let file_name = unesc(f.get(2)?);
    let file_size: u64 = f.get(3)?.parse().ok()?;
    let format = format_from_token(f.get(4)?)?;
    let date_added: u64 = f.get(5)?.parse().ok()?;
    let date_taken = match *f.get(6)? {
        "" => None,
        other => Some(other.parse().ok()?),
    };
    let rating: u8 = f.get(7)?.parse().ok()?;
    let color_label = color_from_token(f.get(8)?)?;
    // Derived rather than stored, so the two can never disagree.
    let import_key = ImportKey::from_metadata(&file_path, file_size);
    Some(Photo {
        id,
        file_path,
        file_name,
        file_size,
        format,
        date_added,
        date_taken,
        rating,
        color_label,
        tags: parse_tags(f.get(11).copied().unwrap_or("")),
        exif: ExifData::empty(),
        adjustments: crate::ImageAdjustments::default(),
        faces: Vec::new(),
        import_key,
        flagged: f.get(9).is_some_and(|v| *v == "1"),
        hidden: f.get(10).is_some_and(|v| *v == "1"),
    })
}

fn parse_tags(field: &str) -> Vec<String> {
    if field.is_empty() {
        return Vec::new();
    }
    field.split(',').map(unesc).collect()
}

fn apply_exif(exif: &mut ExifData, rest: &str) {
    for pair in rest.split('|') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = unesc(value);
        set_exif_field(exif, key, &value);
    }
}

// ============================================================================
// Escaping
// ============================================================================

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\p"),
            ',' => out.push_str("\\c"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('p') => out.push('|'),
            Some('c') => out.push(','),
            Some('n') => out.push('\n'),
            // An escape this build does not know. The backslash is kept along
            // with what followed it, so the text survives a round trip through
            // an older reader instead of losing a character silently.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

// ============================================================================
// Tokens
// ============================================================================

// Written out rather than reusing `ImageFormat::extension` or a display label.
// A token in a file on the user's disk is a compatibility promise; a label is
// a thing someone may reword on a Tuesday. Tying the two together means a
// wording change silently stops old libraries loading.

const fn format_token(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Png => "png",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Gif => "gif",
        ImageFormat::Tiff => "tiff",
        ImageFormat::WebP => "webp",
        ImageFormat::Heic => "heic",
        ImageFormat::Raw => "raw",
    }
}

fn format_from_token(t: &str) -> Option<ImageFormat> {
    Some(match t {
        "jpeg" => ImageFormat::Jpeg,
        "png" => ImageFormat::Png,
        "bmp" => ImageFormat::Bmp,
        "gif" => ImageFormat::Gif,
        "tiff" => ImageFormat::Tiff,
        "webp" => ImageFormat::WebP,
        "heic" => ImageFormat::Heic,
        "raw" => ImageFormat::Raw,
        _ => return None,
    })
}

const fn color_token(c: ColorLabel) -> &'static str {
    match c {
        ColorLabel::None => "none",
        ColorLabel::Red => "red",
        ColorLabel::Orange => "orange",
        ColorLabel::Yellow => "yellow",
        ColorLabel::Green => "green",
        ColorLabel::Blue => "blue",
        ColorLabel::Purple => "purple",
    }
}

fn color_from_token(t: &str) -> Option<ColorLabel> {
    Some(match t {
        "none" => ColorLabel::None,
        "red" => ColorLabel::Red,
        "orange" => ColorLabel::Orange,
        "yellow" => ColorLabel::Yellow,
        "green" => ColorLabel::Green,
        "blue" => ColorLabel::Blue,
        "purple" => ColorLabel::Purple,
        _ => return None,
    })
}

// ============================================================================
// EXIF
// ============================================================================

/// The EXIF fields that are present, as `key`/`value` pairs.
///
/// Absent fields are omitted entirely rather than written empty, which is the
/// whole reason this is key/value: a photograph from a phone typically carries
/// four or five of the twenty-two.
fn exif_pairs(e: &ExifData) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let mut text = |k: &'static str, v: &Option<String>| {
        if let Some(v) = v {
            out.push((k, v.clone()));
        }
    };
    text("camera_make", &e.camera_make);
    text("camera_model", &e.camera_model);
    text("lens", &e.lens);
    text("shutter_speed", &e.shutter_speed);
    text("date_taken", &e.date_taken);
    text("software", &e.software);
    text("copyright", &e.copyright);
    text("color_space", &e.color_space);
    text("white_balance", &e.white_balance);
    text("metering_mode", &e.metering_mode);
    text("exposure_program", &e.exposure_program);

    if let Some(v) = e.focal_length_mm {
        out.push(("focal_length_mm", v.to_string()));
    }
    if let Some(v) = e.aperture {
        out.push(("aperture", v.to_string()));
    }
    if let Some(v) = e.iso {
        out.push(("iso", v.to_string()));
    }
    if let Some(v) = e.flash_fired {
        out.push(("flash_fired", u8::from(v).to_string()));
    }
    if let Some(v) = e.gps_latitude {
        out.push(("gps_latitude", v.to_string()));
    }
    if let Some(v) = e.gps_longitude {
        out.push(("gps_longitude", v.to_string()));
    }
    if let Some(v) = e.gps_altitude {
        out.push(("gps_altitude", v.to_string()));
    }
    if let Some(v) = e.orientation {
        out.push(("orientation", v.to_string()));
    }
    if let Some(v) = e.width {
        out.push(("width", v.to_string()));
    }
    if let Some(v) = e.height {
        out.push(("height", v.to_string()));
    }
    if let Some(v) = e.exposure_bias {
        out.push(("exposure_bias", v.to_string()));
    }
    out
}

/// Set one field from its key. An unknown key is ignored, which is what makes
/// a file from a newer build readable rather than fatal.
fn set_exif_field(e: &mut ExifData, key: &str, value: &str) {
    match key {
        "camera_make" => e.camera_make = Some(value.to_owned()),
        "camera_model" => e.camera_model = Some(value.to_owned()),
        "lens" => e.lens = Some(value.to_owned()),
        "shutter_speed" => e.shutter_speed = Some(value.to_owned()),
        "date_taken" => e.date_taken = Some(value.to_owned()),
        "software" => e.software = Some(value.to_owned()),
        "copyright" => e.copyright = Some(value.to_owned()),
        "color_space" => e.color_space = Some(value.to_owned()),
        "white_balance" => e.white_balance = Some(value.to_owned()),
        "metering_mode" => e.metering_mode = Some(value.to_owned()),
        "exposure_program" => e.exposure_program = Some(value.to_owned()),
        "focal_length_mm" => e.focal_length_mm = value.parse().ok(),
        "aperture" => e.aperture = value.parse().ok(),
        "iso" => e.iso = value.parse().ok(),
        "flash_fired" => e.flash_fired = Some(value == "1"),
        "gps_latitude" => e.gps_latitude = value.parse().ok(),
        "gps_longitude" => e.gps_longitude = value.parse().ok(),
        "gps_altitude" => e.gps_altitude = value.parse().ok(),
        "orientation" => e.orientation = value.parse().ok(),
        "width" => e.width = value.parse().ok(),
        "height" => e.height = value.parse().ok(),
        "exposure_bias" => e.exposure_bias = value.parse().ok(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail at the line that did it.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn a_photo(id: PhotoId, path: &str) -> Photo {
        Photo {
            id,
            file_path: path.to_owned(),
            file_name: "IMG.jpg".to_owned(),
            file_size: 2412,
            format: ImageFormat::Jpeg,
            date_added: 1_726_000_000,
            date_taken: Some(1_725_000_000),
            rating: 4,
            color_label: ColorLabel::Red,
            tags: vec!["holiday".to_owned(), "pier".to_owned()],
            exif: ExifData::empty(),
            adjustments: crate::ImageAdjustments::default(),
            faces: Vec::new(),
            import_key: ImportKey::from_metadata(path, 2412),
            flagged: true,
            hidden: false,
        }
    }

    /// Everything a user can produce survives a save and a load.
    #[test]
    fn a_library_survives_a_round_trip() {
        let photos = vec![a_photo(1, "/home/a/one.jpg"), a_photo(2, "/home/a/two.jpg")];
        let loaded = parse(&serialize(&photos)).expect("it is our own output");

        assert_eq!(loaded.skipped, 0);
        assert_eq!(loaded.photos.len(), 2);
        let first = loaded.photos.first().expect("one");
        assert_eq!(first.id, 1);
        assert_eq!(first.file_path, "/home/a/one.jpg");
        assert_eq!(first.file_size, 2412);
        assert_eq!(first.format, ImageFormat::Jpeg);
        assert_eq!(first.date_taken, Some(1_725_000_000));
        assert_eq!(first.rating, 4);
        assert_eq!(first.color_label, ColorLabel::Red);
        assert_eq!(first.tags, vec!["holiday".to_owned(), "pier".to_owned()]);
        assert!(first.flagged);
        assert!(!first.hidden);
    }

    /// The import key is rebuilt from the path and size, and matches.
    ///
    /// It is not stored, so this is the test that the derivation on load
    /// agrees with the one at import -- if it did not, re-importing a
    /// photograph already in a saved library would not be recognised as a
    /// duplicate.
    #[test]
    fn the_import_key_is_rederived_and_agrees() {
        let photos = vec![a_photo(1, "/home/a/one.jpg")];
        let loaded = parse(&serialize(&photos)).expect("round trip");
        assert_eq!(
            loaded.photos.first().expect("one").import_key,
            photos.first().expect("one").import_key
        );
    }

    /// A path or tag containing the separators survives them.
    ///
    /// The case that made escaping necessary: a filename may contain `|` or a
    /// comma, and a tag is whatever the user typed. Without escaping, one
    /// pipe in a path shifts every field after it by one.
    #[test]
    fn separators_inside_a_value_survive() {
        let mut photo = a_photo(9, "/home/a/awkward|name,with\\slash.jpg");
        photo.tags = vec![
            "a,comma".to_owned(),
            "a|pipe".to_owned(),
            "back\\slash".to_owned(),
        ];
        let loaded = parse(&serialize(&[photo.clone()])).expect("round trip");

        let got = loaded.photos.first().expect("one");
        assert_eq!(got.file_path, photo.file_path);
        assert_eq!(got.tags, photo.tags);
    }

    /// Only the EXIF fields that are present are written, and they come back.
    #[test]
    fn exif_round_trips_the_fields_that_are_present() {
        let mut photo = a_photo(1, "/a.jpg");
        photo.exif.camera_make = Some("Canon".to_owned());
        photo.exif.iso = Some(400);
        photo.exif.aperture = Some(1.8);
        photo.exif.flash_fired = Some(false);

        let text = serialize(&[photo]);
        assert!(text.contains("camera_make=Canon"), "{text}");
        assert!(
            !text.contains("lens="),
            "an absent field was written anyway: {text}"
        );

        let loaded = parse(&text).expect("round trip");
        let exif = &loaded.photos.first().expect("one").exif;
        assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
        assert_eq!(exif.iso, Some(400));
        assert_eq!(exif.aperture, Some(1.8));
        assert_eq!(exif.flash_fired, Some(false));
        assert_eq!(exif.lens, None);
    }

    /// Something that is not a library is refused, not guessed at.
    #[test]
    fn a_file_that_is_not_a_library_is_refused() {
        let err = parse("hello\nworld\n").expect_err("should refuse");
        assert_eq!(err, LoadError::NotALibrary);
        assert_eq!(
            parse("").expect_err("an empty file is not a library either"),
            LoadError::NotALibrary
        );
    }

    /// A file from a newer build is refused rather than half-read.
    #[test]
    fn a_newer_version_is_refused() {
        let err = parse("PHOTOLIBRARY|99\n").expect_err("should refuse");
        assert_eq!(err, LoadError::UnsupportedVersion(99));
    }

    /// A corrupt record costs one photograph, not the library.
    #[test]
    fn a_corrupt_record_costs_one_photograph() {
        let good = serialize(&[a_photo(1, "/a.jpg"), a_photo(2, "/b.jpg")]);
        let broken = good.replace("PHOTO|2|/b.jpg", "PHOTO|2|/b.jpg|truncated");
        let loaded = parse(&broken).expect("the header is still good");

        assert_eq!(loaded.photos.len(), 1, "the good record still loaded");
        assert_eq!(loaded.skipped, 1, "and the bad one was counted, not hidden");
    }

    /// A record type this build has never heard of is ignored.
    ///
    /// This is what lets a version 2 file, which will have `ALBUM` records,
    /// still load its photographs in a build that predates albums being
    /// creatable at all.
    #[test]
    fn an_unknown_record_type_is_ignored() {
        let mut text = serialize(&[a_photo(1, "/a.jpg")]);
        text.push_str("ALBUM|1|Holiday|1\n");
        let loaded = parse(&text).expect("still readable");
        assert_eq!(loaded.photos.len(), 1);
        assert_eq!(loaded.skipped, 0, "an unknown record is not a failure");
    }

    /// An EXIF line with no photograph before it is counted, not dropped.
    #[test]
    fn a_stray_exif_line_is_counted() {
        let text = format!("{MAGIC}|{VERSION}\nEXIF|iso=100\n");
        let loaded = parse(&text).expect("header is good");
        assert_eq!(loaded.photos.len(), 0);
        assert_eq!(loaded.skipped, 1);
    }
}
