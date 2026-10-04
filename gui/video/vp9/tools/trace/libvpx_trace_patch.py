#!/usr/bin/env python3
"""Instrument a copy of libvpx v1.17.0 with decision traces for the VP9
realtime encoder port (`gui/video/vp9/src/enc/trace.rs` writes the same
lines from the port; README.md beside this file has the whole procedure).

Usage: python3 libvpx_trace_patch.py <libvpx-root>

Patch a copy, never the tree a reference encode is made with: the traces
cost nothing when $VP9TRACE is unset, but a reference is made by the
unpatched encoder. Every edit is an exact anchor replacement; the script
fails loudly if an anchor is missing or ambiguous, so a silent half-patch
cannot happen.
"""
import sys
from pathlib import Path

ROOT = Path(sys.argv[1])

TRACE_H = r'''#ifndef VP9_TRACE_H_
#define VP9_TRACE_H_
#include <stdio.h>
#include <stdlib.h>
FILE *vp9_trace_fp(void);
#define VP9T(...)                                \
  do {                                           \
    FILE *vp9t_f_ = vp9_trace_fp();              \
    if (vp9t_f_) fprintf(vp9t_f_, __VA_ARGS__);  \
  } while (0)
#endif
'''


def patch(rel, edits):
    p = ROOT / rel
    text = p.read_text(encoding="utf-8")
    for anchor, new in edits:
        n = text.count(anchor)
        if n != 1:
            raise SystemExit(f"{rel}: anchor found {n} times:\n{anchor}")
        text = text.replace(anchor, new)
    p.write_text(text, encoding="utf-8", newline="")
    print(f"patched {rel}: {len(edits)} edits")


(ROOT / "vp9/encoder/vp9_trace.h").write_text(TRACE_H, encoding="utf-8", newline="")

ENC_FRAME = "vp9/encoder/vp9_encodeframe.c"
patch(ENC_FRAME, [
    # The trace file, opened once from $VP9TRACE.
    ("static void encode_superblock(VP9_COMP *cpi, ThreadData *td, TOKENEXTRA **t,\n"
     "                              int output_enabled, int mi_row, int mi_col,\n"
     "                              BLOCK_SIZE bsize, PICK_MODE_CONTEXT *ctx);\n",
     "#include \"vp9/encoder/vp9_trace.h\"\n"
     "FILE *vp9_trace_fp(void) {\n"
     "  static FILE *f = NULL;\n"
     "  static int init = 0;\n"
     "  if (!init) {\n"
     "    const char *p = getenv(\"VP9TRACE\");\n"
     "    init = 1;\n"
     "    if (p) f = fopen(p, \"w\");\n"
     "  }\n"
     "  return f;\n"
     "}\n"
     "static void encode_superblock(VP9_COMP *cpi, ThreadData *td, TOKENEXTRA **t,\n"
     "                              int output_enabled, int mi_row, int mi_col,\n"
     "                              BLOCK_SIZE bsize, PICK_MODE_CONTEXT *ctx);\n"),
    # Per-superblock summary helper.
    ("// This function chooses partitioning based on the variance between source and\n",
     "static void trace_sb(MACROBLOCK *x, int mi_row, int mi_col, int segment_id,\n"
     "                     unsigned int y_sad, char path) {\n"
     "  int i;\n"
     "  unsigned int vl = 0;\n"
     "  for (i = 0; i < 25; ++i) vl |= (unsigned int)(x->variance_low[i] != 0) << i;\n"
     "  VP9T(\"S %d %d seg=%d cs=%d lshc=%d ysad=%u p=%c vl=%x col=%d%d skin=%d smp=%d,%d,%d pmv=%d,%d\\n\",\n"
     "       mi_row, mi_col, segment_id, (int)x->content_state_sb,\n"
     "       x->last_sb_high_content, y_sad, path, vl, x->color_sensitivity[0],\n"
     "       x->color_sensitivity[1], x->sb_is_skin, x->sb_use_mv_part,\n"
     "       x->sb_mvrow_part, x->sb_mvcol_part, x->pred_mv[LAST_FRAME].row,\n"
     "       x->pred_mv[LAST_FRAME].col);\n"
     "}\n"
     "\n"
     "// This function chooses partitioning based on the variance between source and\n"),
    # Copy without y_sad (low source sad).
    ("        copy_partitioning(cpi, x, xd, mi_row, mi_col, segment_id, sb_offset)) {\n"
     "      x->sb_use_mv_part = 1;\n",
     "        copy_partitioning(cpi, x, xd, mi_row, mi_col, segment_id, sb_offset)) {\n"
     "      x->sb_use_mv_part = 1;\n"
     "      trace_sb(x, mi_row, mi_col, segment_id, UINT_MAX, 'C');\n"),
    # 64x64 early exit.
    ("          update_partition_svc(cpi, BLOCK_64X64, mi_row, mi_col);\n"
     "        if (cpi->sf.copy_partition_flag) {\n"
     "          update_prev_partition(cpi, x, segment_id, mi_row, mi_col, sb_offset);\n"
     "        }\n"
     "        return 0;\n",
     "          update_partition_svc(cpi, BLOCK_64X64, mi_row, mi_col);\n"
     "        if (cpi->sf.copy_partition_flag) {\n"
     "          update_prev_partition(cpi, x, segment_id, mi_row, mi_col, sb_offset);\n"
     "        }\n"
     "        trace_sb(x, mi_row, mi_col, segment_id, y_sad, 'E');\n"
     "        return 0;\n"),
    # Copy after y_sad.
    ("      chroma_check(cpi, x, bsize, y_sad, is_key_frame, scene_change_detected);\n"
     "      if (cpi->sf.svc_use_lowres_part &&\n"
     "          cpi->svc.spatial_layer_id == cpi->svc.number_spatial_layers - 2)\n"
     "        update_partition_svc(cpi, BLOCK_64X64, mi_row, mi_col);\n"
     "      return 0;\n",
     "      chroma_check(cpi, x, bsize, y_sad, is_key_frame, scene_change_detected);\n"
     "      if (cpi->sf.svc_use_lowres_part &&\n"
     "          cpi->svc.spatial_layer_id == cpi->svc.number_spatial_layers - 2)\n"
     "        update_partition_svc(cpi, BLOCK_64X64, mi_row, mi_col);\n"
     "      trace_sb(x, mi_row, mi_col, segment_id, y_sad, 'P');\n"
     "      return 0;\n"),
    # Full variance partitioning.
    ("  chroma_check(cpi, x, bsize, y_sad, is_key_frame, scene_change_detected);\n"
     "  if (vt2) vpx_free(vt2);\n",
     "  chroma_check(cpi, x, bsize, y_sad, is_key_frame, scene_change_detected);\n"
     "  trace_sb(x, mi_row, mi_col, segment_id, y_sad, is_key_frame ? 'K' : 'V');\n"
     "  if (vt2) vpx_free(vt2);\n"),
    # The stale mode info set_low_temp_var_flag reads.
    ("  const int mv_thr = cm->width > 640 ? 8 : 4;\n",
     "  const int mv_thr = cm->width > 640 ? 8 : 4;\n"
     "  VP9T(\"L %d %d mv=%d,%d sbt=%d rfp=%d\\n\", mi_row, mi_col,\n"
     "       xd->mi[0]->mv[0].as_mv.row, xd->mi[0]->mv[0].as_mv.col,\n"
     "       xd->mi[0]->sb_type, ref_frame_partition);\n"),
    # Per block, after the mode search.
    ("  duplicate_mode_info_in_sb(cm, xd, mi_row, mi_col, bsize);\n"
     "\n"
     "  for (plane = 0; plane < MAX_MB_PLANE; ++plane) {\n"
     "    struct macroblockd_plane *pd = &xd->plane[plane];\n"
     "    memcpy(pd->above_context, a + num_4x4_blocks_wide * plane,\n",
     "  duplicate_mode_info_in_sb(cm, xd, mi_row, mi_col, bsize);\n"
     "  VP9T(\"B %d %d bs=%d seg=%d m=%d r=%d mv=%d,%d f=%d tx=%d sk=%d skt=%d rate=%d dist=%lld\\n\",\n"
     "       mi_row, mi_col, bsize, mi->segment_id, mi->mode, mi->ref_frame[0],\n"
     "       mi->mv[0].as_mv.row, mi->mv[0].as_mv.col, mi->interp_filter,\n"
     "       mi->tx_size, x->skip, x->skip_txfm[0], rd_cost->rate,\n"
     "       (long long)rd_cost->dist);\n"
     "\n"
     "  for (plane = 0; plane < MAX_MB_PLANE; ++plane) {\n"
     "    struct macroblockd_plane *pd = &xd->plane[plane];\n"
     "    memcpy(pd->above_context, a + num_4x4_blocks_wide * plane,\n"),
    # The cyclic refresh's segment after the mode search.
    ("      vp9_cyclic_refresh_update_segment(cpi, mi, mi_row, mi_col, bsize,\n"
     "                                        ctx->rate, ctx->dist, x->skip, p);\n",
     "      vp9_cyclic_refresh_update_segment(cpi, mi, mi_row, mi_col, bsize,\n"
     "                                        ctx->rate, ctx->dist, x->skip, p);\n"
     "      VP9T(\"U %d %d seg=%d\\n\", mi_row, mi_col, mi->segment_id);\n"),
    # Frame-level state, just before the tiles are coded.
    ("  // Frame segmentation\n"
     "  if (cpi->oxcf.aq_mode == PERCEPTUAL_AQ) build_kmeans_segmentation(cpi);\n",
     "  VP9T(\"F %u q=%d kf=%d rdm=%d epb=%d spb=%d rff=%d fsg=%d ne=%d,%d,%d hss=%d \"\n"
     "       \"scl=%d sef=%d seg=%d crd=%d,%d crrd=%d trs=%lld tds=%lld sbi=%d \"\n"
     "       \"tfg=%d vbp=%lld,%lld,%lld,%lld vsad=%lld vcopy=%lld alm=%d hp=%d \"\n"
     "       \"ifl=%d txm=%d\\n\",\n"
     "       cm->current_video_frame, cm->base_qindex, cm->frame_type == KEY_FRAME,\n"
     "       cpi->rd.RDMULT, x->errorperbit, x->sadperbit16, cpi->ref_frame_flags,\n"
     "       cpi->rc.frames_since_golden, cpi->noise_estimate.enabled,\n"
     "       cpi->noise_estimate.value, (int)cpi->noise_estimate.level,\n"
     "       cpi->rc.high_source_sad, cpi->sf.short_circuit_low_temp_var,\n"
     "       cpi->sf.skip_encode_frame, cm->seg.enabled,\n"
     "       cpi->cyclic_refresh->qindex_delta[1],\n"
     "       cpi->cyclic_refresh->qindex_delta[2], cpi->cyclic_refresh->rdmult,\n"
     "       (long long)cpi->cyclic_refresh->thresh_rate_sb,\n"
     "       (long long)cpi->cyclic_refresh->thresh_dist_sb,\n"
     "       cpi->cyclic_refresh->sb_index, cpi->rc.frames_till_gf_update_due,\n"
     "       (long long)cpi->vbp_thresholds[0], (long long)cpi->vbp_thresholds[1],\n"
     "       (long long)cpi->vbp_thresholds[2], (long long)cpi->vbp_thresholds[3],\n"
     "       (long long)cpi->vbp_threshold_sad, (long long)cpi->vbp_threshold_copy,\n"
     "       cpi->rc.avg_frame_low_motion, cm->allow_high_precision_mv,\n"
     "       (int)cm->interp_filter, (int)cm->tx_mode);\n"
     "  // Frame segmentation\n"
     "  if (cpi->oxcf.aq_mode == PERCEPTUAL_AQ) build_kmeans_segmentation(cpi);\n"),
])

PICKMODE = "vp9/encoder/vp9_pickmode.c"
patch(PICKMODE, [
    ("#include \"vp9/encoder/vp9_rd.h\"\n",
     "#include \"vp9/encoder/vp9_rd.h\"\n"
     "#include \"vp9/encoder/vp9_trace.h\"\n"),
    # The block's search setup.
    ("  for (ref_frame = LAST_FRAME; ref_frame <= usable_ref_frame; ++ref_frame) {\n"
     "    // Skip find_predictor if the reference frame is not in the\n",
     "  VP9T(\"P %d %d bs=%d urf=%d fslt=%d lshc=%d ipen=%d sef=%d\\n\", mi_row, mi_col,\n"
     "       bsize, usable_ref_frame, force_skip_low_temp_var,\n"
     "       x->last_sb_high_content, intra_cost_penalty, x->skip_encode);\n"
     "  for (ref_frame = LAST_FRAME; ref_frame <= usable_ref_frame; ++ref_frame) {\n"
     "    // Skip find_predictor if the reference frame is not in the\n"),
    # The candidate vectors per reference.
    ("  if (cpi->use_svc || cpi->oxcf.speed <= 7 || bsize < BLOCK_32X32)\n"
     "    x->sb_use_mv_part = 0;\n",
     "  {\n"
     "    int r;\n"
     "    for (r = LAST_FRAME; r <= usable_ref_frame; ++r)\n"
     "      if (!skip_ref_find_pred[r])\n"
     "        VP9T(\"R %d nearest=%d,%d near=%d,%d ctx=%d bri=%d psad=%d\\n\", r,\n"
     "             frame_mv[NEARESTMV][r].as_mv.row,\n"
     "             frame_mv[NEARESTMV][r].as_mv.col, frame_mv[NEARMV][r].as_mv.row,\n"
     "             frame_mv[NEARMV][r].as_mv.col, x->mbmi_ext->mode_context[r],\n"
     "             x->mv_best_ref_index[r], x->pred_mv_sad[r]);\n"
     "  }\n"
     "  if (cpi->use_svc || cpi->oxcf.speed <= 7 || bsize < BLOCK_32X32)\n"
     "    x->sb_use_mv_part = 0;\n"),
    # Each inter candidate's cost.
    ("    mode_checked[this_mode][ref_frame] = 1;\n",
     "    VP9T(\"M r=%d m=%d mv=%d,%d f=%d tx=%d rate=%d dist=%lld rd=%lld et=%d \"\n"
     "         \"skt=%d sk=%d rmv=%d\\n\",\n"
     "         ref_frame, this_mode, frame_mv[this_mode][ref_frame].as_mv.row,\n"
     "         frame_mv[this_mode][ref_frame].as_mv.col, mi->interp_filter,\n"
     "         mi->tx_size, this_rdc.rate, (long long)this_rdc.dist,\n"
     "         (long long)this_rdc.rdcost, this_early_term, x->skip_txfm[0],\n"
     "         x->skip, rate_mv);\n"
     "    mode_checked[this_mode][ref_frame] = 1;\n"),
    # Each intra candidate's cost.
    ("      if (this_rdc.rdcost < best_rdc.rdcost) {\n"
     "        best_rdc = this_rdc;\n"
     "        best_pickmode.best_mode = this_mode;\n"
     "        best_pickmode.best_intra_tx_size = mi->tx_size;\n",
     "      VP9T(\"I m=%d tx=%d rate=%d dist=%lld rd=%lld skt=%d\\n\", this_mode,\n"
     "           mi->tx_size, this_rdc.rate, (long long)this_rdc.dist,\n"
     "           (long long)this_rdc.rdcost, x->skip_txfm[0]);\n"
     "      if (this_rdc.rdcost < best_rdc.rdcost) {\n"
     "        best_rdc = this_rdc;\n"
     "        best_pickmode.best_mode = this_mode;\n"
     "        best_pickmode.best_intra_tx_size = mi->tx_size;\n"),
    # The motion search's result.
    ("  if (scaled_ref_frame) {\n"
     "    int i;\n"
     "    for (i = 0; i < MAX_MB_PLANE; i++) xd->plane[i].pre[0] = backup_yv12[i];\n"
     "  }\n"
     "  return rv;\n",
     "  VP9T(\"N ref=%d rv=%d mv=%d,%d rmv=%d\\n\", ref, rv, tmp_mv->as_mv.row,\n"
     "       tmp_mv->as_mv.col, *rate_mv);\n"
     "  if (scaled_ref_frame) {\n"
     "    int i;\n"
     "    for (i = 0; i < MAX_MB_PLANE; i++) xd->plane[i].pre[0] = backup_yv12[i];\n"
     "  }\n"
     "  return rv;\n"),
])

ENCODER = "vp9/encoder/vp9_encoder.c"
patch(ENCODER, [
    ("#include \"vp9/encoder/vp9_temporal_filter.h\"\n",
     "#include \"vp9/encoder/vp9_temporal_filter.h\"\n"
     "#include \"vp9/encoder/vp9_trace.h\"\n"),
    ("  *size = VPXMAX(1, *size);\n"
     "\n"
     "#if 0\n"
     "  output_frame_level_debug_stats(cpi);\n",
     "  VP9T(\"E %u size=%d seg1=%d seg2=%d lca=%.17g rgf=%d alm=%d bl=%lld\\n\",\n"
     "       cm->current_video_frame, (int)*size,\n"
     "       cpi->cyclic_refresh->actual_num_seg1_blocks,\n"
     "       cpi->cyclic_refresh->actual_num_seg2_blocks,\n"
     "       cpi->cyclic_refresh->low_content_avg, cpi->refresh_golden_frame,\n"
     "       cpi->rc.avg_frame_low_motion, (long long)cpi->rc.buffer_level);\n"
     "  *size = VPXMAX(1, *size);\n"
     "\n"
     "#if 0\n"
     "  output_frame_level_debug_stats(cpi);\n"),
])
print("done")
