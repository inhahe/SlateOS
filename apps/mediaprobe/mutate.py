"""Mutation test for mediaprobe's Matroska demuxer (`src/mkv/demux.rs`).

Each row puts back one way of not reading Matroska as RFC 9559 says --
a flag ignored, a lacing misread, a cluster's end missed, a cue passed
over -- and names the tests that have to notice. The decisive ones demux
libvpx's VP9 vectors and hold every picture decoded from them to libvpx's
MD5s (`tests/vp9_vectors.rs`); the rest lay files out block by block.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "mkv" / "demux.rs"

ORDER = "simple_blocks_come_out_in_order_with_their_times"
LACES = "each_lacing_gives_its_frames"
BAD_LACE = "a_lace_that_does_not_add_up_is_dropped"
GROUP = "a_block_group_is_a_key_frame_without_a_reference"
UNKNOWN = "a_cluster_of_unknown_length_ends_where_the_next_begins"
SEEK = "a_seek_goes_to_the_cluster_of_the_time_sought_with_cues_or_without"
CUE = "a_seek_goes_to_a_cue_not_past_it"
WALK = "a_walk_stops_at_the_first_cluster_past_the_time"
SCALE = "the_timestamp_scale_is_the_files"
ENCODING = "header_stripping_is_undone_and_other_compression_passed_over"
RESYNC = "bad_bytes_are_searched_past_for_the_next_cluster"
VECTORS = "every_frame_demuxed_decodes_to_libvpx_pictures"
KEY = "a_seek_lands_on_a_frame_decoding_can_start_at"
DELAY = "a_codec_delay_is_taken_off_its_tracks_timestamps"
PRE_ROLL = "a_seek_starts_a_pre_roll_before_the_time_sought"

DEMUX = [
    (
        "a SimpleBlock's key flag is ignored",
        "keyframe.unwrap_or(block.flags & 0x80 != 0)",
        "keyframe.unwrap_or(false)",
        [ORDER, KEY],
    ),
    (
        "a block group with a reference is a key frame",
        "self.queue_block(block, c.time, Some(!referenced), duration);",
        "self.queue_block(block, c.time, Some(true), duration);",
        [GROUP],
    ),
    (
        "Xiph lacing stops at 254",
        "                    if byte != 255 {",
        "                    if byte < 254 {",
        [LACES],
    ),
    (
        "EBML lacing differences are taken unsigned",
        "size = size.checked_add(i128::from(raw).checked_sub(bias)?)?;",
        "size = size.checked_add(i128::from(raw))?;",
        [LACES],
    ),
    (
        "fixed lacing need not divide evenly",
        "            if rest.checked_rem(count)? != 0 {",
        "            if false {",
        [BAD_LACE],
    ),
    (
        "laced frames share the first one's time",
        "            let offset = default_ns.map_or(0, |d| {",
        "            let offset = None::<u64>.map_or(0, |d| {",
        [LACES],
    ),
    (
        "every laced frame is a key frame",
        "                keyframe: keyframe && i == 0,",
        "                keyframe,",
        [LACES],
    ),
    (
        "a cluster of unknown length runs on into the next",
        "        if TOP_LEVEL.contains(&el.id) {",
        "        if false {",
        [UNKNOWN],
    ),
    (
        "a cluster's timestamp is ignored",
        "                next.time = uint(&b).unwrap_or(0);",
        "                next.time = 0;",
        [ORDER],
    ),
    (
        "the timestamp scale is a millisecond whatever the file says",
        "                        self.scale = s;",
        "                        let _ = s;",
        [SCALE],
    ),
    (
        "a seek walks the clusters though there are cues",
        "        let target = if self.cues.is_empty() {",
        "        let target = if true {",
        [CUE],
    ),
    (
        "a seek takes the first cue, not the last before the time",
        "                .max_by_key(|c| c.time)",
        "                .min_by_key(|c| c.time)",
        [SEEK],
    ),
    (
        "a walk does not stop at a later cluster",
        "                Some(t) if t > ticks => break,",
        "                Some(t) if t > ticks => {}",
        [WALK],
    ),
    (
        "a stripped header is not put back",
        "            data.extend_from_slice(&stream.strip);",
        "            let _ = &stream.strip;",
        [ENCODING],
    ),
    (
        "a compressed track is read as it is",
        "                    } else {\n                        stream.readable = false;",
        "                    } else {\n                        let _ = &stream;",
        [ENCODING],
    ),
    (
        "bad bytes are not searched past",
        "            if let Some(i) = chunk.windows(4).position(|w| w == CLUSTER_ID_BYTES) {",
        "            if let Some(i) = None::<usize> {",
        [RESYNC],
    ),
    (
        "a block's track number keeps its marker bit",
        "    let (track, n) = vint(b, 0, false)?;",
        "    let (track, n) = vint(b, 0, true)?;",
        [ORDER, VECTORS],
    ),
    (
        "a codec delay is not taken off the timestamps",
        "i64::try_from(start.saturating_add(offset).saturating_sub(delay))",
        "i64::try_from(start.saturating_add(offset))",
        [DELAY],
    ),
    (
        "a seek starts no pre-roll early",
        "        let block_ns = time_ns.saturating_add(delay).saturating_sub(pre_roll);",
        "        let block_ns = time_ns.saturating_add(delay);",
        [PRE_ROLL],
    ),
    (
        "a seek reads the played time as block time",
        "        let block_ns = time_ns.saturating_add(delay).saturating_sub(pre_roll);",
        "        let block_ns = time_ns.saturating_sub(pre_roll);",
        [PRE_ROLL],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:]
    raise SystemExit(sweep(SRC, DEMUX, "mediaprobe", timeout=900, only=only or None))
