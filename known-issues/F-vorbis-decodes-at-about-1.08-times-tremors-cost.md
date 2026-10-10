### [F] Vorbis decodes at about 1.08 times Tremor's cost -- 2026-10-04

**Status:** OPEN — lane F's; tech debt, not a bug. Mostly paid on
2026-10-04 (it was 1.52 times).

**In short:** the Vorbis decoder (`gui/video/vorbis`) does about a twelfth
more work than Tremor's own C to decode the same sound -- the same samples
to the bit, just a little more slowly: 675 million instructions to
Tremor's 627 on its benchmark (the 20 encoded test streams, 27.6 s of
sound; callgrind, so other load does not move the counts), about 460 times
faster than real time against Tremor's 520. Nothing a user does is held up
by it.

**Done** (design-decisions §1352): the MDCT walks the block in fixed-size
chunks, its butterfly stages monomorphised by stride so that each turn's
four twiddles are one chunk of the table read at constant offsets; the bit
reader reads eight bytes at once where it can; residue type 2 has fast
paths for one and two channels; floor 1 renders its lines over a slice.

**Where the rest is** (callgrind, port against Tremor, millions of
instructions, each inclusive):

| | port | Tremor |
|---|---|---|
| inverse MDCT (`mdct::backward`) | 275 | 232 |
| residues (`Residue::inverse`, the codebooks under it) | 169 | 145 |
| floor 1's curve (`Floor1::inverse2`) | 45 | 39 |
| the window (`window::apply`) | 41 | 38 |

**The proper fix, the rest:**
1. The MDCT's pre-rotation, bit-reversal and post-rotation still index the
   twiddle table with bounds checks and branch on which half of the walk
   they are in; and the whole transform spills registers (299 stack
   references in its 1847 instructions). Splitting each loop at its half
   and monomorphising the post-rotation's stride as the butterflies' is
   the next step.
2. The residue's `decodev_add` and `decodevs_add` (types 1 and 0) still
   index their target with a check per value; and `Residue::inverse`
   allocates its list of coded channels each call.
3. Re-measure with `tools/profile.sh` (callgrind, in WSL) after each, the
   references (`tests/streams.rs`, `tests/damage.rs`) holding every change
   to Tremor's samples.
