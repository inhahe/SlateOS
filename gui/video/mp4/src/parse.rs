//! Reading the boxes: FFmpeg's walk through them (`mov_read_default`), and
//! what each box this reads says (`mov_read_*`), damage handled as FFmpeg
//! handles it -- a box claiming more than its parent holds is cut to the
//! parent, a reader that stops short has the rest skipped, one that reads
//! on into the next box is put back at its end.

use std::io::{Read, Seek};

use crate::Error;
use crate::index::{DISCARD, Edit, Entry, KEYFRAME, Stream, Stsc, Tts};
use crate::reader::Reader;
use crate::track::{Audio, Codec, Colour, Kind, Video, audio_codec, read_esds, video_codec};

/// A box being read: its type and the size of its body (FFmpeg's
/// `MOVAtom`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Atom {
    pub kind: [u8; 4],
    pub size: i64,
}

const ROOT: [u8; 4] = *b"root";
/// Boxes nested deeper than this are refused, as FFmpeg refuses them.
const MAX_DEPTH: u32 = 10;

/// A track's defaults for its fragments (`trex`).
#[derive(Clone, Copy, Debug, Default)]
struct Trex {
    track_id: u32,
    stsd_id: u32,
    duration: u32,
    size: u32,
    flags: u32,
}

/// The fragment being read (FFmpeg's `MOVFragment`).
#[derive(Clone, Copy, Debug, Default)]
struct Fragment {
    found_tfhd: bool,
    track_id: u32,
    base_data_offset: u64,
    moof_offset: u64,
    implicit_offset: u64,
    stsd_id: u32,
    duration: u32,
    size: u32,
    flags: u32,
}

/// One track's part of the current fragment (`MOVFragmentStreamInfo`).
#[derive(Clone, Copy, Debug)]
struct FragStream {
    id: i64,
    tfdt_dts: Option<i64>,
    next_trun_dts: Option<i64>,
}

/// What the description boxes add to a track beyond its samples: the
/// public description, read as the boxes come.
#[derive(Clone, Debug, Default)]
pub(crate) struct Description {
    pub codec: Codec,
    pub codec_tag: [u8; 4],
    pub config: Vec<u8>,
    pub language: [u8; 3],
    pub default: bool,
    pub tkhd_width: u32,
    pub tkhd_height: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: Option<(u32, u32)>,
    pub colour: Option<Colour>,
    pub channels: u32,
    pub sample_rate: u32,
    pub bits_per_coded_sample: u32,
    pub audio_cid: i16,
    pub stsd_version: u8,
    /// AAC's MPEG-4 object type from `esds`, which may make it MP3.
    pub object_type: Option<u8>,
}

impl Description {
    pub(crate) fn video(&self) -> Video {
        Video {
            width: self.width,
            height: self.height,
            track_width: self.tkhd_width,
            track_height: self.tkhd_height,
            pixel_aspect: self.pixel_aspect,
            colour: self.colour,
        }
    }

    pub(crate) fn audio(&self) -> Audio {
        Audio {
            channels: self.channels,
            sample_rate: self.sample_rate,
        }
    }
}

/// The walk through a file's boxes, and all it learns.
pub(crate) struct Parser<'a, R> {
    pub r: &'a mut Reader<R>,
    /// The movie's timescale (`mvhd`).
    pub time_scale: i32,
    pub isom: bool,
    pub found_moov: bool,
    pub found_mdat: bool,
    /// The second look for `moov`, which finds one hidden in a `free`.
    pub moov_retry: bool,
    /// Whether the edit lists rewrite the index (FFmpeg's
    /// `advanced_editlist`, on unless a fragmented file turns it off).
    pub advanced_editlist: bool,
    pub streams: Vec<Stream>,
    pub descriptions: Vec<Description>,
    /// While inside a `trak`: its stream.
    trak_index: Option<usize>,
    trex: Vec<Trex>,
    fragment: Fragment,
    frag_streams: Vec<FragStream>,
    /// The track `tfhd` named, in `frag_streams`.
    frag_current: Option<usize>,
    depth: u32,
}

impl<'a, R: Read + Seek> Parser<'a, R> {
    pub(crate) fn new(r: &'a mut Reader<R>) -> Self {
        Self {
            r,
            time_scale: 0,
            isom: false,
            found_moov: false,
            found_mdat: false,
            moov_retry: false,
            advanced_editlist: true,
            streams: Vec::new(),
            descriptions: Vec::new(),
            trak_index: None,
            trex: Vec::new(),
            fragment: Fragment::default(),
            frag_streams: Vec::new(),
            frag_current: None,
            depth: 0,
        }
    }

    /// The whole file, as `mov_read_header` reads it: the top-level boxes in
    /// turn, and once more from the start if no `moov` was found (when one
    /// may be hiding in a `free` box).
    pub(crate) fn read_header(&mut self) -> Result<(), Error> {
        let size = i64::try_from(self.r.len()).unwrap_or(i64::MAX);
        loop {
            if self.moov_retry {
                self.r.seek_to(0)?;
            }
            self.walk(Atom { kind: ROOT, size })?;
            if self.found_moov || self.moov_retry {
                break;
            }
            self.moov_retry = true;
        }
        if !self.found_moov {
            return Err(Error::Invalid("no moov box"));
        }
        for (s, d) in self.streams.iter_mut().zip(&self.descriptions) {
            if s.time_scale <= 0 {
                s.time_scale = if self.time_scale > 0 {
                    self.time_scale
                } else {
                    1
                };
            }
            if s.kind == Kind::Audio && d.codec == Codec::Aac {
                s.skip_samples = s.start_pad;
            }
        }
        Ok(())
    }

    /// A container's children, as `mov_read_default` walks them.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "FFmpeg's signed 64-bit size arithmetic: every size is a box's, at most the file's length or a 64-bit size field checked against the parent before use"
    )]
    fn walk(&mut self, atom: Atom) -> Result<(), Error> {
        if self.depth > MAX_DEPTH {
            return Err(Error::Invalid("boxes nested too deeply"));
        }
        self.depth += 1;
        let result = self.walk_children(atom);
        self.depth -= 1;
        result
    }

    #[allow(clippy::arithmetic_side_effects, reason = "as `walk`")]
    fn walk_children(&mut self, atom: Atom) -> Result<(), Error> {
        let parent_size = if atom.size < 0 { i64::MAX } else { atom.size };
        let mut total: i64 = 0;
        while total <= parent_size - 8 {
            if self.r.remaining() < 8 {
                // FFmpeg reads the header regardless and stops at the end
                // of the file.
                self.r.skip(8)?;
                break;
            }
            let mut size = i64::from(self.r.u32()?);
            let mut kind = self.r.fourcc()?;
            if ((kind == *b"free" && self.moov_retry) || kind == *b"hoov") && size >= 8 {
                // A moov hidden in a free or hoov box, as some broken
                // writers leave it.
                let back = self.r.pos();
                if self.r.remaining() >= 8 {
                    self.r.skip(4)?;
                    let inner = self.r.fourcc()?;
                    if inner == *b"mvhd" || inner == *b"cmov" {
                        kind = *b"moov";
                    }
                }
                self.r.seek_to(back)?;
            }
            if atom.kind != ROOT && atom.kind != *b"moov" && (kind == *b"trak" || kind == *b"mdat")
            {
                // "Broken file, trak/mdat not at top-level": this container
                // ends here, and the box is read by its parent.
                let back = self.r.pos().saturating_sub(8);
                self.r.seek_to(back)?;
                return Ok(());
            }
            total += 8;
            if size == 1 && total + 8 <= parent_size {
                let big = self.r.u64()?;
                size = i64::try_from(big).unwrap_or(-1).wrapping_sub(8);
                total += 8;
            }
            if size == 0 {
                size = parent_size - total + 8;
            }
            if size < 0 {
                break;
            }
            size -= 8;
            if size < 0 {
                break;
            }
            size = size.min(parent_size - total);
            let a = Atom { kind, size };
            let start = self.r.pos();
            if !self.dispatch(a)? {
                self.r.skip(u64::try_from(size).unwrap_or(0))?;
            } else {
                // FFmpeg stops reading the top level once it has both the
                // index and the media, if the box just read ends the file.
                let end = i64::try_from(start)
                    .unwrap_or(i64::MAX)
                    .saturating_add(size);
                if self.found_moov
                    && self.found_mdat
                    && u64::try_from(end).is_ok_and(|e| e == self.r.len())
                {
                    return Ok(());
                }
                // A reader that stopped short has the rest skipped; one that
                // read on is put back at the box's end.
                let to = u64::try_from(end).unwrap_or(u64::MAX).min(self.r.len());
                self.r.seek_to(to)?;
            }
            total += size;
        }
        if total < parent_size && parent_size < 0x7ffff {
            let rest = u64::try_from(parent_size - total).unwrap_or(0);
            self.r.skip(rest)?;
        }
        Ok(())
    }

    /// Read a box this knows: `false` if it does not, for the walk to pass
    /// over.
    fn dispatch(&mut self, a: Atom) -> Result<bool, Error> {
        match &a.kind {
            b"dinf" | b"edts" | b"mdia" | b"minf" | b"mvex" | b"stbl" | b"traf" | b"tref"
            | b"udta" | b"sinf" | b"schi" => self.walk(a)?,
            b"ftyp" => self.ftyp(a)?,
            b"moov" => self.moov(a)?,
            b"mdat" => {
                if a.size != 0 {
                    self.found_mdat = true;
                }
            }
            b"mvhd" => self.mvhd()?,
            b"trak" => self.trak(a)?,
            b"tkhd" => self.tkhd()?,
            b"mdhd" => self.mdhd()?,
            b"hdlr" => self.hdlr()?,
            b"elst" => self.elst(a)?,
            b"stsd" => self.stsd(a)?,
            b"stts" => self.stts()?,
            b"ctts" => self.ctts()?,
            b"stsc" => self.stsc(a)?,
            b"stsz" | b"stz2" => self.stsz(a)?,
            b"stco" | b"co64" => self.stco(a)?,
            b"stss" => self.stss()?,
            b"stps" => self.stps()?,
            b"sdtp" => self.sdtp(a)?,
            b"sbgp" => self.sbgp()?,
            b"av1C" | b"avcC" | b"hvcC" | b"vpcC" | b"dOps" | b"dfLa" | b"glbl" => {
                self.config(a)?;
            }
            b"esds" => self.esds(a)?,
            b"colr" => self.colr(a)?,
            b"pasp" => self.pasp()?,
            b"trex" => self.trex()?,
            b"moof" => self.moof(a)?,
            b"tfhd" => self.tfhd()?,
            b"tfdt" => self.tfdt()?,
            b"trun" => self.trun()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn last(&mut self) -> Option<(&mut Stream, &mut Description)> {
        self.streams.last_mut().zip(self.descriptions.last_mut())
    }

    fn ftyp(&mut self, a: Atom) -> Result<(), Error> {
        let brand = self.r.fourcc()?;
        if !self.streams.is_empty() {
            return Ok(());
        }
        if brand != *b"qt  " {
            self.isom = true;
        }
        // The minor version and the compatible brands, read as FFmpeg reads
        // them so that a short box fails as there.
        let rest = a.size.saturating_sub(8);
        if rest < 0 || rest == i64::from(i32::MAX) {
            return Err(Error::Invalid("an ftyp box too short"));
        }
        self.r.u32()?;
        self.r.bytes(u64::try_from(rest).unwrap_or(0))?;
        Ok(())
    }

    fn moov(&mut self, a: Atom) -> Result<(), Error> {
        if self.found_moov {
            // A second moov is passed over.
            return Ok(());
        }
        self.walk(a)?;
        self.found_moov = true;
        Ok(())
    }

    fn mvhd(&mut self) -> Result<(), Error> {
        let version = self.r.u8()?;
        self.r.u24()?;
        self.r.skip(if version == 1 { 16 } else { 8 })?;
        let scale = self.r.u32()?.cast_signed();
        self.time_scale = if scale <= 0 { 1 } else { scale };
        Ok(())
    }

    fn trak(&mut self, a: Atom) -> Result<(), Error> {
        self.streams.push(Stream::new());
        self.descriptions.push(Description::default());
        let index = self.streams.len().saturating_sub(1);
        self.trak_index = Some(index);
        self.walk(a)?;
        self.trak_index = None;
        let advanced = self.advanced_editlist;
        let movie_scale = self.time_scale;
        let Some(sc) = self.streams.get_mut(index) else {
            return Ok(());
        };
        // An stsc naming chunks no stco gives, in a track with no samples,
        // is cleared.
        if sc.chunk_offsets.is_empty() && sc.stts.is_empty() && !sc.stsc.is_empty() {
            sc.stsc.clear();
        }
        match sanity_check(sc) {
            Sanity::Ok => {}
            // Mandatory tables missing: the track stays, without samples.
            Sanity::Missing => return Ok(()),
            Sanity::Contradictory => {
                return Err(Error::Invalid(
                    "an stsc and stco that contradict each other",
                ));
            }
        }
        if sc.time_scale <= 0 {
            sc.time_scale = if movie_scale > 0 { movie_scale } else { 1 };
        }
        // FFmpeg turns its edit list handling off for fragmented files, whose
        // moov has no samples -- for this track and every one after it.
        if sc.stts.is_empty() && advanced {
            self.advanced_editlist = false;
        }
        let (advanced, codec) = (
            self.advanced_editlist,
            self.descriptions
                .get(index)
                .map_or(Codec::Other, |d| d.codec),
        );
        if let Some(sc) = self.streams.get_mut(index) {
            sc.build_index(advanced, movie_scale, codec);
        }
        Ok(())
    }

    fn tkhd(&mut self) -> Result<(), Error> {
        let Some((sc, _)) = self.last() else {
            return Ok(());
        };
        if sc.id != -1 {
            return Err(Error::Invalid("a second tkhd in one trak"));
        }
        let version = self.r.u8()?;
        let flags = self.r.u24()?;
        if let Some((_, d)) = self.last() {
            d.default = flags & 1 != 0;
        }
        self.r.skip(if version == 1 { 16 } else { 8 })?;
        let id = self.r.u32()?.cast_signed();
        self.r.skip(4)?;
        self.r.skip(if version == 1 { 8 } else { 4 })?;
        // reserved (8), layer, alternate group, volume, reserved (8), the
        // display matrix (36).
        self.r.skip(8 + 8 + 36)?;
        let width = self.r.u32()?.cast_signed();
        let height = self.r.u32()?.cast_signed();
        if let Some((sc, d)) = self.last() {
            sc.id = i64::from(id);
            d.tkhd_width = (width >> 16).cast_unsigned();
            d.tkhd_height = (height >> 16).cast_unsigned();
        }
        Ok(())
    }

    fn mdhd(&mut self) -> Result<(), Error> {
        let Some((sc, _)) = self.last() else {
            return Ok(());
        };
        if sc.time_scale != 0 {
            return Err(Error::Invalid("a second mdhd in one trak"));
        }
        let version = self.r.u8()?;
        if version > 1 {
            return Err(Error::Unsupported("an mdhd of a version after 1"));
        }
        self.r.u24()?;
        self.r.skip(if version == 1 { 16 } else { 8 })?;
        let scale = self.r.u32()?.cast_signed();
        let duration = if version == 1 {
            let d = self.r.u64()?;
            if d == u64::MAX { 0 } else { d.cast_signed() }
        } else {
            let d = self.r.u32()?;
            if d == u32::MAX { 0 } else { i64::from(d) }
        };
        let lang = self.r.u16()?;
        if let Some((sc, d)) = self.last() {
            sc.time_scale = if scale <= 0 { 1 } else { scale };
            sc.duration = duration;
            d.language = language(lang);
        }
        Ok(())
    }

    fn hdlr(&mut self) -> Result<(), Error> {
        self.r.u32()?;
        let _component_type = self.r.fourcc()?;
        let subtype = self.r.fourcc()?;
        if self.trak_index.is_none() {
            return Ok(());
        }
        if let Some((sc, _)) = self.last() {
            match &subtype {
                b"vide" => sc.kind = Kind::Video,
                b"soun" => sc.kind = Kind::Audio,
                b"subp" | b"clcp" => sc.kind = Kind::Subtitle,
                _ => {}
            }
        }
        Ok(())
    }

    fn elst(&mut self, a: Atom) -> Result<(), Error> {
        if self.streams.is_empty() {
            return Ok(());
        }
        let version = self.r.u8()?;
        self.r.u24()?;
        let mut count = i64::from(self.r.u32()?);
        let mut left = a.size.saturating_sub(8);
        let entry = if version == 1 { 20 } else { 12 };
        if left != count.saturating_mul(entry) {
            // FFmpeg believes the box's size over its count.
            count = left.checked_div(entry).unwrap_or(0);
        }
        let mut edits = Vec::new();
        let mut i = 0;
        while i < count && left > 0 {
            let (duration, time) = if version == 1 {
                let d = self.r.u64()?.cast_signed();
                let t = self.r.u64()?.cast_signed();
                left = left.saturating_sub(16);
                (d, t)
            } else {
                let d = i64::from(self.r.u32()?);
                let t = i64::from(self.r.u32()?.cast_signed());
                left = left.saturating_sub(8);
                (d, t)
            };
            self.r.u32()?; // the rate
            left = left.saturating_sub(4);
            if duration < 0 {
                return Err(Error::Invalid("an edit of negative duration"));
            }
            edits.push(Edit { duration, time });
            i = i.saturating_add(1);
        }
        if !edits.is_empty()
            && let Some((sc, _)) = self.last()
        {
            sc.edits = edits;
        }
        Ok(())
    }

    fn stts(&mut self) -> Result<(), Error> {
        if self.trak_index.is_none() || self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let entries = self.r.u32()?;
        if u64::from(entries).saturating_mul(8) > self.r.remaining() {
            return Err(Error::Truncated);
        }
        let mut stts = Vec::with_capacity(usize::try_from(entries).unwrap_or(0));
        let (mut current_dts, mut corrected_dts, mut duration, mut total) =
            (0i64, 0i64, 0i64, 0u64);
        for _ in 0..entries {
            let count = self.r.u32()?;
            let mut delta = self.r.u32()?;
            // A delta past FFmpeg's max_stts_delta (2^32 - 2^22) is read as a
            // negative correction written as unsigned, and clipped to 1.
            if delta > MAX_STTS_DELTA {
                let magnitude = delta.cast_signed();
                delta = 1;
                let step = if magnitude < 0 {
                    i64::from(magnitude)
                } else {
                    1
                };
                corrected_dts = corrected_dts.saturating_add(step.saturating_mul(i64::from(count)));
            } else {
                corrected_dts =
                    corrected_dts.wrapping_add(i64::from(delta).wrapping_mul(i64::from(count)));
            }
            current_dts = current_dts.wrapping_add(i64::from(delta).wrapping_mul(i64::from(count)));
            if current_dts > corrected_dts {
                let drift = current_dts
                    .saturating_sub(corrected_dts)
                    .checked_div(i64::from(count.max(1)))
                    .unwrap_or(0);
                let correction = if i64::from(delta) > drift {
                    u32::try_from(drift).unwrap_or(0)
                } else {
                    delta.saturating_sub(1)
                };
                current_dts =
                    current_dts.wrapping_sub(i64::from(correction).wrapping_mul(i64::from(count)));
                delta = delta.wrapping_sub(correction);
            }
            duration = duration.wrapping_add(i64::from(delta).wrapping_mul(i64::from(count)));
            total = total.saturating_add(u64::from(count));
            stts.push((count, delta));
        }
        let _ = total;
        if let Some((sc, _)) = self.last() {
            sc.stts = stts;
            sc.has_stts = !sc.stts.is_empty();
            if duration != 0 {
                sc.duration = sc.duration.min(duration);
            }
            sc.track_end = duration;
        }
        Ok(())
    }

    fn ctts(&mut self) -> Result<(), Error> {
        if self.trak_index.is_none() || self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let entries = self.r.u32()?;
        if entries == 0 {
            return Ok(());
        }
        if u64::from(entries).saturating_mul(8) > self.r.remaining() {
            return Err(Error::Truncated);
        }
        let mut ctts = Vec::new();
        let mut dts_shift = self.streams.last().map_or(0, |s| s.dts_shift);
        for i in 0..entries {
            let count = self.r.u32()?.cast_signed();
            let offset = self.r.u32()?.cast_signed();
            if count <= 0 {
                continue;
            }
            ctts.push((count.cast_unsigned(), offset));
            // FFmpeg takes the shift from every entry but the last two.
            if u64::from(i).saturating_add(2) < u64::from(entries) {
                dts_shift = update_dts_shift(dts_shift, offset);
            }
        }
        if let Some((sc, _)) = self.last() {
            sc.ctts = ctts;
            sc.has_ctts = !sc.ctts.is_empty();
            sc.dts_shift = dts_shift;
        }
        Ok(())
    }

    fn stsc(&mut self, a: Atom) -> Result<(), Error> {
        if self.trak_index.is_none() || self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let entries = self.r.u32()?;
        if u64::from(entries).saturating_mul(12).saturating_add(4)
            > u64::try_from(a.size).unwrap_or(0)
        {
            return Err(Error::Invalid("an stsc of more entries than its box holds"));
        }
        if entries == 0 {
            return Ok(());
        }
        if self.streams.last().is_some_and(|s| !s.stsc.is_empty()) {
            // A second stsc is ignored.
            return Ok(());
        }
        let mut stsc = Vec::with_capacity(usize::try_from(entries).unwrap_or(0));
        for _ in 0..entries {
            let first = self.r.u32()?;
            let count = self.r.u32()?;
            let id = self.r.u32()?;
            stsc.push(Stsc { first, count, id });
        }
        repair_stsc(&mut stsc);
        if let Some((sc, _)) = self.last() {
            sc.stsc = stsc;
        }
        Ok(())
    }

    fn stsz(&mut self, a: Atom) -> Result<(), Error> {
        if self.trak_index.is_none() || self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let (sample_size, field_size) = if a.kind == *b"stsz" {
            (self.r.u32()?, 32u32)
        } else {
            self.r.u24()?;
            (0, u32::from(self.r.u8()?))
        };
        let entries = self.r.u32()?;
        if let Some((sc, _)) = self.last() {
            if a.kind == *b"stsz" {
                if sc.sample_size == 0 {
                    sc.sample_size = sample_size;
                }
                sc.stsz_sample_size = sample_size;
            }
            sc.sample_count = entries;
        }
        if sample_size != 0 {
            return Ok(());
        }
        if !matches!(field_size, 4 | 8 | 16 | 32) {
            return Err(Error::Invalid("an stz2 field size"));
        }
        if entries == 0 {
            return Ok(());
        }
        let bytes = u64::from(entries)
            .saturating_mul(u64::from(field_size))
            .saturating_add(4)
            >> 3;
        if bytes > self.r.remaining() {
            // FFmpeg drops a truncated table and goes on.
            if let Some((sc, _)) = self.last() {
                sc.sample_sizes.clear();
                sc.sample_count = 0;
            }
            return Ok(());
        }
        let raw = self.r.bytes(bytes)?;
        let mut sizes = Vec::with_capacity(usize::try_from(entries).unwrap_or(0));
        let mut bit = 0usize;
        for _ in 0..entries {
            sizes.push(bits(&raw, bit, field_size));
            bit = bit.saturating_add(usize::try_from(field_size).unwrap_or(32));
        }
        if let Some((sc, _)) = self.last() {
            sc.sample_count = entries;
            sc.sample_sizes = sizes;
        }
        Ok(())
    }

    fn stco(&mut self, a: Atom) -> Result<(), Error> {
        if self.trak_index.is_none() || self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let wide = a.kind == *b"co64";
        let entries = u64::from(self.r.u32()?).min(
            u64::try_from(a.size.saturating_sub(8).max(0))
                .unwrap_or(0)
                .checked_div(if wide { 8 } else { 4 })
                .unwrap_or(0),
        );
        if entries == 0 {
            return Ok(());
        }
        if self
            .streams
            .last()
            .is_some_and(|s| !s.chunk_offsets.is_empty())
        {
            return Ok(());
        }
        let mut offsets = Vec::with_capacity(usize::try_from(entries).unwrap_or(0));
        for _ in 0..entries {
            let offset = if wide {
                // An offset that does not fit a signed 64-bit number is 0.
                self.r.u64()?.cast_signed().max(0)
            } else {
                i64::from(self.r.u32()?)
            };
            offsets.push(offset);
        }
        if let Some((sc, _)) = self.last() {
            sc.chunk_offsets = offsets;
        }
        Ok(())
    }

    fn stss(&mut self) -> Result<(), Error> {
        if self.trak_index.is_none() || self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let entries = self.r.u32()?;
        if entries == 0 {
            if let Some((sc, _)) = self.last() {
                sc.keyframe_absent = true;
            }
            return Ok(());
        }
        if u64::from(entries).saturating_mul(4) > self.r.remaining() {
            return Err(Error::Truncated);
        }
        let mut keys = Vec::with_capacity(usize::try_from(entries).unwrap_or(0));
        for _ in 0..entries {
            keys.push(self.r.u32()?);
        }
        if let Some((sc, _)) = self.last() {
            sc.keyframes = keys;
        }
        Ok(())
    }

    fn stps(&mut self) -> Result<(), Error> {
        if self.trak_index.is_none() || self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let entries = self.r.u32()?;
        if u64::from(entries).saturating_mul(4) > self.r.remaining() {
            return Err(Error::Truncated);
        }
        let mut stps = Vec::with_capacity(usize::try_from(entries).unwrap_or(0));
        for _ in 0..entries {
            stps.push(self.r.u32()?);
        }
        if let Some((sc, _)) = self.last() {
            sc.stps = stps;
        }
        Ok(())
    }

    fn sdtp(&mut self, a: Atom) -> Result<(), Error> {
        if self.streams.is_empty() {
            return Ok(());
        }
        self.r.u32()?;
        let entries = u64::try_from(a.size.saturating_sub(4).max(0)).unwrap_or(0);
        let take = entries.min(self.r.remaining());
        let flags = self.r.bytes(take)?;
        if let Some((sc, _)) = self.last() {
            sc.sdtp = flags;
        }
        Ok(())
    }

    /// `sbgp`: of its groupings FFmpeg reads 'rap ' -- samples that may start
    /// decoding though `stss` does not say so.
    fn sbgp(&mut self) -> Result<(), Error> {
        if self.streams.is_empty() {
            return Ok(());
        }
        let version = self.r.u8()?;
        self.r.u24()?;
        let grouping = self.r.fourcc()?;
        if version == 1 {
            self.r.u32()?;
        }
        let entries = self.r.u32()?;
        if grouping != *b"rap " || entries == 0 {
            return Ok(());
        }
        if u64::from(entries).saturating_mul(8) > self.r.remaining() {
            return Err(Error::Truncated);
        }
        let mut groups = Vec::with_capacity(usize::try_from(entries).unwrap_or(0));
        for _ in 0..entries {
            let count = self.r.u32()?;
            let index = self.r.u32()?;
            groups.push((count, index));
        }
        if let Some((sc, _)) = self.last() {
            sc.rap_group = groups;
        }
        Ok(())
    }

    /// `stsd`: the sample entries -- the codec, and what it is to decode.
    fn stsd(&mut self, a: Atom) -> Result<(), Error> {
        if self.streams.is_empty() {
            return Ok(());
        }
        if self
            .streams
            .last()
            .is_some_and(|s| s.stsd_count > 0 || !s.extradata.is_empty())
        {
            return Err(Error::Invalid("a second stsd in one trak"));
        }
        let version = self.r.u8()?;
        self.r.u24()?;
        let entries = self.r.u32()?.cast_signed();
        if entries <= 0 || i64::from(entries) > a.size / 8 || entries > 1024 {
            return Err(Error::Invalid("an stsd's entry count"));
        }
        if let Some((sc, d)) = self.last() {
            d.stsd_version = version;
            sc.extradata = vec![Vec::new(); usize::try_from(entries).unwrap_or(0)];
        }
        let mut pseudo = 0i32;
        while pseudo < entries {
            if self.r.remaining() == 0 {
                return Err(Error::Truncated);
            }
            self.stsd_entry(pseudo)?;
            pseudo = pseudo.saturating_add(1);
        }
        // The first entry's setup is the track's.
        if let Some((sc, d)) = self.last() {
            d.config = sc.extradata.first().cloned().unwrap_or_default();
        }
        Ok(())
    }

    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the file, and an entry's size checked to be at least 8"
    )]
    fn stsd_entry(&mut self, pseudo: i32) -> Result<(), Error> {
        let start = self.r.pos();
        let size = i64::from(self.r.u32()?);
        let format = self.r.fourcc()?;
        if size >= 16 {
            self.r.skip(6)?;
            self.r.u16()?; // the data reference
        } else if size <= 7 {
            return Err(Error::Invalid("an stsd entry's size"));
        }
        let read = i64::try_from(self.r.pos() - start).unwrap_or(0);
        let codec_tag = self.descriptions.last().map_or([0; 4], |d| d.codec_tag);
        if codec_tag != [0; 4] && skip_entry(codec_tag, format) {
            // FFmpeg reads one codec a track: a later entry of another is
            // passed over.
            self.r.skip(u64::try_from(size - read).unwrap_or(0))?;
            if let Some((sc, _)) = self.last() {
                sc.stsd_count = sc.stsd_count.saturating_add(1);
            }
            return Ok(());
        }
        let isom = self.isom;
        let Some((sc, d)) = self.last() else {
            return Ok(());
        };
        sc.pseudo_stream_id = if codec_tag == [0; 4] { pseudo } else { -1 };
        // The codec, and the kind it makes the track (`mov_codec_id`).
        let audio = audio_codec(format);
        let mut codec = Codec::Other;
        if sc.kind != Kind::Video
            && let Some(c) = audio
        {
            sc.kind = Kind::Audio;
            codec = c;
        } else if sc.kind != Kind::Audio && format != [0; 4] && format != *b"mp4s" {
            if let Some(c) = video_codec(format) {
                sc.kind = Kind::Video;
                codec = c;
            }
        }
        d.codec = codec;
        d.codec_tag = format;
        let kind = sc.kind;
        match kind {
            Kind::Video => self.stsd_video(start)?,
            Kind::Audio => self.stsd_audio(isom)?,
            _ => {}
        }
        // The boxes after the entry's fields: av1C, esds, colr...
        let left = size - i64::try_from(self.r.pos() - start).unwrap_or(0);
        if left > 8 {
            self.walk(Atom {
                kind: *b"stsd",
                size: left,
            })?;
        } else if left > 0 {
            self.r.skip(u64::try_from(left).unwrap_or(0))?;
        }
        if let Some((sc, d)) = self.last() {
            if let Some(slot) = sc
                .extradata
                .get_mut(usize::try_from(pseudo).unwrap_or(usize::MAX))
            {
                *slot = core::mem::take(&mut d.config);
            }
            if d.codec == Codec::Aac
                && let Some(o) = d.object_type
            {
                d.codec = match o {
                    0x69 | 0x6B => Codec::Mp3,
                    0x40 | 0x66 | 0x67 | 0x68 => Codec::Aac,
                    0xA5 => Codec::Ac3,
                    0xA6 => Codec::Eac3,
                    0xAD => Codec::Opus,
                    _ => Codec::Other,
                };
            }
            if d.sample_rate == 0 && sc.kind == Kind::Audio && sc.time_scale > 1 {
                d.sample_rate = sc.time_scale.cast_unsigned();
            }
            sc.stsd_count = sc.stsd_count.saturating_add(1);
        }
        Ok(())
    }

    /// A visual sample entry's fields (`mov_parse_stsd_video`).
    fn stsd_video(&mut self, start: u64) -> Result<(), Error> {
        self.r.skip(16)?;
        let width = self.r.u16()?;
        let height = self.r.u16()?;
        // resolution (8), data size (4), frames per sample (2), the codec's
        // name (32).
        self.r.skip(46)?;
        let depth = self.r.u16()?;
        if let Some((_, d)) = self.last() {
            d.width = u32::from(width);
            d.height = u32::from(height);
            d.bits_per_coded_sample = u32::from(depth);
        }
        // FFmpeg rereads the entry for a palette: from its start, 82 bytes on,
        // the depth again and the colour table's ID.
        self.r.seek_to(start.saturating_add(82))?;
        let packed = self.r.u16()?;
        let table = self.r.u16()?;
        let bit_depth = packed & 0x1f;
        if matches!(bit_depth, 1 | 2 | 4 | 8) && table == 0 {
            // A palette in the file: skipped, as nothing here draws it.
            let first = self.r.u32()?;
            self.r.u16()?;
            let last = u32::from(self.r.u16()?);
            if first <= 255 && last <= 255 && first <= last {
                self.r.skip(
                    u64::from(last.saturating_sub(first).saturating_add(1)).saturating_mul(8),
                )?;
            }
        }
        Ok(())
    }

    /// A sound sample entry's fields (`mov_parse_stsd_audio`).
    fn stsd_audio(&mut self, isom: bool) -> Result<(), Error> {
        let version = self.r.u16()?;
        self.r.skip(6)?;
        let channels = u32::from(self.r.u16()?);
        let bits = u32::from(self.r.u16()?);
        let audio_cid = self.r.u16()?.cast_signed();
        self.r.u16()?;
        let rate = self.r.u32()? >> 16;
        let stsd_version = self.descriptions.last().map_or(0, |d| d.stsd_version);
        let (mut channels, mut bits, mut rate) = (channels, bits, rate);
        let (mut samples_per_frame, mut bytes_per_frame) = (0u32, 0u32);
        if !isom || (stsd_version == 0 && version > 0) {
            if version == 1 {
                samples_per_frame = self.r.u32()?;
                self.r.u32()?;
                bytes_per_frame = self.r.u32()?;
                self.r.u32()?;
            } else if version == 2 {
                self.r.u32()?;
                let r = f64::from_bits(self.r.u64()?);
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "C's conversion of the double to int, held in range"
                )]
                {
                    rate = r.clamp(0.0, f64::from(u32::MAX)) as u32;
                }
                channels = self.r.u32()?;
                self.r.u32()?;
                bits = self.r.u32()?;
                self.r.u32()?;
                bytes_per_frame = self.r.u32()?;
                samples_per_frame = self.r.u32()?;
            }
        }
        if let Some((sc, d)) = self.last() {
            d.channels = channels;
            d.sample_rate = rate;
            d.bits_per_coded_sample = bits;
            d.audio_cid = audio_cid;
            sc.samples_per_frame = samples_per_frame;
            sc.bytes_per_frame = bytes_per_frame;
            // Uncompressed sound: the size of a sample, every channel's.
            if let Some(b) = pcm_bits(d.codec_tag, bits)
                && let Some(size) = (b >> 3).checked_mul(channels)
            {
                sc.sample_size = size;
            }
        }
        Ok(())
    }

    /// A codec configuration box: kept whole, as the track's setup.
    fn config(&mut self, a: Atom) -> Result<(), Error> {
        if self.streams.is_empty() || a.size > 1 << 30 {
            return Ok(());
        }
        let body = self.r.bytes(u64::try_from(a.size.max(0)).unwrap_or(0))?;
        if let Some((_, d)) = self.last()
            && d.config.is_empty()
        {
            d.config = body;
        }
        Ok(())
    }

    fn esds(&mut self, a: Atom) -> Result<(), Error> {
        if self.streams.is_empty() || a.size > 1 << 30 {
            return Ok(());
        }
        let body = self.r.bytes(u64::try_from(a.size.max(0)).unwrap_or(0))?;
        let (object_type, dsi) = read_esds(&body);
        if let Some((_, d)) = self.last() {
            d.object_type = object_type;
            if d.config.is_empty() {
                d.config = dsi;
            }
        }
        Ok(())
    }

    fn colr(&mut self, a: Atom) -> Result<(), Error> {
        if self.streams.is_empty() || a.size < 4 {
            return Ok(());
        }
        let kind = self.r.fourcc()?;
        if (kind == *b"nclx" || kind == *b"nclc") && a.size >= 10 {
            let primaries = self.r.u16()?;
            let transfer = self.r.u16()?;
            let matrix = self.r.u16()?;
            let full_range = kind == *b"nclx" && a.size >= 11 && self.r.u8()? & 0x80 != 0;
            if let Some((_, d)) = self.last() {
                d.colour = Some(Colour {
                    primaries,
                    transfer,
                    matrix,
                    full_range,
                });
            }
        }
        Ok(())
    }

    fn pasp(&mut self) -> Result<(), Error> {
        let h = self.r.u32()?;
        let v = self.r.u32()?;
        if let Some((_, d)) = self.last()
            && h != 0
            && v != 0
        {
            d.pixel_aspect = Some((h, v));
        }
        Ok(())
    }

    fn trex(&mut self) -> Result<(), Error> {
        self.r.u32()?;
        let t = Trex {
            track_id: self.r.u32()?,
            stsd_id: self.r.u32()?,
            duration: self.r.u32()?,
            size: self.r.u32()?,
            flags: self.r.u32()?,
        };
        self.trex.push(t);
        Ok(())
    }

    fn moof(&mut self, a: Atom) -> Result<(), Error> {
        self.fragment.found_tfhd = false;
        let offset = self.r.pos().saturating_sub(8);
        self.fragment.moof_offset = offset;
        self.fragment.implicit_offset = offset;
        // A fresh part of the fragment index, a slot per stream.
        if self.streams.iter().any(|s| s.id < 0) {
            return Err(Error::Invalid("a fragment of a track with no ID"));
        }
        self.frag_streams = self
            .streams
            .iter()
            .map(|s| FragStream {
                id: s.id,
                tfdt_dts: None,
                next_trun_dts: None,
            })
            .collect();
        self.frag_current = None;
        self.walk(a)
    }

    fn tfhd(&mut self) -> Result<(), Error> {
        self.r.u8()?;
        let flags = self.r.u24()?;
        let track_id = self.r.u32()?;
        if track_id == 0 {
            return Err(Error::Invalid("a tfhd of track 0"));
        }
        let Some(trex) = self.trex.iter().find(|t| t.track_id == track_id).copied() else {
            return Ok(());
        };
        let frag = &mut self.fragment;
        frag.found_tfhd = true;
        frag.track_id = track_id;
        self.frag_current = self
            .frag_streams
            .iter()
            .position(|f| f.id == i64::from(track_id));
        frag.base_data_offset = if flags & TFHD_BASE_DATA_OFFSET != 0 {
            self.r.u64()?
        } else if flags & TFHD_DEFAULT_BASE_IS_MOOF != 0 {
            frag.moof_offset
        } else {
            frag.implicit_offset
        };
        frag.stsd_id = if flags & TFHD_STSD_ID != 0 {
            self.r.u32()?
        } else {
            trex.stsd_id
        };
        frag.duration = if flags & TFHD_DEFAULT_DURATION != 0 {
            self.r.u32()?
        } else {
            trex.duration
        };
        frag.size = if flags & TFHD_DEFAULT_SIZE != 0 {
            self.r.u32()?
        } else {
            trex.size
        };
        frag.flags = if flags & TFHD_DEFAULT_FLAGS != 0 {
            self.r.u32()?
        } else {
            trex.flags
        };
        if let Some(f) = self.frag_current.and_then(|i| self.frag_streams.get_mut(i)) {
            f.next_trun_dts = None;
        }
        Ok(())
    }

    fn frag_stream(&self) -> Option<usize> {
        let id = i64::from(self.fragment.track_id);
        self.streams.iter().position(|s| s.id == id)
    }

    fn tfdt(&mut self) -> Result<(), Error> {
        let Some(i) = self.frag_stream() else {
            return Ok(());
        };
        let stsd_id = self.fragment.stsd_id;
        if let Some(sc) = self.streams.get(i)
            && i64::from(sc.pseudo_stream_id).wrapping_add(1) != i64::from(stsd_id)
            && sc.pseudo_stream_id != -1
        {
            return Ok(());
        }
        let version = self.r.u8()?;
        self.r.u24()?;
        let base = if version != 0 {
            self.r.u64()?.cast_signed()
        } else {
            i64::from(self.r.u32()?)
        };
        if let Some(f) = self.frag_current.and_then(|i| self.frag_streams.get_mut(i)) {
            f.tfdt_dts = Some(base);
        }
        if let Some(sc) = self.streams.get_mut(i) {
            sc.track_end = base;
        }
        Ok(())
    }

    /// `trun`: a run of a fragment's samples, appended to its track's index
    /// (`mov_read_trun`, for fragments read in the file's order).
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions and times as FFmpeg's 64-bit sums, each sample's checked to stay in range"
    )]
    fn trun(&mut self) -> Result<(), Error> {
        if !self.fragment.found_tfhd {
            return Err(Error::Invalid("a trun with no tfhd before it"));
        }
        let Some(i) = self.frag_stream() else {
            return Ok(());
        };
        let frag = self.fragment;
        let Some(sc) = self.streams.get(i) else {
            return Ok(());
        };
        if i64::from(sc.pseudo_stream_id) + 1 != i64::from(frag.stsd_id)
            && sc.pseudo_stream_id != -1
        {
            return Ok(());
        }
        self.r.u8()?;
        let flags = self.r.u24()?;
        let entries = self.r.u32()?;
        let data_offset = if flags & TRUN_DATA_OFFSET != 0 {
            self.r.u32()?.cast_signed()
        } else {
            0
        };
        let first_flags = if flags & TRUN_FIRST_SAMPLE_FLAGS != 0 {
            self.r.u32()?
        } else {
            frag.flags
        };
        let fs = self
            .frag_current
            .and_then(|j| self.frag_streams.get(j))
            .copied();
        let Some(sc) = self.streams.get_mut(i) else {
            return Ok(());
        };
        let mut dts = match fs {
            Some(FragStream {
                next_trun_dts: Some(next),
                ..
            }) => next.wrapping_sub(sc.time_offset),
            Some(FragStream {
                tfdt_dts: Some(t), ..
            }) => t.wrapping_sub(sc.time_offset),
            _ => sc.track_end.wrapping_sub(sc.time_offset),
        };
        // FFmpeg's unsigned sum, which wraps.
        let mut offset = frag
            .base_data_offset
            .wrapping_add_signed(i64::from(data_offset))
            .cast_signed();
        if entries == 0 {
            return Ok(());
        }
        let per = u64::from(flags & TRUN_SAMPLE_DURATION != 0)
            + u64::from(flags & TRUN_SAMPLE_SIZE != 0)
            + u64::from(flags & TRUN_SAMPLE_FLAGS != 0)
            + u64::from(flags & TRUN_SAMPLE_CTS != 0);
        if u64::from(entries).saturating_mul(per * 4) > self.r.remaining() {
            return Err(Error::Truncated);
        }
        let mut prev_dts = sc.index.last().map(|e| e.timestamp);
        let mut distance = 0u32;
        if flags & TRUN_SAMPLE_CTS != 0 {
            sc.has_ctts = true;
        }
        sc.has_stts = true;
        for n in 0..entries {
            let mut sample_duration = frag.duration;
            let mut sample_size = frag.size;
            let mut sample_flags = if n == 0 { first_flags } else { frag.flags };
            let mut cts = 0i32;
            if flags & TRUN_SAMPLE_DURATION != 0 {
                sample_duration = self.r.u32()?;
            }
            if flags & TRUN_SAMPLE_SIZE != 0 {
                sample_size = self.r.u32()?;
            }
            if flags & TRUN_SAMPLE_FLAGS != 0 {
                sample_flags = self.r.u32()?;
            }
            if flags & TRUN_SAMPLE_CTS != 0 {
                cts = self.r.u32()?.cast_signed();
            }
            let Some(sc) = self.streams.get_mut(i) else {
                return Ok(());
            };
            sc.dts_shift = update_dts_shift(sc.dts_shift, cts);
            let keyframe = sample_flags & (FRAG_SAMPLE_IS_NON_SYNC | FRAG_SAMPLE_DEPENDS_YES) == 0;
            let mut entry_flags = 0;
            if keyframe {
                distance = 0;
                entry_flags |= KEYFRAME;
            }
            if prev_dts.is_some_and(|p| p >= dts) {
                entry_flags |= DISCARD;
            }
            sc.index.push(Entry {
                pos: offset,
                timestamp: dts,
                size: sample_size,
                min_distance: distance,
                flags: entry_flags,
            });
            sc.tts.push(Tts {
                count: 1,
                offset: cts,
                duration: sample_duration,
            });
            distance = distance.saturating_add(1);
            if dts.checked_add(i64::from(sample_duration)).is_none() || sample_size == 0 {
                return Err(Error::Invalid("a trun sample"));
            }
            prev_dts = Some(dts);
            dts += i64::from(sample_duration);
            offset = offset.saturating_add(i64::from(sample_size));
        }
        if let Some(f) = self.frag_current.and_then(|j| self.frag_streams.get_mut(j))
            && let Some(sc) = self.streams.get(i)
        {
            f.next_trun_dts = Some(dts.wrapping_add(sc.time_offset));
        }
        self.fragment.implicit_offset = u64::try_from(offset).unwrap_or(0);
        if let Some(sc) = self.streams.get_mut(i) {
            sc.track_end = dts.wrapping_add(sc.time_offset);
            if sc.duration < sc.track_end {
                sc.duration = sc.track_end;
            }
        }
        Ok(())
    }
}

const TFHD_BASE_DATA_OFFSET: u32 = 0x01;
const TFHD_STSD_ID: u32 = 0x02;
const TFHD_DEFAULT_DURATION: u32 = 0x08;
const TFHD_DEFAULT_SIZE: u32 = 0x10;
const TFHD_DEFAULT_FLAGS: u32 = 0x20;
const TFHD_DEFAULT_BASE_IS_MOOF: u32 = 0x02_0000;
const TRUN_DATA_OFFSET: u32 = 0x01;
const TRUN_FIRST_SAMPLE_FLAGS: u32 = 0x04;
const TRUN_SAMPLE_DURATION: u32 = 0x100;
const TRUN_SAMPLE_SIZE: u32 = 0x200;
const TRUN_SAMPLE_FLAGS: u32 = 0x400;
const TRUN_SAMPLE_CTS: u32 = 0x800;
const FRAG_SAMPLE_IS_NON_SYNC: u32 = 0x1_0000;
const FRAG_SAMPLE_DEPENDS_YES: u32 = 0x100_0000;
/// FFmpeg's `max_stts_delta` default: a larger delta is a negative one.
const MAX_STTS_DELTA: u32 = u32::MAX - (1 << 22) + 1;

/// FFmpeg's `mov_update_dts_shift`: a negative composition offset raises the
/// shift that makes every decoding time at most its presentation time.
pub(crate) fn update_dts_shift(shift: i32, offset: i32) -> i32 {
    if offset >= 0 {
        return shift;
    }
    let offset = if offset == i32::MIN {
        offset.saturating_add(1)
    } else {
        offset
    };
    shift.max(offset.saturating_neg())
}

enum Sanity {
    Ok,
    Missing,
    Contradictory,
}

/// FFmpeg's `sanity_checks`.
fn sanity_check(sc: &Stream) -> Sanity {
    let chunks = !sc.chunk_offsets.is_empty();
    let samples = sc.sample_count != 0;
    if (chunks && (sc.stts.is_empty() || sc.stsc.is_empty() || (sc.sample_size == 0 && !samples)))
        || (samples && (!chunks || (sc.sample_size == 0 && sc.sample_sizes.is_empty())))
    {
        return Sanity::Missing;
    }
    if let Some(last) = sc.stsc.last()
        && u64::from(last.first) > u64::try_from(sc.chunk_offsets.len()).unwrap_or(u64::MAX)
    {
        return Sanity::Contradictory;
    }
    Sanity::Ok
}

/// FFmpeg's repair of an `stsc`: from the last entry back, an entry out of
/// order or zero is replaced by the next valid one, or patched if last.
fn repair_stsc(stsc: &mut Vec<Stsc>) {
    let mut i = stsc.len();
    while i > 0 {
        i = i.saturating_sub(1);
        let first_min = u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1);
        let Some(cur) = stsc.get(i).copied() else {
            break;
        };
        let next = stsc.get(i.wrapping_add(1)).copied();
        let prev = i.checked_sub(1).and_then(|p| stsc.get(p)).copied();
        let bad = next.is_some_and(|n| cur.first >= n.first)
            || prev.is_some_and(|p| cur.first <= p.first)
            || cur.first < first_min
            || cur.count < 1
            || cur.id < 1;
        if !bad {
            continue;
        }
        match next {
            None => {
                if cur.count == 0 && i > 0 {
                    stsc.truncate(i);
                    continue;
                }
                let mut fixed = cur;
                fixed.first = fixed.first.max(first_min);
                if let Some(p) = prev
                    && fixed.first <= p.first
                {
                    fixed.first = p.first.saturating_add(1).min(i32::MAX.cast_unsigned());
                }
                fixed.count = fixed.count.max(1);
                fixed.id = fixed.id.max(1);
                if let Some(e) = stsc.get_mut(i) {
                    *e = fixed;
                }
            }
            Some(n) => {
                if let Some(e) = stsc.get_mut(i) {
                    *e = Stsc {
                        first: n.first.saturating_sub(1),
                        count: n.count,
                        id: n.id,
                    };
                }
            }
        }
    }
}

/// Whether FFmpeg passes over a sample entry of `format` after one of
/// `codec_tag` (`mov_skip_multiple_stsd`).
fn skip_entry(codec_tag: [u8; 4], format: [u8; 4]) -> bool {
    codec_tag != format
        && !(codec_tag == *b"AV1x" && format == *b"AVup")
        && codec_tag != *b"apcn"
        && codec_tag != *b"apch"
        && codec_tag != *b"dvpp"
        && codec_tag != *b"dvcp"
        && codec_tag != *b"jpeg"
}

/// The bits a sample of uncompressed sound takes, for the codes FFmpeg reads
/// as PCM, as `mov_parse_stsd_audio` settles them.
fn pcm_bits(tag: [u8; 4], bits: u32) -> Option<u32> {
    match &tag {
        b"raw " => Some(if bits == 16 { 16 } else { 8 }),
        b"twos" | b"sowt" => Some(match bits {
            8 => 8,
            24 => 24,
            32 => 32,
            _ => 16,
        }),
        b"in24" => Some(24),
        b"in32" | b"fl32" => Some(32),
        b"fl64" => Some(64),
        b"ulaw" | b"alaw" => Some(8),
        _ => None,
    }
}

/// `field` bits from bit `at` of `raw`, big-endian.
fn bits(raw: &[u8], at: usize, field: u32) -> u32 {
    let mut v = 0u32;
    for k in 0..usize::try_from(field).unwrap_or(0) {
        let bit = at.saturating_add(k);
        let byte = raw.get(bit >> 3).copied().unwrap_or(0);
        // The bit's place in its byte, from the top: 7 - bit % 8.
        let b = (byte >> (7 ^ (bit & 7))) & 1;
        v = (v << 1) | u32::from(b);
    }
    v
}

/// `mdhd`'s language: three letters of five bits each, from 0x60; a
/// QuickTime language code below 0x400 is `und` here.
fn language(code: u16) -> [u8; 3] {
    if code < 0x400 || code == 0x7fff {
        return *b"und";
    }
    let letter = |shift: u16| {
        let v = u8::try_from((code >> shift) & 0x1f).unwrap_or(0);
        v.wrapping_add(0x60)
    };
    [letter(10), letter(5), letter(0)]
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    fn run(first: u32, count: u32, id: u32) -> Stsc {
        Stsc { first, count, id }
    }

    #[test]
    fn a_negative_offset_raises_the_shift_and_a_positive_one_does_not() {
        assert_eq!(update_dts_shift(0, 512), 0);
        assert_eq!(update_dts_shift(0, -512), 512);
        assert_eq!(update_dts_shift(1024, -512), 1024, "the largest is kept");
        assert_eq!(
            update_dts_shift(0, i32::MIN),
            i32::MAX,
            "the lowest offset is held to i32::MAX"
        );
    }

    #[test]
    fn an_stsc_is_repaired_as_ffmpeg_repairs_it() {
        // A repeated first chunk: the entry is replaced by the next valid
        // one, a chunk before it.
        let mut s = vec![run(1, 2, 1), run(1, 3, 1), run(4, 2, 1)];
        repair_stsc(&mut s);
        assert_eq!(s, [run(1, 2, 1), run(3, 2, 1), run(4, 2, 1)]);
        // A last entry of no samples after another: dropped.
        let mut s = vec![run(1, 2, 1), run(3, 0, 1)];
        repair_stsc(&mut s);
        assert_eq!(s, [run(1, 2, 1)]);
        // A lone entry of chunk 0 and entry 0: patched to 1 and 1.
        let mut s = vec![run(0, 4, 0)];
        repair_stsc(&mut s);
        assert_eq!(s, [run(1, 4, 1)]);
    }

    #[test]
    fn compact_sizes_read_as_their_fields() {
        let raw = [0b1010_0101, 0xff, 0x01];
        assert_eq!(bits(&raw, 0, 4), 0b1010);
        assert_eq!(bits(&raw, 4, 4), 0b0101);
        assert_eq!(bits(&raw, 8, 16), 0xff01);
        assert_eq!(bits(&raw, 20, 8), 0x10, "past the end reads zeros");
    }

    #[test]
    fn a_language_is_three_letters_or_und() {
        // "eng": e=5, n=14, g=7, five bits each.
        assert_eq!(language((5 << 10) | (14 << 5) | 7), *b"eng");
        assert_eq!(language(0x55C4), *b"und");
        assert_eq!(language(0), *b"und", "a QuickTime code");
        assert_eq!(language(0x7fff), *b"und");
    }
}
