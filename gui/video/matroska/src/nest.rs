//! The Segment's top-level elements that hold lists -- SeekHead, Chapters,
//! Tags and Attachments -- read as FFmpeg's `ebml_parse` reads them,
//! damage included.
//!
//! FFmpeg reads these into lists as it goes, and an error partway does not
//! take back what was read before it: the chapters, tags and attachments
//! before the damage stay, and so does the one being read, as far as it
//! got. Which of its fields that leaves set, and to what, is FFmpeg's:
//!
//! - a list element gets its place -- zeroed, before its defaults are set --
//!   as soon as its ID is read, so one whose size or depth is refused stays
//!   in the list with nothing in it;
//! - a number cut short by the end of the file is kept as far as it was
//!   read, the missing bytes as zeros (`avio_r8` gives 0 at the end);
//! - a string cut short is not kept, and the field keeps what it had;
//! - bytes cut short (an attachment's data) leave the field empty;
//! - a master element may run past the end of the file -- its children are
//!   read until one cannot be -- but not past its parent.
//!
//! The sizes FFmpeg allows by type are kept: 8 bytes for a number, 16 MiB
//! for a string, 256 MiB for bytes, and master elements 16 deep, the
//! Segment counted (`EBML_MAX_DEPTH`).

use std::io::{Read, Seek};

use crate::ebml::{Header, Id, MAX_BINARY, MAX_STRING, Reader, Size, vint_len};
use crate::{Error, ids};

/// How deep master elements nest at most, the Segment counted: FFmpeg's
/// `EBML_MAX_DEPTH`.
const MAX_LEVELS: u32 = 16;

/// FFmpeg's `AV_NOPTS_VALUE` as the unsigned number a chapter's times are
/// read into: a chapter's time not given.
pub(crate) const NOPTS: u64 = 1 << 63;

/// A child whose header could not be read: its ID, when that much was --
/// FFmpeg has then already given it its place in a list.
pub(crate) struct Broken {
    pub id: Option<Id>,
    pub error: Error,
}

impl From<Error> for Broken {
    fn from(error: Error) -> Self {
        Self { id: None, error }
    }
}

/// One byte, or `None` at the end of the source.
fn byte<R: Read + Seek>(r: &mut Reader<R>) -> Result<Option<u8>, Error> {
    if r.remaining() == 0 {
        return Ok(None);
    }
    let mut b = [0u8];
    r.read_into(&mut b)?;
    Ok(Some(b[0]))
}

/// A variable-size integer of at most `max` bytes, as FFmpeg's
/// `ebml_read_num` reads one: its length and value, the marker kept or not.
fn number<R: Read + Seek>(
    r: &mut Reader<R>,
    max: u32,
    keep_marker: bool,
) -> Result<(u32, u64), Error> {
    let first = byte(r)?.ok_or(Error::Truncated)?;
    let len = vint_len(first)
        .filter(|&n| n <= max)
        .ok_or(Error::Invalid("an EBML number's first byte"))?;
    let marker = 0x80u8.checked_shr(len.saturating_sub(1)).unwrap_or(0);
    let mut value = u64::from(if keep_marker { first } else { first & !marker });
    for _ in 1..len {
        value = (value << 8) | u64::from(byte(r)?.ok_or(Error::Truncated)?);
    }
    Ok((len, value))
}

/// An element's ID where reading is, as FFmpeg's `ebml_parse` reads one:
/// one to four bytes, whatever their value (FFmpeg reads a reserved ID as
/// one it does not know).
pub(crate) fn read_id<R: Read + Seek>(r: &mut Reader<R>) -> Result<Id, Error> {
    let (_, id) = number(r, 4, true)?;
    Id::try_from(id).map_err(|_| Error::Invalid("an EBML ID"))
}

/// An element's size where reading is: one to eight bytes, all value bits
/// set meaning unknown.
pub(crate) fn read_size<R: Read + Seek>(r: &mut Reader<R>) -> Result<Size, Error> {
    let (len, size) = number(r, 8, false)?;
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a size is 1 to 8 bytes, so its value bits are 7 to 56: the shift is in range"
    )]
    let unknown = size == (1u64 << (7 * len)) - 1;
    Ok(if unknown {
        Size::Unknown
    } else {
        Size::Known(size)
    })
}

/// The next child of a master element ending at `end`: `None` at its end.
///
/// As FFmpeg's `ebml_parse` reads one: an ID, then a size, then the checks
/// against the parent -- a child of unknown size, or one running past its
/// parent, is refused. A failure after the ID carries the ID.
pub(crate) fn child<R: Read + Seek>(r: &mut Reader<R>, end: u64) -> Result<Option<Header>, Broken> {
    let start = r.pos();
    if start >= end {
        return Ok(None);
    }
    let id = read_id(r)?;
    let with_id = |error| Broken {
        id: Some(id),
        error,
    };
    let size = read_size(r).map_err(with_id)?;
    let data = r.pos();
    let Size::Known(n) = size else {
        return Err(with_id(Error::Invalid(
            "an element of unknown size inside a sized one",
        )));
    };
    match data.checked_add(n) {
        Some(child_end) if child_end <= end => Ok(Some(Header {
            id,
            size,
            start,
            data,
        })),
        _ => Err(with_id(Error::Invalid(
            "an element running past its parent",
        ))),
    }
}

/// Into a master element one level below `levels`: the level count inside
/// it, or FFmpeg's refusal past its depth.
pub(crate) fn enter(levels: u32) -> Result<u32, Error> {
    if levels >= MAX_LEVELS {
        return Err(Error::Unsupported("elements nested more than 16 deep"));
    }
    Ok(levels.saturating_add(1))
}

/// The size of an element `child` has read.
fn size_of(h: &Header) -> u64 {
    match h.size {
        Size::Known(n) => n,
        Size::Unknown => u64::MAX,
    }
}

/// Where an element ends; the elements here are all of known size.
pub(crate) fn end_of(h: &Header) -> Result<u64, Error> {
    h.end()
        .ok_or(Error::Invalid("a master element of unknown size"))
}

/// An unsigned number into `out`, as FFmpeg's `ebml_read_uint`: `default`
/// if empty; refused, `out` untouched, if longer than 8 bytes; and if the
/// file ends inside it, what was read, the missing bytes zeros -- kept, and
/// an error.
pub(crate) fn uint<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    default: u64,
    out: &mut u64,
) -> Result<(), Error> {
    let n = size_of(h);
    if n > 8 {
        return Err(Error::Invalid("an unsigned integer longer than 8 bytes"));
    }
    if n == 0 {
        *out = default;
        return Ok(());
    }
    let mut v = 0u64;
    let mut short = false;
    for _ in 0..n {
        let b = match byte(r)? {
            Some(b) => b,
            None => {
                short = true;
                0
            }
        };
        v = (v << 8) | u64::from(b);
    }
    *out = v;
    if short {
        return Err(Error::Truncated);
    }
    Ok(())
}

/// A string into `out`, as FFmpeg's `ebml_read_ascii`: `default` if empty
/// and there is one, else the bytes up to the first NUL (so an empty
/// element is an empty string, not none); refused past 16 MiB, and not
/// kept if the file ends inside it.
fn string<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    default: Option<&[u8]>,
    out: &mut Option<Vec<u8>>,
) -> Result<(), Error> {
    let n = size_of(h);
    if n > MAX_STRING {
        return Err(Error::Invalid("a string over 16 MiB"));
    }
    if n == 0
        && let Some(d) = default
    {
        *out = Some(d.to_vec());
        return Ok(());
    }
    if n > r.remaining() {
        return Err(Error::Truncated);
    }
    let mut s = vec![0u8; usize::try_from(n).map_err(|_| Error::Truncated)?];
    r.read_into(&mut s)?;
    if let Some(nul) = s.iter().position(|&b| b == 0) {
        s.truncate(nul);
    }
    *out = Some(s);
    Ok(())
}

/// Where a binary element's bytes are, into `out`, as FFmpeg's
/// `ebml_read_binary` reads them (but not read yet): refused past 256 MiB,
/// `out` untouched; cleared if the file ends inside it.
fn binary_place<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    out: &mut Option<(u64, u64)>,
) -> Result<(), Error> {
    let n = size_of(h);
    if n > MAX_BINARY {
        return Err(Error::Invalid("a binary element too large"));
    }
    if n > r.remaining() {
        *out = None;
        return Err(Error::Truncated);
    }
    *out = Some((h.data, n));
    r.seek_to(h.data.saturating_add(n))
}

/// Pass over an element FFmpeg does not read here: refused if it runs past
/// the end of the file, or is larger than C's `int` holds (FFmpeg's
/// `ffio_limit` takes one).
pub(crate) fn skip<R: Read + Seek>(r: &mut Reader<R>, h: &Header) -> Result<(), Error> {
    let n = size_of(h);
    if n > r.remaining() || n > u64::from(i32::MAX.unsigned_abs()) {
        return Err(Error::Truncated);
    }
    r.seek_to(h.data.saturating_add(n))
}

/// Pass over an element FFmpeg does not know, where a SeekHead points: as
/// [`skip`], and refused if of unknown size.
pub(crate) fn skip_any<R: Read + Seek>(r: &mut Reader<R>, h: &Header) -> Result<(), Error> {
    if h.size == Size::Unknown {
        return Err(Error::Invalid(
            "an element of unknown size FFmpeg does not know",
        ));
    }
    skip(r, h)
}

/// One `Seek` entry as FFmpeg reads it: the element's ID as a number of up
/// to eight bytes, and its position from the Segment's data, all ones when
/// the entry gives none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SeekEntry {
    pub id: u64,
    pub pos: u64,
}

/// The most SeekHead entries kept, chained SeekHeads' together: some 16 MiB
/// of them, against a hostile file. Entries naming a Cluster, which FFmpeg
/// passes over, are not kept, so a SeekHead listing every Cluster of a long
/// file does not crowd out the rest.
const MAX_SEEK_ENTRIES: usize = 1 << 20;

/// A SeekHead's entries, appended to `out`.
///
/// FFmpeg makes room for an entry as soon as its ID is read; an entry left
/// without its ID or position is passed over when the entries are
/// followed, so only those with something in them are kept here.
///
/// # Errors
///
/// When it is damaged; the entries read before stay in `out`, and the one
/// being read as far as it got.
pub(crate) fn read_seek_head<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    levels: u32,
    out: &mut Vec<SeekEntry>,
) -> Result<(), Error> {
    let levels = enter(levels)?;
    let end = end_of(h)?;
    r.seek_to(h.data)?;
    while let Some(c) = child(r, end).map_err(|b| b.error)? {
        if c.id != ids::SEEK {
            skip(r, &c)?;
            continue;
        }
        enter(levels)?;
        let mut e = SeekEntry {
            id: 0,
            pos: u64::MAX,
        };
        let read = read_seek(r, &c, &mut e);
        if out.len() < MAX_SEEK_ENTRIES && e.id != u64::from(ids::CLUSTER) {
            out.push(e);
        }
        read?;
    }
    Ok(())
}

fn read_seek<R: Read + Seek>(
    r: &mut Reader<R>,
    c: &Header,
    e: &mut SeekEntry,
) -> Result<(), Error> {
    let end = end_of(c)?;
    while let Some(f) = child(r, end).map_err(|b| b.error)? {
        match f.id {
            ids::SEEK_ID => uint(r, &f, 0, &mut e.id)?,
            ids::SEEK_POSITION => uint(r, &f, u64::MAX, &mut e.pos)?,
            _ => skip(r, &f)?,
        }
    }
    Ok(())
}

/// One `ChapterAtom` as FFmpeg's `MatroskaChapter` holds it: its times
/// [`NOPTS`] when not given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawChapter {
    pub start: u64,
    pub end: u64,
    pub uid: u64,
    /// The last `ChapString` of its `ChapterDisplay`s.
    pub title: Option<Vec<u8>>,
}

/// A Chapters element's atoms -- every edition's top-level ones, in order;
/// nested atoms FFmpeg does not read -- appended to `out`.
///
/// # Errors
///
/// When it is damaged; the atoms read before stay in `out`, and the one
/// being read as far as it got.
pub(crate) fn read_chapters<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    levels: u32,
    out: &mut Vec<RawChapter>,
) -> Result<(), Error> {
    let levels = enter(levels)?;
    let end = end_of(h)?;
    r.seek_to(h.data)?;
    while let Some(edition) = child(r, end).map_err(|b| b.error)? {
        if edition.id != ids::EDITION_ENTRY {
            skip(r, &edition)?;
            continue;
        }
        let in_edition = enter(levels)?;
        let edition_end = end_of(&edition)?;
        loop {
            let atom = match child(r, edition_end) {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(b) => {
                    if b.id == Some(ids::CHAPTER_ATOM) {
                        out.push(RawChapter::default());
                    }
                    return Err(b.error);
                }
            };
            if atom.id != ids::CHAPTER_ATOM {
                skip(r, &atom)?;
                continue;
            }
            out.push(RawChapter::default());
            let Some(c) = out.last_mut() else {
                return Ok(());
            };
            read_atom(r, &atom, in_edition, c)?;
        }
    }
    Ok(())
}

fn read_atom<R: Read + Seek>(
    r: &mut Reader<R>,
    atom: &Header,
    levels: u32,
    c: &mut RawChapter,
) -> Result<(), Error> {
    let levels = enter(levels)?;
    c.start = NOPTS;
    c.end = NOPTS;
    c.uid = 0;
    let end = end_of(atom)?;
    while let Some(f) = child(r, end).map_err(|b| b.error)? {
        match f.id {
            ids::CHAPTER_TIME_START => uint(r, &f, NOPTS, &mut c.start)?,
            ids::CHAPTER_TIME_END => uint(r, &f, NOPTS, &mut c.end)?,
            ids::CHAPTER_UID => uint(r, &f, 0, &mut c.uid)?,
            ids::CHAPTER_DISPLAY => {
                enter(levels)?;
                let display_end = end_of(&f)?;
                while let Some(d) = child(r, display_end).map_err(|b| b.error)? {
                    if d.id == ids::CHAP_STRING {
                        string(r, &d, None, &mut c.title)?;
                    } else {
                        skip(r, &d)?;
                    }
                }
            }
            // A nested atom too: FFmpeg reads only the top level.
            _ => skip(r, &f)?,
        }
    }
    Ok(())
}

/// One `SimpleTag` as FFmpeg's `MatroskaTag` holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawTag {
    pub name: Option<Vec<u8>>,
    pub string: Option<Vec<u8>>,
    /// `und` unless the tag says; none only in a tag left zeroed.
    pub lang: Option<Vec<u8>>,
    /// `TagDefault` (or the misspelt ID some writers used), 0 if not given
    /// -- FFmpeg's default, where the specification's is 1.
    pub default: u64,
    pub sub: Vec<RawTag>,
}

/// A `Tag`'s `Targets`, as far as FFmpeg reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawTarget {
    /// `TargetType`: the prefix of the keys of a tag on the file.
    pub kind: Option<Vec<u8>>,
    pub track: u64,
    pub chapter: u64,
    pub attachment: u64,
}

/// One `Tag`: whom it is for, and its tags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawTags {
    pub target: RawTarget,
    pub tags: Vec<RawTag>,
}

/// A Tags element's `Tag`s, appended to `out`.
///
/// # Errors
///
/// When it is damaged; what was read before stays in `out`.
pub(crate) fn read_tags<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    levels: u32,
    out: &mut Vec<RawTags>,
) -> Result<(), Error> {
    let levels = enter(levels)?;
    let end = end_of(h)?;
    r.seek_to(h.data)?;
    loop {
        let t = match child(r, end) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(()),
            Err(b) => {
                if b.id == Some(ids::TAG) {
                    out.push(RawTags::default());
                }
                return Err(b.error);
            }
        };
        if t.id != ids::TAG {
            skip(r, &t)?;
            continue;
        }
        out.push(RawTags::default());
        let Some(tag) = out.last_mut() else {
            return Ok(());
        };
        let in_tag = enter(levels)?;
        let tag_end = end_of(&t)?;
        loop {
            let c = match child(r, tag_end) {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(b) => {
                    if b.id == Some(ids::SIMPLE_TAG) {
                        tag.tags.push(RawTag::default());
                    }
                    return Err(b.error);
                }
            };
            match c.id {
                ids::SIMPLE_TAG => {
                    tag.tags.push(RawTag::default());
                    let Some(simple) = tag.tags.last_mut() else {
                        return Ok(());
                    };
                    read_simple_tag(r, &c, in_tag, simple)?;
                }
                ids::TARGETS => read_targets(r, &c, in_tag, &mut tag.target)?,
                _ => skip(r, &c)?,
            }
        }
    }
}

fn read_targets<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    levels: u32,
    target: &mut RawTarget,
) -> Result<(), Error> {
    enter(levels)?;
    // The numbers take their defaults again in each Targets; the type keeps
    // the last one given.
    let mut type_value = 50;
    target.track = 0;
    target.chapter = 0;
    target.attachment = 0;
    let end = end_of(h)?;
    while let Some(f) = child(r, end).map_err(|b| b.error)? {
        match f.id {
            ids::TARGET_TYPE => string(r, &f, None, &mut target.kind)?,
            // Read for its checks; FFmpeg acts on it nowhere.
            ids::TARGET_TYPE_VALUE => uint(r, &f, 50, &mut type_value)?,
            ids::TAG_TRACK_UID => uint(r, &f, 0, &mut target.track)?,
            ids::TAG_CHAPTER_UID => uint(r, &f, 0, &mut target.chapter)?,
            ids::TAG_ATTACHMENT_UID => uint(r, &f, 0, &mut target.attachment)?,
            _ => skip(r, &f)?,
        }
    }
    Ok(())
}

fn read_simple_tag<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    levels: u32,
    tag: &mut RawTag,
) -> Result<(), Error> {
    let levels = enter(levels)?;
    tag.lang = Some(b"und".to_vec());
    tag.default = 0;
    let end = end_of(h)?;
    loop {
        let f = match child(r, end) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(()),
            Err(b) => {
                if b.id == Some(ids::SIMPLE_TAG) {
                    tag.sub.push(RawTag::default());
                }
                return Err(b.error);
            }
        };
        match f.id {
            ids::TAG_NAME => string(r, &f, None, &mut tag.name)?,
            ids::TAG_STRING => string(r, &f, None, &mut tag.string)?,
            ids::TAG_LANGUAGE => string(r, &f, Some(b"und"), &mut tag.lang)?,
            ids::TAG_DEFAULT | ids::TAG_DEFAULT_BOGUS => uint(r, &f, 0, &mut tag.default)?,
            ids::SIMPLE_TAG => {
                tag.sub.push(RawTag::default());
                let Some(sub) = tag.sub.last_mut() else {
                    return Ok(());
                };
                read_simple_tag(r, &f, levels, sub)?;
            }
            _ => skip(r, &f)?,
        }
    }
}

/// One `AttachedFile` as FFmpeg's `MatroskaAttachment` holds it, its data
/// where it is in the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawAttachment {
    pub uid: u64,
    pub name: Option<Vec<u8>>,
    pub mime: Option<Vec<u8>>,
    pub description: Option<Vec<u8>>,
    /// Where `FileData`'s bytes begin, and how many.
    pub data: Option<(u64, u64)>,
}

/// An Attachments element's files, appended to `out`.
///
/// # Errors
///
/// When it is damaged; what was read before stays in `out`.
pub(crate) fn read_attachments<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    levels: u32,
    out: &mut Vec<RawAttachment>,
) -> Result<(), Error> {
    let levels = enter(levels)?;
    let end = end_of(h)?;
    r.seek_to(h.data)?;
    loop {
        let f = match child(r, end) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(()),
            Err(b) => {
                if b.id == Some(ids::ATTACHED_FILE) {
                    out.push(RawAttachment::default());
                }
                return Err(b.error);
            }
        };
        if f.id != ids::ATTACHED_FILE {
            skip(r, &f)?;
            continue;
        }
        out.push(RawAttachment::default());
        let Some(a) = out.last_mut() else {
            return Ok(());
        };
        enter(levels)?;
        a.uid = 0;
        let file_end = end_of(&f)?;
        while let Some(c) = child(r, file_end).map_err(|b| b.error)? {
            match c.id {
                ids::FILE_UID => uint(r, &c, 0, &mut a.uid)?,
                ids::FILE_NAME => string(r, &c, None, &mut a.name)?,
                ids::FILE_MEDIA_TYPE => string(r, &c, None, &mut a.mime)?,
                ids::FILE_DATA => binary_place(r, &c, &mut a.data)?,
                ids::FILE_DESCRIPTION => string(r, &c, None, &mut a.description)?,
                _ => skip(r, &c)?,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use std::io::Cursor;

    fn reader(bytes: &[u8]) -> Reader<Cursor<Vec<u8>>> {
        Reader::new(Cursor::new(bytes.to_vec())).unwrap()
    }

    /// An element: ID bytes, a one-byte size, the payload.
    fn el(id: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_vec();
        v.push(0x80 | u8::try_from(payload.len()).unwrap());
        v.extend_from_slice(payload);
        v
    }

    fn top(bytes: &[u8]) -> (Reader<Cursor<Vec<u8>>>, Header) {
        let mut r = reader(bytes);
        let h = child(&mut r, u64::MAX)
            .map_err(|b| b.error)
            .unwrap()
            .unwrap();
        (r, h)
    }

    #[test]
    fn a_reserved_id_is_read_as_any_other() {
        // 0xFF: a one-byte ID whose value bits are all ones, which FFmpeg
        // reads as an element it does not know.
        let mut r = reader(&[0xff, 0x81, 0]);
        let h = child(&mut r, 3).map_err(|b| b.error).unwrap().unwrap();
        assert_eq!((h.id, h.size, h.data), (0xff, Size::Known(1), 2));
    }

    #[test]
    fn a_child_past_its_parent_is_refused_with_its_id() {
        let mut r = reader(&[0xb6, 0x85, 0, 0]);
        let b = child(&mut r, 4).err().unwrap();
        assert_eq!(b.id, Some(0xb6));
        let mut r = reader(&[0xb6, 0xff]);
        assert_eq!(
            child(&mut r, 2).err().unwrap().id,
            Some(0xb6),
            "unknown size"
        );
        let mut r = reader(&[0x00]);
        assert_eq!(child(&mut r, 1).err().unwrap().id, None, "no ID");
    }

    #[test]
    fn a_number_cut_short_keeps_what_was_read() {
        // A ChapterUID of four bytes of which two are in the file.
        let (mut r, h) = top(&[0x73, 0xc4, 0x84, 0x12, 0x34]);
        let mut v = 7;
        assert_eq!(uint(&mut r, &h, 0, &mut v), Err(Error::Truncated));
        assert_eq!(v, 0x1234_0000);
        // Longer than 8 bytes: refused, untouched.
        let (mut r, h) = top(&[0x73, 0xc4, 0x89, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let mut v = 7;
        assert!(uint(&mut r, &h, 0, &mut v).is_err());
        assert_eq!(v, 7);
    }

    #[test]
    fn a_string_is_empty_when_empty_and_untouched_when_cut() {
        let (mut r, h) = top(&[0x85, 0x80]);
        let mut s = Some(b"before".to_vec());
        string(&mut r, &h, None, &mut s).unwrap();
        assert_eq!(
            s.as_deref(),
            Some(&b""[..]),
            "an empty element: an empty string"
        );
        let (mut r, h) = top(&[0x44, 0x7a, 0x80]);
        string(&mut r, &h, Some(b"und"), &mut s).unwrap();
        assert_eq!(s.as_deref(), Some(&b"und"[..]), "empty, with a default");
        let (mut r, h) = top(&[0x85, 0x84, b'a', b'b']);
        let mut s = Some(b"before".to_vec());
        assert_eq!(string(&mut r, &h, None, &mut s), Err(Error::Truncated));
        assert_eq!(s.as_deref(), Some(&b"before"[..]));
        let (mut r, h) = top(&[0x85, 0x83, b'a', 0, b'b']);
        string(&mut r, &h, None, &mut s).unwrap();
        assert_eq!(s.as_deref(), Some(&b"a"[..]), "up to its first NUL");
    }

    #[test]
    fn data_cut_short_is_no_data() {
        let (mut r, h) = top(&[0x46, 0x5c, 0x84, 1, 2]);
        let mut d = Some((99, 99));
        assert_eq!(binary_place(&mut r, &h, &mut d), Err(Error::Truncated));
        assert_eq!(d, None);
    }

    #[test]
    fn chapters_come_from_every_edition_and_only_their_top_level() {
        let nested = el(&[0xb6], &el(&[0x73, 0xc4], &[9]));
        let mut atom1 = el(&[0x73, 0xc4], &[1]);
        atom1.extend(el(&[0x91], &[5]));
        atom1.extend(el(&[0x80], &el(&[0x85], b"one")));
        atom1.extend(el(&[0x80], &el(&[0x85], b"uno")));
        atom1.extend(nested);
        let atom2 = el(&[0x73, 0xc4], &[2]);
        let e1 = el(&[0x45, 0xb9], &el(&[0xb6], &atom1));
        let e2 = el(&[0x45, 0xb9], &el(&[0xb6], &atom2));
        let mut body = e1;
        body.extend(e2);
        let (mut r, h) = top(&el(&[0x10, 0x43, 0xa7, 0x70], &body));
        let mut out = Vec::new();
        read_chapters(&mut r, &h, 1, &mut out).unwrap();
        assert_eq!(
            out,
            [
                RawChapter {
                    start: 5,
                    end: NOPTS,
                    uid: 1,
                    title: Some(b"uno".to_vec())
                },
                RawChapter {
                    start: NOPTS,
                    end: NOPTS,
                    uid: 2,
                    title: None
                },
            ]
        );
    }

    #[test]
    fn tags_nest_only_as_deep_as_ffmpeg_reads() {
        // A SimpleTag holding a SimpleTag holding ... `depth` deep.
        let simple = |depth: usize| {
            let mut t = el(&[0x67, 0xc8], &el(&[0x45, 0xa3], b"n"));
            for _ in 1..depth {
                let mut inner = el(&[0x45, 0xa3], b"n");
                inner.extend(t);
                t = el(&[0x67, 0xc8], &inner);
            }
            t
        };
        let tags = |depth| {
            let bytes = simple(depth);
            // Sizes past one byte: build with two-byte sizes.
            let tag = sized(&[0x73, 0x73], &bytes);
            sized(&[0x12, 0x54, 0xc3, 0x67], &tag)
        };
        // Linear: the Segment is level 1, so 13 SimpleTags nest; a 14th is
        // refused (and its place stays, zeroed).
        let (mut r, h) = top(&tags(13));
        let mut out = Vec::new();
        read_tags(&mut r, &h, 1, &mut out).unwrap();
        let (mut r, h) = top(&tags(14));
        let mut out = Vec::new();
        assert!(read_tags(&mut r, &h, 1, &mut out).is_err());
        let mut t = &out[0].tags[0];
        for _ in 1..13 {
            t = &t.sub[0];
        }
        assert_eq!(t.sub, [RawTag::default()], "the 14th, zeroed");
    }

    /// An element with a two-byte size.
    fn sized(id: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_vec();
        let n = u16::try_from(payload.len()).unwrap();
        v.extend_from_slice(&(0x4000 | n).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }
}
