### [F] The VP9 encoder makes libvpx's decisions only above 352x288 -- 2026-10-04 -- **FIXED 2026-10-04**

**Status:** FIXED 2026-10-04 (lane F); on `main` since the boot-tested
publication of `cc0cc9137`. The
learned partitioning is ported (`enc/mlpart.rs`, `enc/nonrd.rs`'s
`pick_partition`), with glibc's `logf` as the reference machine runs it
(`enc/glibcmath.rs`). A third reference encode at 350x286,
`tests/data/encoder/rt8small.ivf`, is byte-identical for all 90 frames from
the encoder's own decisions and from libvpx's replayed, and the decision
traces agree on all 152,823 lines. The 320x240 `disable_16x16part_nonkey`
note below needs nothing: it changes only variance thresholds, which a
frame partitioned by search never reads.

**In short:** the encoder reproduces libvpx's realtime encoder byte for byte
on a 1280x720 clip, but for a picture of 352x288 pixels or fewer (a small
window, a thumbnail-sized stream) libvpx cuts each inter frame's blocks a
different way: by trying the cuts and keeping the cheapest, pruned by a
small neural network. That way of cutting is not ported, so at those sizes
every frame after the first is a valid stream -- it decodes, and rate
control holds the bitrate -- but not the one `vpxenc` would write.

**Where.** libvpx v1.17.0's `vp9_speed_features.c` sets
`sf->nonrd_use_ml_partition` at speed 8 for any inter frame of
`width * height <= 352 * 288`, which makes `partition_search_type`
`ML_BASED_PARTITION`. `vp9_encodeframe.c`'s `encode_nonrd_sb_row` then calls
`get_estimated_pred` and `nonrd_pick_partition` instead of
`choose_partitioning` and `nonrd_use_partition`; `nonrd_pick_partition` asks
`ml_predict_var_partitioning` (the networks `vp9_var_part_nnconfig_64`, `_32`
and `_16` in `vp9_partition_models.h`) whether to try the whole block, its
split, or both, and runs `vp9_pick_inter_mode` at each level it keeps. The
port's `gui/video/vp9/src/enc/nonrd.rs` always takes the variance
partitioning (`InterDecisions::choose_partitioning`).

**Already libvpx's at those sizes:** key frames (the learned partitioning is
for inter frames only); the speed-8 filter search on every block rather than
a chessboard, and `adaptive_rd_thresh` 2 rather than 1
(`enc/pickinter.rs`, `speed8_cb_pred_filter_search`,
`speed8_adaptive_rd_thresh`); the low-resolution branches of the variance
thresholds, cyclic refresh, rate control and noise estimate.

**Also needed with it:** at 320x240 and below, after frame 8 and once the
average inter quantiser passes 208, libvpx also sets
`sf->disable_16x16part_nonkey` (it is computed before the partition type
switches to the learned one); the port passes `false`
(`partition::inter_thresholds`'s `disable_16x16`).

**How to reproduce.** Encode any clip at 352x288 with `EncoderConfig::realtime`
and compare with `vpxenc` at the settings in
`gui/video/vp9/tests/data/encoder/README.md`: frame 0 matches, frame 1 does
not.

**The proper fix.** Port `get_estimated_pred`, `nonrd_pick_partition` (its
real-time path: square splits only, `x->sb_pickmode_part`, the
`pc_tree` contexts it keeps per level) and `ml_predict_var_partitioning` with
its three networks, and check them against a second reference encode at
352x288 with the decision trace (`enc/trace.rs`, the instrumented libvpx
described beside it).
