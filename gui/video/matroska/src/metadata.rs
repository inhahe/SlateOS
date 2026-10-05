//! What FFmpeg makes of a file's description beyond its tracks: the
//! metadata of the file, of each track, chapter and attachment -- keys and
//! values, as `ffprobe` shows them -- and the chapters and attachments
//! themselves.
//!
//! FFmpeg's rules (`matroska_read_header`'s last part, and
//! `matroska_convert_tag`), its behaviour reproduced rather than its code:
//!
//! - **The file's** metadata is its `title`, `encoder` (`MuxingApp`) and
//!   `creation_time` (`DateUTC`), then the tags that name no track,
//!   chapter or attachment -- their keys prefixed by the tag's
//!   `TargetType` (`ALBUM/TITLE`) where it has one.
//! - **A track's** is its `enc_key_id` (an encrypted track's key ID, in
//!   base64), `language` (unless `und`), `title` (its `Name`), and for
//!   video `stereo_mode` and `alpha_mode`; then the tags naming its UID.
//! - **A chapter** is each top-level `ChapterAtom` of every edition with a
//!   start and a nonzero UID that starts after the last one kept (any does
//!   while that one started at 0) -- a chapter whose end is before its
//!   start is left out, but still counts as started -- and one of a UID
//!   already seen replaces that one in place.
//!   Its metadata is its `title` (the last `ChapString` of its displays),
//!   then its tags.
//! - **An attachment** is each `AttachedFile` with a name, a media type and
//!   data; its metadata `filename`, `mimetype` and `title` (its
//!   description), then its tags. FFmpeg makes each a stream after the
//!   tracks, and stops at 1000 streams.
//! - **A tag** (`SimpleTag`) is its name as the key and its string as the
//!   value; one in a language other than `und` is keyed `NAME-lang`, and
//!   under its plain name too if it is the default; one inside another is
//!   keyed `OUTER/INNER`. Setting a key replaces the first entry of the same
//!   name, its case aside, and a tag with no string removes it.
//!   `LEAD_PERFORMER` becomes `performer` and `PART_NUMBER` `track`.
//!
//! The order of the entries is FFmpeg's to the last detail -- its
//! dictionary replaces an entry by moving its last one into the gap and
//! adding the new one at the end, and its renaming pass rebuilds the
//! dictionary, dropping an entry identical to an earlier one -- because
//! `ffprobe` and every player built on FFmpeg list them in that order.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::SegmentInfo;
use crate::nest::{NOPTS, RawAttachment, RawChapter, RawTag, RawTags, SeekEntry};
use crate::track::Track;

/// Keys and values describing a file, a track, a chapter or an attachment,
/// as FFmpeg's demuxer gives them (an `AVDictionary`): bytes as the file
/// wrote them (UTF-8 by the specification, not checked), in FFmpeg's order.
/// A key may appear twice, as in FFmpeg after `PART_NUMBER` is renamed
/// `track` beside a `track` already there.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    entries: Vec<(Vec<u8>, Vec<u8>)>,
    /// Whether FFmpeg's renaming pass could change anything now: a key it
    /// renames was set, or a key that is held twice -- either may leave an
    /// entry to rename or two the same.
    dirty: bool,
}

impl PartialEq for Metadata {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Eq for Metadata {}

impl Metadata {
    /// The value of the first entry whose key is `key`, ASCII case aside --
    /// as FFmpeg's `av_dict_get` finds one.
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_slice())
    }

    /// Every entry, in order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.entries
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// FFmpeg's `av_dict_set(.., 0)`: the first entry keyed `key`, case
    /// aside, is removed -- the last entry moved into its place -- and
    /// `(key, value)` added at the end, if there is a value.
    pub(crate) fn set(&mut self, key: &[u8], value: Option<&[u8]>, budget: &mut Budget) {
        let cost = self
            .entries
            .len()
            .saturating_add(key.len())
            .saturating_add(value.map_or(0, <[u8]>::len));
        if !budget.spend(cost) {
            return;
        }
        let mut matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| k.eq_ignore_ascii_case(key))
            .map(|(i, _)| i);
        let first = matches.next();
        // Two or more already: after one is replaced, the new entry may
        // equal another, which the renaming pass drops.
        if matches.next().is_some() || renamed(key).is_some() {
            self.dirty = true;
        }
        if let Some(i) = first {
            self.entries.swap_remove(i);
        }
        if let Some(v) = value {
            self.entries.push((key.to_vec(), v.to_vec()));
        }
    }

    /// FFmpeg's `ff_metadata_conv` with Matroska's names: the dictionary
    /// rebuilt in order, `LEAD_PERFORMER` renamed `performer` and
    /// `PART_NUMBER` `track` (case aside), and an entry dropped where an
    /// earlier one has the same key (case aside) and the same value.
    fn rename(&mut self, budget: &mut Budget) {
        if !self.dirty {
            // Nothing to rename and no key twice: the rebuild would give
            // back the same entries in the same order.
            return;
        }
        let cost = self.entries.iter().fold(self.entries.len(), |n, (k, v)| {
            n.saturating_add(k.len()).saturating_add(v.len())
        });
        if !budget.spend(cost) {
            return;
        }
        let mut seen: HashSet<(Vec<u8>, Vec<u8>)> = HashSet::with_capacity(self.entries.len());
        let mut out = Vec::with_capacity(self.entries.len());
        for (k, v) in self.entries.drain(..) {
            let k = renamed(&k).map_or(k, <[u8]>::to_vec);
            if seen.insert((k.to_ascii_lowercase(), v.clone())) {
                out.push((k, v));
            }
        }
        self.entries = out;
        self.dirty = false;
    }
}

/// What FFmpeg renames a Matroska tag to (`ff_mkv_metadata_conv`).
fn renamed(key: &[u8]) -> Option<&'static [u8]> {
    if key.eq_ignore_ascii_case(b"LEAD_PERFORMER") {
        Some(b"performer")
    } else if key.eq_ignore_ascii_case(b"PART_NUMBER") {
        Some(b"track")
    } else {
        None
    }
}

/// A bound on the work of building the metadata: FFmpeg's dictionary is a
/// list searched end to end on every change, so a file of hundreds of
/// thousands of tags for one dictionary would take it -- and a faithful copy
/// -- hours. Each change costs the entries it looks at and the bytes it
/// copies; past 2^28 of those (some 20,000 tags for one dictionary, far
/// beyond any real file) the rest of the file's tags are left out.
pub(crate) struct Budget(usize);

impl Budget {
    pub(crate) const fn new() -> Self {
        Self(1 << 28)
    }

    /// A smaller budget, for a test to reach the end of quickly.
    #[cfg(test)]
    const fn of(n: usize) -> Self {
        Self(n)
    }

    fn spend(&mut self, n: usize) -> bool {
        match self.0.checked_sub(n) {
            Some(left) => {
                self.0 = left;
                true
            }
            None => {
                self.0 = 0;
                false
            }
        }
    }
}

/// The longest key FFmpeg makes of a tag's name and its prefixes: its
/// 1024-byte buffer, less the NUL.
const MAX_KEY: usize = 1023;

/// FFmpeg's `matroska_convert_tag`: `tags` into `metadata`, each key behind
/// `prefix/` where there is a prefix.
fn convert_tag(
    tags: &[RawTag],
    metadata: &mut Metadata,
    prefix: Option<&[u8]>,
    budget: &mut Budget,
) {
    for t in tags {
        let lang = t.lang.as_deref().filter(|l| *l != b"und");
        let Some(name) = t.name.as_deref() else {
            continue;
        };
        let mut key: Vec<u8> = match prefix {
            Some(p) => p
                .iter()
                .chain(b"/")
                .chain(name)
                .take(MAX_KEY)
                .copied()
                .collect(),
            None => name.iter().take(MAX_KEY).copied().collect(),
        };
        if t.default != 0 || lang.is_none() {
            metadata.set(&key, t.string.as_deref(), budget);
            if !t.sub.is_empty() {
                convert_tag(&t.sub, metadata, Some(&key), budget);
            }
        }
        if let Some(lang) = lang {
            key.push(b'-');
            key.extend_from_slice(lang);
            key.truncate(MAX_KEY);
            metadata.set(&key, t.string.as_deref(), budget);
            if !t.sub.is_empty() {
                convert_tag(&t.sub, metadata, Some(&key), budget);
            }
        }
    }
    metadata.rename(budget);
}

/// One chapter, as FFmpeg makes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chapter {
    /// `ChapterUID`: FFmpeg's ID for the chapter, which it holds signed --
    /// one of 2^63 or more shows negative.
    pub uid: u64,
    /// When it starts, in nanoseconds.
    pub start: i64,
    /// When it ends, in nanoseconds, where the file says;
    /// [`crate::Demuxer::chapter_ends`] gives the rest as FFmpeg fills them
    /// in.
    pub end: Option<i64>,
    /// Its `title`, then the tags naming it.
    pub metadata: Metadata,
}

/// What an attachment is, as FFmpeg takes it from the start of its media
/// type (case counting).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentKind {
    /// `image/gif`: a picture.
    Gif,
    /// `image/jpeg`: a picture.
    Jpeg,
    /// `image/png`: a picture.
    Png,
    /// `image/tiff`: a picture.
    Tiff,
    /// `application/x-truetype-font` or `application/x-font`: a font, for
    /// subtitles that name it.
    TrueType,
    /// `application/vnd.ms-opentype`: a font.
    OpenType,
    /// `binary`.
    Binary,
    /// A media type FFmpeg does not name.
    Other,
}

impl AttachmentKind {
    fn of(media_type: &[u8]) -> Self {
        const NAMES: [(&[u8], AttachmentKind); 8] = [
            (b"image/gif", AttachmentKind::Gif),
            (b"image/jpeg", AttachmentKind::Jpeg),
            (b"image/png", AttachmentKind::Png),
            (b"image/tiff", AttachmentKind::Tiff),
            (b"application/x-truetype-font", AttachmentKind::TrueType),
            (b"application/x-font", AttachmentKind::TrueType),
            (b"application/vnd.ms-opentype", AttachmentKind::OpenType),
            (b"binary", AttachmentKind::Binary),
        ];
        NAMES
            .iter()
            .find(|(name, _)| media_type.starts_with(name))
            .map_or(Self::Other, |&(_, kind)| kind)
    }

    /// Whether FFmpeg shows it as a picture -- a stream of one frame, the
    /// cover art of a recording -- rather than an attachment.
    pub fn is_picture(self) -> bool {
        matches!(self, Self::Gif | Self::Jpeg | Self::Png | Self::Tiff)
    }
}

/// One attached file -- cover art, a font for the subtitles -- as FFmpeg
/// keeps it: one with a name, a media type and data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// `FileUID`, by which tags name it.
    pub uid: u64,
    /// `FileName`, as written.
    pub name: Vec<u8>,
    /// `FileMediaType` (a MIME type), as written.
    pub media_type: Vec<u8>,
    /// `FileDescription`, where it has one.
    pub description: Option<Vec<u8>>,
    pub kind: AttachmentKind,
    /// How many bytes it holds, which [`crate::Demuxer::attachment_data`]
    /// reads.
    pub size: u64,
    /// `filename`, `mimetype`, `title` (its description), then the tags
    /// naming it.
    pub metadata: Metadata,
    /// Where its bytes begin in the file.
    pub(crate) position: u64,
}

/// FFmpeg's limit on a file's streams (`max_streams`): tracks and
/// attachments together.
const MAX_STREAMS: usize = 1000;

/// The lists FFmpeg reads from the Segment's top-level elements, before it
/// makes anything of them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Lists {
    pub seek: Vec<SeekEntry>,
    pub chapters: Vec<RawChapter>,
    pub tags: Vec<RawTags>,
    pub attachments: Vec<RawAttachment>,
}

/// The file's metadata, its chapters and its attachments, made from what was
/// read; each track's metadata set in `tracks`.
pub(crate) struct Described {
    pub metadata: Metadata,
    pub chapters: Vec<Chapter>,
    pub attachments: Vec<Attachment>,
}

/// What `matroska_read_header` makes of the lists, in its order: the file's
/// metadata, the tracks', the attachments, the chapters, then the tags.
pub(crate) fn describe(info: &SegmentInfo, tracks: &mut [Track], lists: &Lists) -> Described {
    let mut budget = Budget::new();
    let mut file = Metadata::default();
    file.set(b"title", info.title.as_deref(), &mut budget);
    file.set(b"encoder", info.muxing_app.as_deref(), &mut budget);
    if let Some(time) = info.date_utc.and_then(creation_time) {
        file.set(b"creation_time", Some(&time), &mut budget);
    }
    for t in tracks.iter_mut() {
        t.metadata = track_metadata(t, &mut budget);
    }

    let mut attachments: Vec<Attachment> = Vec::new();
    for a in &lists.attachments {
        let (Some(name), Some(mime), Some((position, size))) = (&a.name, &a.mime, a.data) else {
            continue;
        };
        if size == 0 {
            continue;
        }
        if tracks.len().saturating_add(attachments.len()) >= MAX_STREAMS {
            break;
        }
        let mut metadata = Metadata::default();
        metadata.set(b"filename", Some(name), &mut budget);
        metadata.set(b"mimetype", Some(mime), &mut budget);
        if let Some(d) = &a.description {
            metadata.set(b"title", Some(d), &mut budget);
        }
        attachments.push(Attachment {
            uid: a.uid,
            name: name.clone(),
            media_type: mime.clone(),
            description: a.description.clone(),
            kind: AttachmentKind::of(mime),
            size,
            metadata,
            position,
        });
    }

    let mut chapters: Vec<Chapter> = Vec::new();
    // Which chapter each atom made, if it made one.
    let mut made: Vec<Option<usize>> = Vec::with_capacity(lists.chapters.len());
    let mut by_uid: HashMap<u64, usize> = HashMap::new();
    let mut max_start = 0u64;
    for c in &lists.chapters {
        let mut chapter = None;
        if c.start != NOPTS && c.uid != 0 && (max_start == 0 || c.start > max_start) {
            chapter = new_chapter(&mut chapters, &mut by_uid, c, &mut budget);
            max_start = c.start;
        }
        made.push(chapter);
    }

    for t in &lists.tags {
        let target = &t.target;
        if target.attachment != 0 {
            for a in attachments
                .iter_mut()
                .filter(|a| a.uid == target.attachment)
            {
                convert_tag(&t.tags, &mut a.metadata, None, &mut budget);
            }
        } else if target.chapter != 0 {
            for (atom, chapter) in lists.chapters.iter().zip(&made) {
                if atom.uid == target.chapter
                    && let Some(c) = chapter.and_then(|i| chapters.get_mut(i))
                {
                    convert_tag(&t.tags, &mut c.metadata, None, &mut budget);
                }
            }
        } else if target.track != 0 {
            for track in tracks.iter_mut().filter(|tr| tr.uid == target.track) {
                convert_tag(&t.tags, &mut track.metadata, None, &mut budget);
            }
        } else {
            convert_tag(&t.tags, &mut file, target.kind.as_deref(), &mut budget);
        }
    }

    Described {
        metadata: file,
        chapters,
        attachments,
    }
}

/// FFmpeg's `avpriv_new_chapter`: none if it ends before it starts; else the
/// chapter of that UID, made if there is none yet, given the atom's times
/// and title.
fn new_chapter(
    chapters: &mut Vec<Chapter>,
    by_uid: &mut HashMap<u64, usize>,
    atom: &RawChapter,
    budget: &mut Budget,
) -> Option<usize> {
    // FFmpeg passes the unsigned times on as signed ones.
    let (start, end) = (atom.start.cast_signed(), atom.end.cast_signed());
    let given = end != NOPTS.cast_signed();
    if given && start > end {
        return None;
    }
    let i = *by_uid.entry(atom.uid).or_insert_with(|| {
        chapters.push(Chapter {
            uid: atom.uid,
            start,
            end: None,
            metadata: Metadata::default(),
        });
        chapters.len().saturating_sub(1)
    });
    let c = chapters.get_mut(i)?;
    c.metadata.set(b"title", atom.title.as_deref(), budget);
    c.start = start;
    c.end = given.then_some(end);
    Some(i)
}

/// FFmpeg's names for `StereoMode`'s values, 0 (mono) to 14.
const STEREO_MODES: [&[u8]; 15] = [
    b"mono",
    b"left_right",
    b"bottom_top",
    b"top_bottom",
    b"checkerboard_rl",
    b"checkerboard_lr",
    b"row_interleaved_rl",
    b"row_interleaved_lr",
    b"col_interleaved_rl",
    b"col_interleaved_lr",
    b"anaglyph_cyan_red",
    b"right_left",
    b"anaglyph_green_magenta",
    b"block_lr",
    b"block_rl",
];

/// A track's own metadata, before its tags, in FFmpeg's order.
fn track_metadata(t: &Track, budget: &mut Budget) -> Metadata {
    let mut m = Metadata::default();
    if let Some(id) = &t.key_id {
        m.set(b"enc_key_id", Some(&base64(id)), budget);
    }
    if t.language != b"und" {
        m.set(b"language", Some(&t.language), budget);
    }
    m.set(b"title", t.name.as_deref(), budget);
    if let Some(v) = &t.video {
        // Mono (0), and a mode FFmpeg has no name for, are not shown.
        if v.stereo_mode != 0
            && let Some(name) = usize::try_from(v.stereo_mode)
                .ok()
                .and_then(|i| STEREO_MODES.get(i))
        {
            m.set(b"stereo_mode", Some(name), budget);
        }
        if v.alpha_mode != 0 {
            m.set(b"alpha_mode", Some(b"1"), budget);
        }
    }
    m
}

/// Bytes in base64, padded, as FFmpeg's `av_base64_encode` writes them.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "each index is a six-bit value, below the 64-entry alphabet"
)]
fn base64(bytes: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

/// `DateUTC` -- nanoseconds from 2001-01-01, UTC -- as FFmpeg writes it as
/// `creation_time`: its microseconds (C's truncating division) from 1970,
/// the whole seconds by C's `gmtime`, and the rest as C's `%06d` prints a
/// remainder that is negative when the time is.
///
/// None before 1969-12-31T12:00:00: the C library of the FFmpeg the
/// fixtures are held to (Windows') refuses a time more than twelve hours
/// before 1970, and FFmpeg then sets none. A `DateUTC` cannot reach the far
/// end of its range (3000).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a DateUTC is an i64 of nanoseconds, so its microseconds from 1970 are within 10^16, far inside i64"
)]
fn creation_time(date_utc: i64) -> Option<Vec<u8>> {
    let micros = date_utc / 1000 + 978_307_200_000_000;
    let seconds = micros / 1_000_000;
    if seconds < -43_200 {
        return None;
    }
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let fraction = micros % 1_000_000;
    Some(
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{fraction:06}Z",
            of_day / 3600,
            of_day / 60 % 60,
            of_day % 60
        )
        .into_bytes(),
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the days here are within a few thousand years of 1970"
)]
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// FFmpeg's `compute_chapters_end`: each chapter's end, its own where the
/// file gives one; else the next chapter's start (in order of start, then
/// UID) if that is later than its own and before the end of the
/// presentation; else that end -- `duration_us` and `start_us`, FFmpeg's
/// duration and start time of the file in microseconds -- or, with neither
/// or one ending before the chapter starts, the chapter's start.
pub(crate) fn chapter_ends(
    chapters: &[Chapter],
    duration_us: Option<i64>,
    start_us: Option<i64>,
) -> Vec<i64> {
    let start = start_us.unwrap_or(0);
    let max_time = match duration_us {
        Some(d) if d > 0 && start_us.is_none_or(|s| s < i64::MAX.saturating_sub(d)) => {
            d.saturating_add(start)
        }
        _ => 0,
    };
    let mut order: Vec<usize> = (0..chapters.len()).collect();
    let uid = |i: usize| chapters.get(i).map_or(0, |c| c.uid.cast_signed());
    let start_of = |i: usize| chapters.get(i).map_or(0, |c| c.start);
    order.sort_by(|&a, &b| start_of(a).cmp(&start_of(b)).then(uid(a).cmp(&uid(b))));
    let mut ends: Vec<i64> = chapters.iter().map(|c| c.end.unwrap_or(i64::MIN)).collect();
    for (k, &i) in order.iter().enumerate() {
        let Some(c) = chapters.get(i) else { continue };
        if c.end.is_some() {
            continue;
        }
        // av_rescale_q to nanoseconds: INT64_MIN when it overflows.
        let mut end = if max_time == 0 {
            i64::MAX
        } else {
            i128::from(max_time)
                .checked_mul(1000)
                .and_then(|ns| i64::try_from(ns).ok())
                .unwrap_or(i64::MIN)
        };
        if let Some(next) = order.get(k.saturating_add(1)).map(|&j| start_of(j))
            && next > c.start
            && next < end
        {
            end = next;
        }
        if let Some(e) = ends.get_mut(i) {
            *e = if end == i64::MAX || end < c.start {
                c.start
            } else {
                end
            };
        }
    }
    ends
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    fn entries(m: &Metadata) -> Vec<(String, String)> {
        m.iter()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_vec()).unwrap(),
                    String::from_utf8(v.to_vec()).unwrap(),
                )
            })
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn setting_a_key_moves_the_last_entry_into_its_place() {
        let mut b = Budget::new();
        let mut m = Metadata::default();
        for (k, v) in [("a", "1"), ("b", "2"), ("c", "3")] {
            m.set(k.as_bytes(), Some(v.as_bytes()), &mut b);
        }
        m.set(b"A", Some(b"4"), &mut b);
        assert_eq!(entries(&m), pairs(&[("c", "3"), ("b", "2"), ("A", "4")]));
        m.set(b"c", None, &mut b);
        assert_eq!(entries(&m), pairs(&[("A", "4"), ("b", "2")]), "removed");
        assert_eq!(m.get(b"a"), Some(&b"4"[..]));
    }

    #[test]
    fn renaming_keeps_both_of_two_values_and_drops_a_repeat() {
        let mut b = Budget::new();
        let mut m = Metadata::default();
        m.set(b"track", Some(b"5"), &mut b);
        m.set(b"PART_NUMBER", Some(b"7"), &mut b);
        m.set(b"lead_performer", Some(b"x"), &mut b);
        m.rename(&mut b);
        assert_eq!(
            entries(&m),
            pairs(&[("track", "5"), ("track", "7"), ("performer", "x")])
        );
        // Another PART_NUMBER of a value already there: dropped as a repeat.
        m.set(b"PART_NUMBER", Some(b"5"), &mut b);
        m.rename(&mut b);
        assert_eq!(
            entries(&m),
            pairs(&[("track", "5"), ("track", "7"), ("performer", "x")])
        );
        // Setting "track" replaces the first of the two.
        m.set(b"track", Some(b"9"), &mut b);
        assert_eq!(
            entries(&m),
            pairs(&[("performer", "x"), ("track", "7"), ("track", "9")])
        );
    }

    fn tag(name: &str, value: Option<&str>, lang: &str, default: u64) -> RawTag {
        RawTag {
            name: Some(name.as_bytes().to_vec()),
            string: value.map(|v| v.as_bytes().to_vec()),
            lang: Some(lang.as_bytes().to_vec()),
            default,
            sub: Vec::new(),
        }
    }

    #[test]
    fn a_tag_is_keyed_by_language_prefix_and_nesting() {
        let mut b = Budget::new();
        let mut m = Metadata::default();
        let mut outer = tag("ARTIST", Some("Someone"), "und", 0);
        outer
            .sub
            .push(tag("SORT_WITH", Some("One, Some"), "und", 0));
        let tags = [
            outer,
            tag("TITLE", Some("Titre"), "fre", 0),
            tag("TITLE", Some("Title"), "eng", 1),
            tag("GONE", None, "und", 0),
        ];
        convert_tag(&tags, &mut m, Some(b"ALBUM"), &mut b);
        assert_eq!(
            entries(&m),
            pairs(&[
                ("ALBUM/ARTIST", "Someone"),
                ("ALBUM/ARTIST/SORT_WITH", "One, Some"),
                ("ALBUM/TITLE-fre", "Titre"),
                ("ALBUM/TITLE", "Title"),
                ("ALBUM/TITLE-eng", "Title"),
            ])
        );
    }

    #[test]
    fn a_key_stops_at_ffmpeg_s_buffer() {
        let mut b = Budget::new();
        let mut m = Metadata::default();
        let long = "N".repeat(2000);
        convert_tag(&[tag(&long, Some("v"), "fre", 0)], &mut m, None, &mut b);
        let (k, _) = m.iter().next().unwrap();
        assert_eq!(k.len(), 1023, "the language does not fit after it");
    }

    #[test]
    fn the_budget_cuts_off_what_would_take_hours() {
        // Each change costs the entries it looks at, so n keys cost about
        // n^2 / 2: a budget of 2 * 10^6 runs out near 2,000.
        let mut b = Budget::of(2_000_000);
        let mut m = Metadata::default();
        let tags: Vec<RawTag> = (0..4_000)
            .map(|i| tag(&format!("K{i}"), Some("v"), "und", 0))
            .collect();
        convert_tag(&tags, &mut m, None, &mut b);
        assert!(m.len() > 1_500 && m.len() < 2_500, "{}", m.len());
        // Spent: nothing more is set, not even a short key.
        m.set(b"late", Some(b"v"), &mut b);
        assert_eq!(m.get(b"late"), None);
        // The real budget holds far more than any real file's tags.
        let mut b = Budget::new();
        let mut m = Metadata::default();
        convert_tag(&tags, &mut m, None, &mut b);
        assert_eq!(m.len(), 4_000);
    }

    #[test]
    fn creation_time_is_ffmpeg_s_down_to_its_quirks() {
        let at = |secs_from_1970: i64, extra_ns: i64| {
            let ns = (secs_from_1970 - 978_307_200) * 1_000_000_000 + extra_ns;
            creation_time(ns).map(|v| String::from_utf8(v).unwrap())
        };
        // Answers from ffprobe (gyan.dev 2026-03-09).
        assert_eq!(at(0, 0).as_deref(), Some("1970-01-01T00:00:00.000000Z"));
        assert_eq!(at(-1, 0).as_deref(), Some("1969-12-31T23:59:59.000000Z"));
        assert_eq!(
            at(-43_200, 0).as_deref(),
            Some("1969-12-31T12:00:00.000000Z")
        );
        assert_eq!(at(-43_201, 0), None);
        assert_eq!(at(0, -1000).as_deref(), Some("1970-01-01T00:00:00.-00001Z"));
        assert_eq!(
            creation_time(-1)
                .map(|v| String::from_utf8(v).unwrap())
                .as_deref(),
            Some("2001-01-01T00:00:00.000000Z"),
            "a nanosecond before 2001 truncates to it"
        );
        assert_eq!(
            creation_time(-123_456_789)
                .map(|v| String::from_utf8(v).unwrap())
                .as_deref(),
            Some("2000-12-31T23:59:59.876544Z")
        );
        assert_eq!(
            creation_time(i64::MAX)
                .map(|v| String::from_utf8(v).unwrap())
                .as_deref(),
            Some("2293-04-11T23:47:16.854775Z")
        );
        assert_eq!(creation_time(i64::MIN), None);
        assert_eq!(
            at(951_782_400, 0).as_deref(),
            Some("2000-02-29T00:00:00.000000Z")
        );
    }

    #[test]
    fn base64_pads_as_ffmpeg_does() {
        assert_eq!(base64(b""), b"");
        assert_eq!(base64(b"f"), b"Zg==");
        assert_eq!(base64(b"fo"), b"Zm8=");
        assert_eq!(base64(b"foo"), b"Zm9v");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd, 0xfc]), b"//79/A==");
    }

    #[test]
    fn a_media_type_is_known_by_its_start() {
        assert_eq!(AttachmentKind::of(b"image/jpeg; q=1"), AttachmentKind::Jpeg);
        assert_eq!(
            AttachmentKind::of(b"application/x-font-ttf"),
            AttachmentKind::TrueType
        );
        assert_eq!(
            AttachmentKind::of(b"Image/png"),
            AttachmentKind::Other,
            "case counts"
        );
        assert_eq!(AttachmentKind::of(b"font/ttf"), AttachmentKind::Other);
    }

    fn chapter(uid: u64, start: i64, end: Option<i64>) -> Chapter {
        Chapter {
            uid,
            start,
            end,
            metadata: Metadata::default(),
        }
    }

    #[test]
    fn a_chapter_without_an_end_ends_where_the_next_starts_or_the_file_ends() {
        let cs = [
            chapter(3, 2_000_000_000, None),
            chapter(1, 0, None),
            chapter(2, 1_000_000_000, Some(1_500_000_000)),
            chapter(4, 2_000_000_000, None),
        ];
        // Five seconds long, from 0.
        assert_eq!(
            chapter_ends(&cs, Some(5_000_000), Some(0)),
            [5_000_000_000, 1_000_000_000, 1_500_000_000, 5_000_000_000],
            "the third: its own; the first two at the same start do not end each other"
        );
        // No duration: a chapter with nothing after it ends where it starts.
        assert_eq!(
            chapter_ends(&cs, None, None),
            [2_000_000_000, 1_000_000_000, 1_500_000_000, 2_000_000_000]
        );
    }
}
