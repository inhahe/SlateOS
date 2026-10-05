//! The demuxer: a file's links and their streams, read in order, and a seek.
//!
//! **Links.** A chained file is a run of links, each a complete Ogg file of
//! its own: its streams' first pages, their headers, their data, their last
//! pages. When opened, the demuxer reads the first link's headers and looks
//! at the file's last page; a serial number the first link does not have
//! means a chain, and each link's start is then found by bisection on serial
//! numbers, as libvorbisfile finds them. A link is played on as the one
//! before it when it holds the same streams -- the same codecs, in the same
//! order, at the same rates and channel counts -- and the file is taken to
//! end where one does not. (A chain whose links share a serial number, which
//! RFC 3533 forbids, is read as one link whose headers come again in band:
//! its later links' times start again from their own granule positions, as
//! FFmpeg times them.)
//!
//! **One timeline.** FFmpeg times each link by its own granule positions,
//! so a chained file's times start again at every link. Here a link's times
//! are moved on so that its first sound plays where the last link's last
//! sound ended, as libvorbisfile counts a chain's samples. The first link's
//! times are FFmpeg's.
//!
//! **Seeking** finds, by bisection over the link's pages, the stream's last
//! page whose granule position is at or before the time, and reads on from
//! it: the packet that runs off its end is kept, and starts at its granule
//! -- the time it has when the file is read through. Before the stream's
//! first such page, the seek is to the link's start, read as on opening.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "file positions bounded by the file's length, and times as saturating sums"
)]

use std::collections::VecDeque;
use std::io::{Read, Seek};

use crate::page::{FIRST, LAST, Page, Pages};
use crate::stream::{Codec, State};
use crate::{Error, Packet, Stream};

/// Below this many bytes, a bisection reads on page by page.
const LINEAR: u64 = 64 * 1024;

/// How far back from a link's end its streams' last pages are looked for.
const END_SEARCH: u64 = 4 * 1024 * 1024;

/// How many pages are read from a link's data to find where each of its
/// streams' sound begins.
const START_SEARCH: usize = 512;

/// A link: a run of the file whose streams are one set.
#[derive(Clone)]
struct Link {
    /// Where its first page after its headers begins.
    data_start: u64,
    /// Where the next link begins, or the file's end.
    end: u64,
    /// Its streams, their headers read, in the order their first pages come,
    /// their times moved on by the links before.
    streams: Vec<State>,
    /// Each stream's first sound and the end of its last, on the file's
    /// clock.
    first: Vec<Option<i64>>,
    last: Vec<Option<i64>>,
}

impl Link {
    fn has_serial(&self, serial: u32) -> bool {
        self.streams.iter().any(|s| s.info.serial == serial)
    }

    /// Whether `next` holds the same streams, to be played on after this.
    fn continued_by(&self, next: &Self) -> bool {
        self.streams.len() == next.streams.len()
            && self.streams.iter().zip(&next.streams).all(|(a, b)| {
                a.info.codec == b.info.codec
                    && (!a.info.codec.is_timed()
                        || (a.info.rate == b.info.rate && a.info.channels == b.info.channels))
            })
    }
}

/// An Ogg file being read.
pub struct Demuxer<R> {
    pages: Pages<R>,
    /// The first link's streams, as their headers describe them.
    streams: Vec<Stream>,
    links: Vec<Link>,
    /// The link being read.
    link: usize,
    /// Its streams, being read.
    states: Vec<State>,
    /// Where the next page is looked for.
    at: u64,
    queue: VecDeque<Packet>,
}

impl<R: Read + Seek> Demuxer<R> {
    /// Opens `source`: its streams' first pages and their headers, and the
    /// links of a chained file.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the source fails; [`Error::Invalid`] when it
    /// holds no Ogg page that starts a stream.
    pub fn open(source: R) -> Result<Self, Error> {
        let mut pages = Pages::new(source)?;
        let (link, states, packets, at) = read_link(&mut pages, 0)?;
        let mut d = Self {
            pages,
            streams: link.streams.iter().map(|s| s.info.clone()).collect(),
            links: vec![link],
            link: 0,
            states,
            at,
            queue: packets.into(),
        };
        d.find_links()?;
        d.time_links()?;
        Ok(d)
    }

    /// The file's streams, in the order their first pages come: the first
    /// link's, which every later link's repeat.
    pub fn streams(&self) -> &[Stream] {
        &self.streams
    }

    /// How many links the file has: 1 unless it is chained.
    pub fn links(&self) -> usize {
        self.links.len()
    }

    /// A stream's tick, `(num, den)` seconds: `(1, rate)` for a timed
    /// stream; `None` for one that is not, or that the file does not have.
    pub fn time_base(&self, stream: usize) -> Option<(u64, u64)> {
        let s = self.streams.get(stream)?;
        (s.codec.is_timed() && s.rate > 0).then_some((1, u64::from(s.rate)))
    }

    /// How long a stream's sound plays, in its ticks: from its first sound
    /// (after Opus's pre-skip) to the end its last page gives, across every
    /// link. (FFmpeg's duration of an Opus stream counts the pre-skip in.)
    pub fn duration(&self, stream: usize) -> Option<u64> {
        let first = self.links.first()?.first.get(stream).copied().flatten()?;
        let last = self.links.last()?.last.get(stream).copied().flatten()?;
        u64::try_from(last.saturating_sub(first)).ok()
    }

    /// The next data packet of any stream, in file order; `None` at the end.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the source fails.
    pub fn next_packet(&mut self) -> Result<Option<Packet>, Error> {
        loop {
            if let Some(p) = self.queue.pop_front() {
                return Ok(Some(p));
            }
            let end = self.links.get(self.link).map_or(0, |l| l.end);
            if self.at >= end {
                if self.link + 1 < self.links.len() {
                    self.enter_link(self.link + 1, true);
                    continue;
                }
                return Ok(None);
            }
            let Some(page) = self.pages.next_page(self.at)? else {
                self.at = end;
                continue;
            };
            if page.position >= end {
                self.at = end;
                continue;
            }
            self.at = page.end();
            if let Some(index) = self
                .states
                .iter()
                .position(|s| s.info.serial == page.serial)
            {
                if let Some(state) = self.states.get_mut(index) {
                    let packets = take_page(state, &page, index);
                    self.queue.extend(packets);
                }
            }
        }
    }

    /// Goes to `ticks` of stream `stream`: the next packets are the
    /// stream's from the last page whose granule position is at or before
    /// it -- the one that runs off that page's end first -- with every
    /// other stream's from there; or, before the stream's first such page,
    /// its link's from the start.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the source fails; [`Error::Invalid`] for a stream
    /// the file does not have.
    pub fn seek(&mut self, stream: usize, ticks: i64) -> Result<(), Error> {
        if stream >= self.streams.len() {
            return Err(Error::Invalid("a seek in a stream the file does not have"));
        }
        // The last link whose sound starts at or before the time.
        let k = self
            .links
            .iter()
            .rposition(|l| {
                l.first
                    .get(stream)
                    .copied()
                    .flatten()
                    .is_some_and(|f| f <= ticks)
            })
            .unwrap_or(0);
        let Some(link) = self.links.get(k) else {
            return Ok(());
        };
        let (lo, hi) = (link.data_start, link.end);
        let Some(s) = link.streams.get(stream) else {
            return Err(Error::Invalid("a seek in a stream the file does not have"));
        };
        let (serial, pre_skip, offset) = (s.info.serial, i64::from(s.info.pre_skip), s.offset);
        let found = self.bisect(lo, hi, serial, |g| {
            g.saturating_sub(pre_skip).saturating_add(offset) <= ticks
        })?;
        self.enter_link(k, found.is_none());
        if let Some(page) = found {
            self.at = page.end();
            if let Some(s) = self.states.get_mut(stream) {
                s.resume_after(&page);
            }
        }
        Ok(())
    }

    /// Reads link `k` from its start (`at_start`), or from wherever the
    /// caller puts it: its streams afresh, with their headers to give with
    /// their next packets in a chained file.
    fn enter_link(&mut self, k: usize, at_start: bool) {
        let chained = self.links.len() > 1;
        let Some(link) = self.links.get(k) else {
            return;
        };
        self.link = k;
        self.at = link.data_start;
        self.states = link.streams.clone();
        for s in &mut self.states {
            let headers = chained.then(|| s.info.headers.clone());
            s.jump(at_start, headers);
        }
        self.queue.clear();
    }

    /// The last page of stream `serial` in `[lo, hi)` with a granule position
    /// that `before` takes, found by bisection: granule positions only grow
    /// along a stream.
    fn bisect(
        &mut self,
        mut lo: u64,
        mut hi: u64,
        serial: u32,
        before: impl Fn(i64) -> bool,
    ) -> Result<Option<Page>, Error> {
        let mut best = None;
        while hi.saturating_sub(lo) > LINEAR {
            let mid = lo + (hi - lo) / 2;
            match self.timed_page(mid, hi, serial)? {
                Some(page) if before(page.granule) => {
                    lo = page.end();
                    best = Some(page);
                }
                _ => hi = mid,
            }
        }
        let mut at = lo;
        while let Some(page) = self.pages.next_page(at)? {
            if page.position >= hi {
                break;
            }
            at = page.end();
            if page.serial == serial && page.granule != -1 {
                if !before(page.granule) {
                    break;
                }
                best = Some(page);
            }
        }
        Ok(best)
    }

    /// The first page of stream `serial` with a granule position, from
    /// `from` and beginning before `end`.
    fn timed_page(&mut self, from: u64, end: u64, serial: u32) -> Result<Option<Page>, Error> {
        let mut at = from;
        while let Some(page) = self.pages.next_page(at)? {
            if page.position >= end {
                return Ok(None);
            }
            if page.serial == serial && page.granule != -1 {
                return Ok(Some(page));
            }
            at = page.end();
        }
        Ok(None)
    }

    /// The links after the first, of a chained file: each found by
    /// bisection on serial numbers from the file's last page.
    fn find_links(&mut self) -> Result<(), Error> {
        let len = self.pages.len();
        while let Some(current) = self.links.last() {
            let from = current.data_start;
            let Some(last) = self.pages.last_page(from, len)? else {
                break;
            };
            if current.has_serial(last.serial) {
                break;
            }
            let boundary = self.boundary(from, last.position)?;
            if let Some(current) = self.links.last_mut() {
                current.end = boundary;
            }
            let Ok((next, ..)) = read_link(&mut self.pages, boundary) else {
                break;
            };
            if !self
                .links
                .first()
                .is_some_and(|first| first.continued_by(&next))
            {
                break;
            }
            self.links.push(next);
        }
        Ok(())
    }

    /// Where the next link begins: the first page from `lo` whose serial
    /// number the last link does not have, given one at `foreign`.
    fn boundary(&mut self, mut lo: u64, foreign: u64) -> Result<u64, Error> {
        let mut found = foreign;
        let mut hi = foreign;
        let serials: Vec<u32> = self
            .links
            .last()
            .map(|l| l.streams.iter().map(|s| s.info.serial).collect())
            .unwrap_or_default();
        let ours = |serial: u32| serials.contains(&serial);
        while hi.saturating_sub(lo) > LINEAR {
            let mid = lo + (hi - lo) / 2;
            match self.pages.next_page(mid)? {
                Some(page) if page.position < hi => {
                    if ours(page.serial) {
                        lo = page.end();
                    } else {
                        found = page.position;
                        hi = page.position;
                    }
                }
                _ => hi = mid,
            }
        }
        let mut at = lo;
        while let Some(page) = self.pages.next_page(at)? {
            if page.position >= found || !ours(page.serial) {
                return Ok(page.position.min(found));
            }
            at = page.end();
        }
        Ok(found)
    }

    /// Each link's streams' first sound and last end, and the offsets that
    /// put each link's times after the last's.
    fn time_links(&mut self) -> Result<(), Error> {
        let count = self.streams.len();
        let mut offsets = vec![0i64; count];
        for k in 0..self.links.len() {
            let first = self.first_sounds(k)?;
            let last = self.last_ends(k)?;
            if let Some(before) = k.checked_sub(1).and_then(|b| self.links.get(b)) {
                for (i, offset) in offsets.iter_mut().enumerate() {
                    let end = before.last.get(i).copied().flatten();
                    let end = end.or_else(|| before.first.get(i).copied().flatten());
                    if let Some(end) = end {
                        *offset = end.saturating_sub(first.get(i).copied().flatten().unwrap_or(0));
                    }
                }
            }
            let Some(link) = self.links.get_mut(k) else {
                break;
            };
            for (s, &o) in link.streams.iter_mut().zip(&offsets) {
                s.offset = o;
            }
            let moved = |v: &[Option<i64>]| -> Vec<Option<i64>> {
                v.iter()
                    .zip(&offsets)
                    .map(|(t, &o)| t.map(|t| t.saturating_add(o)))
                    .collect()
            };
            link.first = moved(&first);
            link.last = moved(&last);
        }
        // The first link's live streams were made before the offsets (all 0
        // in it).
        Ok(())
    }

    /// Where each of link `k`'s streams' sound begins, in the link's own
    /// times: the first packet's start, after Opus's pre-skip -- or, for
    /// Vorbis, whose first packet only primes the decoder, its end.
    fn first_sounds(&mut self, k: usize) -> Result<Vec<Option<i64>>, Error> {
        let Some(link) = self.links.get(k) else {
            return Ok(Vec::new());
        };
        let (mut at, end) = (link.data_start, link.end);
        let mut states = link.streams.clone();
        for s in &mut states {
            s.offset = 0;
            s.jump(true, None);
        }
        let mut first: Vec<Option<i64>> = vec![None; states.len()];
        let wanted = |first: &[Option<i64>], states: &[State]| {
            first
                .iter()
                .zip(states)
                .any(|(f, s)| f.is_none() && s.info.codec.is_timed() && !s.ended)
        };
        let mut read = 0;
        while read < START_SEARCH && wanted(&first, &states) {
            let Some(page) = self.pages.next_page(at)? else {
                break;
            };
            if page.position >= end {
                break;
            }
            at = page.end();
            read += 1;
            let Some(index) = states.iter().position(|s| s.info.serial == page.serial) else {
                continue;
            };
            let Some(state) = states.get_mut(index) else {
                continue;
            };
            let codec = state.info.codec;
            for p in take_page(state, &page, index) {
                if let (Some(slot @ None), Some(pts)) = (first.get_mut(index), p.pts) {
                    *slot = Some(match codec {
                        Codec::Vorbis => pts.saturating_add(i64::try_from(p.duration).unwrap_or(0)),
                        _ => pts.saturating_add(i64::from(p.skip_samples)),
                    });
                }
            }
        }
        Ok(first)
    }

    /// Where each of link `k`'s streams' sound ends, in the link's own
    /// times: its last page's granule position, less Opus's pre-skip.
    fn last_ends(&mut self, k: usize) -> Result<Vec<Option<i64>>, Error> {
        let Some(link) = self.links.get(k) else {
            return Ok(Vec::new());
        };
        let streams: Vec<(u32, i64, bool)> = link
            .streams
            .iter()
            .map(|s| {
                (
                    s.info.serial,
                    i64::from(s.info.pre_skip),
                    s.info.codec.is_timed(),
                )
            })
            .collect();
        let floor = link.end.saturating_sub(END_SEARCH).max(link.data_start);
        let mut ends: Vec<Option<i64>> = vec![None; streams.len()];
        let mut hi = link.end;
        while hi > floor && ends.iter().zip(&streams).any(|(e, s)| e.is_none() && s.2) {
            let lo = hi.saturating_sub(LINEAR).max(floor);
            let mut found: Vec<Option<i64>> = vec![None; streams.len()];
            let mut at = lo;
            while let Some(page) = self.pages.next_page(at)? {
                if page.position >= hi {
                    break;
                }
                at = page.end();
                if page.granule == -1 {
                    continue;
                }
                if let Some(i) = streams.iter().position(|s| s.0 == page.serial) {
                    if let Some(slot) = found.get_mut(i) {
                        *slot = Some(page.granule);
                    }
                }
            }
            for ((end, found), s) in ends.iter_mut().zip(found).zip(&streams) {
                if end.is_none() && s.2 {
                    *end = found.map(|g| g.saturating_sub(s.1));
                }
            }
            hi = lo;
        }
        Ok(ends)
    }
}

/// Reads a link's first pages from `start`: its streams' first pages and
/// their headers, up to the first page that holds data. Returns the link
/// (its end the file's, until a later link is found), its streams as
/// reading leaves them, that page's packets, and where reading goes on.
fn read_link<R: Read + Seek>(
    pages: &mut Pages<R>,
    start: u64,
) -> Result<(Link, Vec<State>, Vec<Packet>, u64), Error> {
    let mut states: Vec<State> = Vec::new();
    let mut at = start;
    let mut packets = Vec::new();
    // A stream starts only on a first page, and only before a page that
    // starts none: RFC 3533 puts every stream's first page first.
    let mut started = false;
    let mut data_start = None;
    while let Some(page) = pages.next_page(at)? {
        let known = states.iter().position(|s| s.info.serial == page.serial);
        let index = match known {
            Some(i) => Some(i),
            None if page.has(FIRST) && !started => {
                states.push(State::new(page.serial));
                Some(states.len() - 1)
            }
            None => None,
        };
        if !page.has(FIRST) {
            started = true;
        }
        at = page.end();
        let Some(state) = index.and_then(|i| states.get_mut(i)) else {
            continue;
        };
        let got = take_page(state, &page, index.unwrap_or(0));
        if !got.is_empty() {
            data_start = Some(page.position);
            packets = got;
            break;
        }
    }
    if states.is_empty() {
        return Err(Error::Invalid("no stream starts here"));
    }
    let link = Link {
        data_start: data_start.unwrap_or(at),
        end: pages.len(),
        streams: states.clone(),
        first: Vec::new(),
        last: Vec::new(),
    };
    Ok((link, states, packets, at))
}

/// A page of a stream: its packets put together, its headers taken, and its
/// data timed.
fn take_page(state: &mut State, page: &Page, index: usize) -> Vec<Packet> {
    let assembled = state.assemble(page);
    let data: Vec<_> = assembled
        .into_iter()
        .filter(|p| !state.header(&p.data))
        .collect();
    if data.is_empty() {
        if page.has(LAST) {
            state.ended = true;
        }
        return Vec::new();
    }
    state.time(page, data, index)
}
