### [F] The VP9 encoder's edge extension is libvpx's, but no reference encode tests it -- 2026-10-04

**Status:** OPEN — answered on lane-f 2026-10-04, and moves to
`known-issues-resolved/` once that has had a boot test on `main`. The
second reference encode, `tests/data/encoder/rt8cut.ivf`, is 651x357: its
last superblock row and column hang over the picture, and its last 8x8
cells are partly outside it. All 150 of its frames come out byte-identical
both from the encoder's own decisions (`tests/encoder.rs`,
`frames_match_vpxenc_through_cuts_noise_and_edges`) and from libvpx's
replayed (`enc/replay.rs`,
`libvpxs_decisions_give_libvpxs_frames_through_cuts_noise_and_edges`).

**In short:** the encoder is checked byte for byte against libvpx's own
encode of a 1280x720 clip. One thing it does to match libvpx -- after each
frame, it copies the picture's edge pixels out over the rest of its buffer,
because a block that hangs over the picture's bottom or right edge predicts
from those copies in libvpx -- never matters in that clip, so nothing would
notice if it went wrong. Removing it entirely still passes every test.

**Where.** `gui/video/vp9/src/enc/encoder.rs`, `extend_frame` (libvpx's
`vpx_extend_frame_inner_borders`, run after the loop filter). A block that
overhangs the picture and reads its reference without moving (a zero vector,
or any vector whose reach stays inside the buffer) predicts from these
pixels; its transform then codes them, so the frame's bytes depend on them.
A decoder makes different pixels there, which no shown pixel depends on --
so the round-trip tests, which compare what a decoder shows, cannot see them
either.

**Why the reference misses it.** 720 rows leave the last superblock row two
8x8 rows deep, and a 64x64 block there can only be cut into halves that end
inside the picture or into smaller blocks; libvpx's choices in the 30 frames
never overhang. 1280 is a whole number of superblocks, so nothing overhangs
the right edge either. `enc/replay.rs`, which codes libvpx's own decisions
and compares bytes, passes with the extension removed.

**The proper fix.** Once the encoder makes its own inter decisions, add a
second reference encode (`vpxenc` as in `tests/data/encoder/README.md`) at a
size whose last superblock row admits an overhanging block -- 640x360, say:
45 rows of 8x8 cells, so a 64x64 at row 40 has five rows inside and three
outside -- and run both the replay and the full comparison over it.
