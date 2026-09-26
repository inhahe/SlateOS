//! A photograph's EXIF: the camera, the exposure, when and where it was taken.
//!
//! # Why a crate
//!
//! The photo manager had the tree's only EXIF reader, and the image viewer's
//! info panel had a camera, an exposure, an ISO, an aperture and a focal length
//! to show and nothing to fill them with -- every one of them was `None` for
//! every photograph. The file manager's roadmap wants the same facts as
//! columns. Three readers of one format is the case for one parser: a parser
//! of untrusted input is an attack surface per copy, and a bug fixed in one
//! copy is not fixed in the others.
//!
//! # Where the EXIF is
//!
//! [`read`] finds it by the file's own structure rather than by searching:
//!
//! - **JPEG**: the first `APP1` segment before the scan whose payload starts
//!   `Exif\0\0`;
//! - **PNG**: the first `eXIf` chunk before the image data;
//! - **WebP**: the `EXIF` chunk, with or without the `Exif\0\0` some writers
//!   put before it;
//! - **TIFF**: the file is the structure.
//!
//! That is where Chrome reads it for JPEG and PNG (see `gui/imagecodec`'s
//! `orientation` module, whose rules these are), so the orientation reported
//! here is the one the decoder turned the picture by. The photo manager's
//! reader searched the whole file for the bytes `Exif\0\0`, which a
//! photograph's compressed data can contain by chance.
//!
//! # What it reads
//!
//! The tags [`ExifData`] names, from IFD0 and the Exif and GPS directories it
//! points to -- and no deeper. The standard puts those pointers in IFD0 only;
//! following one found anywhere else is how a file whose Exif pointer pointed
//! back at its own directory recursed until the stack ran out, crashing the
//! photo manager on import (fixed there on 2026-09-26, and the reason
//! [`MAX_DEPTH`] exists).
//!
//! Every value is read through one function that knows where a value lives --
//! in the entry itself when it fits in four bytes, at an offset when it does
//! not -- and refuses one that runs past the end. A value of the wrong type
//! for its tag is not guessed at: it is left out.
//!
//! Text is kept as text when it is UTF-8 (as good as all of it is ASCII) and
//! shown by its bytes, as escapes, when it is not -- never decoded lossily.

/// A photograph's EXIF, as much of it as the file has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExifData {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub focal_length_mm: Option<f32>,
    pub aperture: Option<f32>,
    /// `1/250`, or `2.5` for a long exposure, in seconds.
    pub shutter_speed: Option<String>,
    pub iso: Option<u32>,
    pub flash_fired: Option<bool>,
    /// As the file writes it: `2025:06:15 14:30:22`.
    pub date_taken: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    /// Metres; below sea level is negative.
    pub gps_altitude: Option<f32>,
    /// 1 to 8, the EXIF values.
    pub orientation: Option<u16>,
    pub software: Option<String>,
    pub copyright: Option<String>,
    /// The width the camera recorded, which is the stored width, before any
    /// turn the orientation asks for.
    pub width: Option<u32>,
    /// The height the camera recorded, likewise.
    pub height: Option<u32>,
    pub color_space: Option<String>,
    pub white_balance: Option<String>,
    pub metering_mode: Option<String>,
    /// How the camera chose the exposure: `Aperture priority`, `Manual`...
    pub exposure_program: Option<String>,
    /// Exposure compensation, in stops.
    pub exposure_bias: Option<f32>,
}

impl ExifData {
    /// No EXIF at all.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether the file had nothing this reads.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The camera, make and model, as one name: `Canon EOS R5`. The model is
    /// used alone when it already begins with the maker's name, as many do.
    #[must_use]
    pub fn camera(&self) -> Option<String> {
        match (self.camera_make.as_deref(), self.camera_model.as_deref()) {
            (Some(make), Some(model)) => {
                let (make, model) = (make.trim(), model.trim());
                // The maker's first word: `NIKON CORPORATION` makes the
                // `NIKON D850`, which says so already.
                let maker = make.split_whitespace().next().unwrap_or(make);
                if model
                    .to_ascii_lowercase()
                    .starts_with(&maker.to_ascii_lowercase())
                {
                    Some(model.to_owned())
                } else {
                    Some(format!("{make} {model}"))
                }
            }
            (Some(one), None) | (None, Some(one)) => Some(one.trim().to_owned()),
            (None, None) => None,
        }
    }

    /// The resolution as `W x H`, or `Unknown`.
    #[must_use]
    pub fn resolution_str(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w} x {h}"),
            _ => String::from("Unknown"),
        }
    }

    /// The position as `37.7749N 122.4194W`.
    #[must_use]
    pub fn gps_str(&self) -> Option<String> {
        match (self.gps_latitude, self.gps_longitude) {
            (Some(lat), Some(lon)) => {
                let lat_dir = if lat >= 0.0 { "N" } else { "S" };
                let lon_dir = if lon >= 0.0 { "E" } else { "W" };
                Some(format!(
                    "{:.4}{} {:.4}{}",
                    lat.abs(),
                    lat_dir,
                    lon.abs(),
                    lon_dir
                ))
            }
            _ => None,
        }
    }

    /// Megapixels, from the recorded size.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a megapixel count shown to one place; f32 is far finer"
    )]
    pub fn megapixels(&self) -> Option<f32> {
        match (self.width, self.height) {
            (Some(w), Some(h)) => {
                let px = f64::from(w) * f64::from(h);
                Some((px / 1_000_000.0) as f32)
            }
            _ => None,
        }
    }

    /// Aperture, shutter and ISO, as a photographer writes them:
    /// `f/2.8  1/250s  ISO 400`.
    #[must_use]
    pub fn exposure_summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ap) = self.aperture {
            parts.push(format!("f/{ap:.1}"));
        }
        if let Some(ref ss) = self.shutter_speed {
            parts.push(format!("{ss}s"));
        }
        if let Some(iso) = self.iso {
            parts.push(format!("ISO {iso}"));
        }
        if parts.is_empty() {
            String::from("No exposure data")
        } else {
            parts.join("  ")
        }
    }
}

/// How deep the directories are followed: IFD0 is depth 0, and the Exif and
/// GPS directories it points to are depth 1. See the module docs.
pub const MAX_DEPTH: u8 = 1;

/// The most entries read from one directory. Real ones hold tens; the count
/// is two bytes the file chose.
const MAX_ENTRIES: usize = 512;

/// The EXIF of the picture file `data` -- JPEG, PNG, WebP or TIFF -- or none.
#[must_use]
pub fn read(data: &[u8]) -> ExifData {
    locate(data).map_or_else(ExifData::empty, parse_tiff)
}

/// Where the TIFF structure holding the EXIF is, in a file of any format this
/// knows.
fn locate(data: &[u8]) -> Option<&[u8]> {
    if data.starts_with(&[0xFF, 0xD8]) {
        jpeg_exif(data)
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_exif(data)
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        webp_exif(data)
    } else if data.starts_with(b"II*\0") || data.starts_with(b"MM\0*") {
        Some(data)
    } else {
        None
    }
}

/// The first `APP1` segment before the scan that is EXIF.
fn jpeg_exif(data: &[u8]) -> Option<&[u8]> {
    let mut at: usize = 2;
    loop {
        if *data.get(at)? != 0xFF {
            return None;
        }
        let marker = *data.get(at.checked_add(1)?)?;
        match marker {
            // Fill bytes before a marker.
            0xFF => {
                at = at.checked_add(1)?;
                continue;
            }
            // Markers with no length.
            0x01 | 0xD0..=0xD7 => {
                at = at.checked_add(2)?;
                continue;
            }
            // The scan, or the end: no EXIF before it.
            0xDA | 0xD9 => return None,
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes([
            *data.get(at.checked_add(2)?)?,
            *data.get(at.checked_add(3)?)?,
        ]));
        let body = data.get(at.checked_add(4)?..at.checked_add(2)?.checked_add(length)?)?;
        if marker == 0xE1
            && let Some(tiff) = body.strip_prefix(b"Exif\0\0")
            && !tiff.is_empty()
        {
            return Some(tiff);
        }
        at = at.checked_add(2)?.checked_add(length)?;
    }
}

/// The first `eXIf` chunk before the image data.
fn png_exif(data: &[u8]) -> Option<&[u8]> {
    let mut at: usize = 8;
    loop {
        let length = usize::try_from(u32::from_be_bytes(
            data.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
        .ok()?;
        let kind = data.get(at.checked_add(4)?..at.checked_add(8)?)?;
        let body_at = at.checked_add(8)?;
        let body = data.get(body_at..body_at.checked_add(length)?)?;
        match kind {
            b"eXIf" => return Some(body),
            b"IDAT" | b"IEND" => return None,
            _ => {}
        }
        // Past the body and its CRC.
        at = body_at.checked_add(length)?.checked_add(4)?;
    }
}

/// The `EXIF` chunk of a WebP file.
fn webp_exif(data: &[u8]) -> Option<&[u8]> {
    let mut at: usize = 12;
    loop {
        let kind = data.get(at..at.checked_add(4)?)?;
        let length = usize::try_from(u32::from_le_bytes(
            data.get(at.checked_add(4)?..at.checked_add(8)?)?
                .try_into()
                .ok()?,
        ))
        .ok()?;
        let body_at = at.checked_add(8)?;
        let body = data.get(body_at..body_at.checked_add(length)?)?;
        if kind == b"EXIF" {
            return Some(body.strip_prefix(b"Exif\0\0").unwrap_or(body));
        }
        // Chunks are padded to an even length.
        at = body_at.checked_add(length)?.checked_add(length & 1)?;
    }
}

/// A TIFF structure: its bytes, and their order.
#[derive(Clone, Copy)]
struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}

/// The EXIF types this reads, with the size of one value of each.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Byte,
    Ascii,
    Short,
    Long,
    Rational,
    Undefined,
    SRational,
}

impl Kind {
    fn from_code(code: u16) -> Option<Self> {
        Some(match code {
            1 => Self::Byte,
            2 => Self::Ascii,
            3 => Self::Short,
            4 => Self::Long,
            5 => Self::Rational,
            7 => Self::Undefined,
            10 => Self::SRational,
            _ => return None,
        })
    }

    const fn size(self) -> usize {
        match self {
            Self::Byte | Self::Ascii | Self::Undefined => 1,
            Self::Short => 2,
            Self::Long => 4,
            Self::Rational | Self::SRational => 8,
        }
    }
}

/// One directory entry's value: its type, how many, and its bytes.
#[derive(Clone, Copy)]
struct Value<'a> {
    kind: Kind,
    count: usize,
    bytes: &'a [u8],
}

impl<'a> Tiff<'a> {
    fn u16_at(self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }

    fn u32_at(self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    /// The value of the 12-byte entry at `entry`: in the entry's last four
    /// bytes when it fits there, at the offset they hold when it does not.
    fn value(self, entry: usize) -> Option<Value<'a>> {
        let kind = Kind::from_code(self.u16_at(entry.checked_add(2)?)?)?;
        let count = usize::try_from(self.u32_at(entry.checked_add(4)?)?).ok()?;
        let length = count.checked_mul(kind.size())?;
        let field = entry.checked_add(8)?;
        let at = if length <= 4 {
            field
        } else {
            usize::try_from(self.u32_at(field)?).ok()?
        };
        let bytes = self.data.get(at..at.checked_add(length)?)?;
        Some(Value { kind, count, bytes })
    }

    /// An unsigned whole number: a SHORT or a LONG (a BYTE, too).
    fn number(self, value: Value<'a>) -> Option<u32> {
        let sub = Tiff {
            data: value.bytes,
            little: self.little,
        };
        match value.kind {
            Kind::Byte => value.bytes.first().copied().map(u32::from),
            Kind::Short => sub.u16_at(0).map(u32::from),
            Kind::Long => sub.u32_at(0),
            _ => None,
        }
    }

    /// The `index`th unsigned rational, as numerator and denominator.
    fn rational(self, value: Value<'a>, index: usize) -> Option<(u32, u32)> {
        if value.kind != Kind::Rational || index >= value.count {
            return None;
        }
        let sub = Tiff {
            data: value.bytes,
            little: self.little,
        };
        let at = index.checked_mul(8)?;
        Some((sub.u32_at(at)?, sub.u32_at(at.checked_add(4)?)?))
    }

    /// A signed rational's value.
    fn signed_ratio(self, value: Value<'a>) -> Option<f64> {
        if value.kind != Kind::SRational {
            return None;
        }
        let sub = Tiff {
            data: value.bytes,
            little: self.little,
        };
        let num = i32::from_ne_bytes(sub.u32_at(0)?.to_ne_bytes());
        let den = i32::from_ne_bytes(sub.u32_at(4)?.to_ne_bytes());
        (den != 0).then(|| f64::from(num) / f64::from(den))
    }

    /// An unsigned rational's value.
    fn ratio(self, value: Value<'a>) -> Option<f64> {
        let (num, den) = self.rational(value, 0)?;
        (den != 0).then(|| f64::from(num) / f64::from(den))
    }
}

/// Text from an ASCII value: up to its first NUL, trimmed; `None` when empty.
fn text(value: Value<'_>) -> Option<String> {
    if value.kind != Kind::Ascii && value.kind != Kind::Undefined {
        return None;
    }
    let end = value
        .bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(value.bytes.len());
    let raw = value.bytes.get(..end)?;
    let shown = core::str::from_utf8(raw).map_or_else(
        |_| quoting::escape_unprintable(raw),
        |s| s.trim().to_owned(),
    );
    (!shown.is_empty()).then_some(shown)
}

/// `f32` for a value shown to a place or two.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a lens or an aperture shown to a place or two; f32 is far finer"
)]
fn to_f32(value: f64) -> f32 {
    value as f32
}

/// The greatest common divisor, for writing an exposure in lowest terms.
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let r = a.checked_rem(b).unwrap_or(0);
        a = b;
        b = r;
    }
    a
}

/// An exposure time as a photographer writes it: `1/250` below a second, in
/// lowest terms, and `2.5` from a second up.
fn shutter(num: u32, den: u32) -> Option<String> {
    if num == 0 || den == 0 {
        return None;
    }
    if num < den {
        let common = gcd(num, den).max(1);
        let (n, d) = (num.checked_div(common)?, den.checked_div(common)?);
        Some(format!("{n}/{d}"))
    } else {
        Some(format!("{:.1}", f64::from(num) / f64::from(den)))
    }
}

/// The EXIF of a TIFF structure.
#[must_use]
pub fn parse_tiff(data: &[u8]) -> ExifData {
    let mut exif = ExifData::empty();
    let little = match data.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return exif,
    };
    let tiff = Tiff { data, little };
    if tiff.u16_at(2) != Some(42) {
        return exif;
    }
    if let Some(ifd0) = tiff.u32_at(4).and_then(|o| usize::try_from(o).ok()) {
        directory(tiff, ifd0, 0, &mut exif);
    }
    exif
}

/// Read the directory at `at`, `depth` directories below IFD0.
fn directory(tiff: Tiff<'_>, at: usize, depth: u8, exif: &mut ExifData) {
    let Some(count) = tiff.u16_at(at) else {
        return;
    };
    let first = at.saturating_add(2);
    for index in 0..usize::from(count).min(MAX_ENTRIES) {
        let Some(entry) = index.checked_mul(12).and_then(|off| first.checked_add(off)) else {
            return;
        };
        let Some(tag) = tiff.u16_at(entry) else {
            return;
        };
        let Some(value) = tiff.value(entry) else {
            continue;
        };
        let number = || tiff.number(value);
        match tag {
            0x0100 | 0xA002 => exif.width = number().or(exif.width),
            0x0101 | 0xA003 => exif.height = number().or(exif.height),
            0x010F => exif.camera_make = text(value),
            0x0110 => exif.camera_model = text(value),
            0x0112 => {
                exif.orientation = number()
                    .and_then(|n| u16::try_from(n).ok())
                    .filter(|n| (1..=8).contains(n));
            }
            0x0131 => exif.software = text(value),
            0x8298 => exif.copyright = text(value),
            0x8769 | 0x8825 if depth < MAX_DEPTH => {
                if let Some(sub) = number().and_then(|o| usize::try_from(o).ok()) {
                    if tag == 0x8769 {
                        directory(tiff, sub, depth.saturating_add(1), exif);
                    } else {
                        gps(tiff, sub, exif);
                    }
                }
            }
            0x829A => {
                exif.shutter_speed = tiff
                    .rational(value, 0)
                    .and_then(|(num, den)| shutter(num, den));
            }
            0x829D => exif.aperture = tiff.ratio(value).map(to_f32),
            0x8822 => exif.exposure_program = number().and_then(program),
            0x8827 => exif.iso = number(),
            0x9003 => exif.date_taken = text(value),
            0x9204 => exif.exposure_bias = tiff.signed_ratio(value).map(to_f32),
            0x9207 => exif.metering_mode = number().map(metering),
            0x9209 => exif.flash_fired = number().map(|n| n & 1 != 0),
            0x920A => exif.focal_length_mm = tiff.ratio(value).map(to_f32),
            0xA001 => {
                exif.color_space = number().map(|n| match n {
                    1 => String::from("sRGB"),
                    0xFFFF => String::from("Uncalibrated"),
                    other => format!("Unknown ({other})"),
                });
            }
            0xA403 => {
                exif.white_balance = number().map(|n| match n {
                    0 => String::from("Auto"),
                    1 => String::from("Manual"),
                    other => format!("Unknown ({other})"),
                });
            }
            0xA434 => exif.lens = text(value),
            _ => {}
        }
    }
}

/// How the camera chose its exposure (tag 0x8822). It was read from
/// ExposureMode (0xA402), whose values mean something else -- so a camera in
/// aperture priority was reported as "Auto Bracket".
fn program(n: u32) -> Option<String> {
    Some(String::from(match n {
        1 => "Manual",
        2 => "Program",
        3 => "Aperture priority",
        4 => "Shutter priority",
        5 => "Creative",
        6 => "Action",
        7 => "Portrait",
        8 => "Landscape",
        _ => return None,
    }))
}

/// How the camera metered (tag 0x9207).
fn metering(n: u32) -> String {
    match n {
        1 => String::from("Average"),
        2 => String::from("Center-weighted"),
        3 => String::from("Spot"),
        4 => String::from("Multi-spot"),
        5 => String::from("Multi-segment"),
        6 => String::from("Partial"),
        0 => String::from("Unknown"),
        other => format!("Other ({other})"),
    }
}

/// Read the GPS directory at `at`.
fn gps(tiff: Tiff<'_>, at: usize, exif: &mut ExifData) {
    let Some(count) = tiff.u16_at(at) else {
        return;
    };
    let first = at.saturating_add(2);
    let (mut lat_ref, mut lon_ref, mut below) = (None, None, false);
    let (mut lat, mut lon, mut alt) = (None, None, None);
    for index in 0..usize::from(count).min(MAX_ENTRIES) {
        let Some(entry) = index.checked_mul(12).and_then(|off| first.checked_add(off)) else {
            return;
        };
        let Some(tag) = tiff.u16_at(entry) else {
            return;
        };
        let Some(value) = tiff.value(entry) else {
            continue;
        };
        match tag {
            1 => lat_ref = value.bytes.first().copied(),
            2 => lat = degrees(tiff, value),
            3 => lon_ref = value.bytes.first().copied(),
            4 => lon = degrees(tiff, value),
            5 => below = tiff.number(value) == Some(1),
            6 => alt = tiff.ratio(value).map(to_f32),
            _ => {}
        }
    }
    exif.gps_latitude = lat.map(|d| if lat_ref == Some(b'S') { -d } else { d });
    exif.gps_longitude = lon.map(|d| if lon_ref == Some(b'W') { -d } else { d });
    exif.gps_altitude = alt.map(|a| if below { -a } else { a });
}

/// Degrees, minutes and seconds, as three rationals, in decimal degrees.
fn degrees(tiff: Tiff<'_>, value: Value<'_>) -> Option<f64> {
    let part = |i| {
        tiff.rational(value, i)
            .filter(|&(_, den)| den != 0)
            .map(|(num, den)| f64::from(num) / f64::from(den))
    };
    Some(part(0)? + part(1)? / 60.0 + part(2)? / 3600.0)
}

#[cfg(test)]
mod tests;
