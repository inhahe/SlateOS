//! The Cues: the file's index of where to start decoding for a time -- each
//! cue point a time and, for each track it covers, the Cluster holding a key
//! frame at that time. Read as FFmpeg reads them into its index
//! (`matroska_index`, then `matroska_add_index_entries`).
//!
//! FFmpeg keeps one list of cue points for the file, and every Cues element
//! it reads adds to it: all those before the first Cluster, or else the one
//! the SeekHead names, read at the first seek. A damaged Cues element adds
//! the points before its damage, and the one damaged as far as it got
//! (`nest.rs`'s rules). The index is made of the whole list.

use std::io::{Read, Seek};

use crate::ebml::{Header, Reader};
use crate::{Error, ids, nest};

/// One track's entry in the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cue {
    /// In the segment's ticks (for an index of key frames found by reading,
    /// the frames' own timestamps).
    pub time: i64,
    pub track: u64,
    /// Where the Cluster begins in the file (not, as written, in the
    /// segment).
    pub cluster: u64,
}

/// One cue point, as FFmpeg's `MatroskaIndex` holds it: its time, and each
/// `(track, Cluster position)` it gives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Point {
    pub time: u64,
    pub positions: Vec<(u64, u64)>,
}

/// A Cues element's points, appended to `out`.
///
/// # Errors
///
/// When the element is damaged; the points before the damage stay in
/// `out`, and the one damaged as far as it got.
pub(crate) fn read_points<R: Read + Seek>(
    r: &mut Reader<R>,
    cues: &Header,
    levels: u32,
    out: &mut Vec<Point>,
) -> Result<(), Error> {
    let levels = nest::enter(levels)?;
    let end = nest::end_of(cues)?;
    r.seek_to(cues.data)?;
    loop {
        let c = match nest::child(r, end) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(()),
            Err(b) => {
                if b.id == Some(ids::CUE_POINT) {
                    out.push(Point::default());
                }
                return Err(b.error);
            }
        };
        if c.id != ids::CUE_POINT {
            nest::skip(r, &c)?;
            continue;
        }
        out.push(Point::default());
        let Some(point) = out.last_mut() else {
            return Ok(());
        };
        read_point(r, &c, levels, point)?;
    }
}

fn read_point<R: Read + Seek>(
    r: &mut Reader<R>,
    h: &Header,
    levels: u32,
    point: &mut Point,
) -> Result<(), Error> {
    let levels = nest::enter(levels)?;
    let end = nest::end_of(h)?;
    loop {
        let c = match nest::child(r, end) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(()),
            Err(b) => {
                if b.id == Some(ids::CUE_TRACK_POSITIONS) {
                    point.positions.push((0, 0));
                }
                return Err(b.error);
            }
        };
        match c.id {
            ids::CUE_TIME => nest::uint(r, &c, 0, &mut point.time)?,
            ids::CUE_TRACK_POSITIONS => {
                point.positions.push((0, 0));
                let Some((track, cluster)) = point.positions.last_mut() else {
                    return Ok(());
                };
                nest::enter(levels)?;
                let positions_end = nest::end_of(&c)?;
                while let Some(f) = nest::child(r, positions_end).map_err(|b| b.error)? {
                    match f.id {
                        ids::CUE_TRACK => nest::uint(r, &f, 0, track)?,
                        ids::CUE_CLUSTER_POSITION => nest::uint(r, &f, 0, cluster)?,
                        _ => nest::skip(r, &f)?,
                    }
                }
            }
            _ => nest::skip(r, &c)?,
        }
    }
}

/// FFmpeg's index of the file's cue points (`matroska_add_index_entries`),
/// ordered by time: none for fewer than two points, or a second point's
/// time past 10^14 nanoseconds ("apparently broken"); else every position
/// of every point.
pub(crate) fn index(points: Vec<Point>, segment_data: u64, timestamp_scale: u64) -> Vec<Cue> {
    if points.len() < 2 {
        return Vec::new();
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "FFmpeg's own comparison, in double"
    )]
    let broken = points
        .get(1)
        .is_some_and(|p| p.time as f64 > 1e14 / timestamp_scale.max(1) as f64);
    if broken {
        return Vec::new();
    }
    let mut out: Vec<Cue> = points
        .into_iter()
        .flat_map(|Point { time, positions }| {
            positions.into_iter().filter_map(move |(track, cluster)| {
                Some(Cue {
                    time: i64::try_from(time).unwrap_or(i64::MAX),
                    track,
                    cluster: cluster.checked_add(segment_data)?,
                })
            })
        })
        .collect();
    // FFmpeg's index is sorted by time; a stable sort keeps the file's order
    // among equal times.
    out.sort_by_key(|c| c.time);
    out
}
