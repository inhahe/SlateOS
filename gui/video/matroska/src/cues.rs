//! The Cues: the file's index of where to start decoding for a time -- each
//! cue point a time and, for each track it covers, the Cluster holding a key
//! frame at that time. Read as FFmpeg reads them into its index
//! (`matroska_add_index_entries`).

use std::io::{Read, Seek};

use crate::ebml::{Header, Reader};
use crate::{Error, ids};

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

/// The index in a `Cues` element, ordered by time; empty where FFmpeg drops
/// it -- fewer than two cue points, or a second cue's time past 10^14
/// nanoseconds ("apparently broken").
///
/// # Errors
///
/// When the element is damaged.
pub(crate) fn read<R: Read + Seek>(
    r: &mut Reader<R>,
    cues: &Header,
    segment_data: u64,
    timestamp_scale: u64,
) -> Result<Vec<Cue>, Error> {
    let mut points: Vec<(u64, Vec<(u64, u64)>)> = Vec::new();
    r.children(cues, |r, point| {
        if point.id != ids::CUE_POINT {
            return Ok(());
        }
        let mut time = 0;
        let mut positions = Vec::new();
        r.children(point, |r, c| {
            match c.id {
                ids::CUE_TIME => time = r.uint(c.size, 0)?,
                ids::CUE_TRACK_POSITIONS => {
                    let (mut track, mut cluster) = (0, 0);
                    r.children(c, |r, p| {
                        match p.id {
                            ids::CUE_TRACK => track = r.uint(p.size, 0)?,
                            ids::CUE_CLUSTER_POSITION => cluster = r.uint(p.size, 0)?,
                            _ => {}
                        }
                        Ok(())
                    })?;
                    positions.push((track, cluster));
                }
                _ => {}
            }
            Ok(())
        })?;
        points.push((time, positions));
        Ok(())
    })?;
    if points.len() < 2 {
        return Ok(Vec::new());
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "FFmpeg's own comparison, in double"
    )]
    let broken = points
        .get(1)
        .is_some_and(|(t, _)| *t as f64 > 1e14 / timestamp_scale.max(1) as f64);
    if broken {
        return Ok(Vec::new());
    }
    let mut out: Vec<Cue> = points
        .into_iter()
        .flat_map(|(time, positions)| {
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
    Ok(out)
}
