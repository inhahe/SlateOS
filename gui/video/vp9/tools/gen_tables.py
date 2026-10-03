"""Write gui/video/vp9/src/tables.rs from libvpx's own source.

The VP9 decoder needs a few thousand numbers that are libvpx's and nobody
else's -- default probabilities, scan orders, quantiser steps, filter
kernels. Typing them would be the one part of the port nobody could check, so
this reads them out of a libvpx checkout and writes them as Rust:

    git clone --depth 1 --branch v1.17.0 https://github.com/webmproject/libvpx
    python tools/gen_tables.py path/to/libvpx > src/tables.rs

Each array is read by its C name, filled by C's own initializer rules (nested
braces, brace elision, missing trailing elements zero), and checked against
the shape the Rust side declares: a table that does not fit is an error that
names it, never a truncation or a guess.
"""

import re
import subprocess
import sys

#: C names the initializers use, by value.
SYMBOLS = {
    **{name: i for i, name in enumerate([
        "BLOCK_4X4", "BLOCK_4X8", "BLOCK_8X4", "BLOCK_8X8", "BLOCK_8X16",
        "BLOCK_16X8", "BLOCK_16X16", "BLOCK_16X32", "BLOCK_32X16",
        "BLOCK_32X32", "BLOCK_32X64", "BLOCK_64X32", "BLOCK_64X64",
        "BLOCK_INVALID"])},
    "TX_4X4": 0, "TX_8X8": 1, "TX_16X16": 2, "TX_32X32": 3,
    "PARTITION_NONE": 0, "PARTITION_HORZ": 1, "PARTITION_VERT": 2,
    "PARTITION_SPLIT": 3, "PARTITION_INVALID": 4,
    "BOTH_ZERO": 0, "ZERO_PLUS_PREDICTED": 1, "BOTH_PREDICTED": 2, "NEW_PLUS_NON_INTRA": 3,
    "BOTH_NEW": 4, "INTRA_PLUS_NON_INTRA": 5, "BOTH_INTRA": 6, "INVALID_CASE": 9,
}

#: (C file, C name, Rust name, element type, shape, what it is).
TABLES = [
    ("vp9/common/vp9_common_data.c", "b_width_log2_lookup", "B_WIDTH_LOG2", "u8", [13],
     "log2 of a block size's width in 4x4 units"),
    ("vp9/common/vp9_common_data.c", "b_height_log2_lookup", "B_HEIGHT_LOG2", "u8", [13],
     "log2 of a block size's height in 4x4 units"),
    ("vp9/common/vp9_common_data.c", "num_4x4_blocks_wide_lookup", "NUM_4X4_WIDE", "u8", [13],
     "a block size's width in 4x4 units"),
    ("vp9/common/vp9_common_data.c", "num_4x4_blocks_high_lookup", "NUM_4X4_HIGH", "u8", [13],
     "a block size's height in 4x4 units"),
    ("vp9/common/vp9_common_data.c", "mi_width_log2_lookup", "MI_WIDTH_LOG2", "u8", [13],
     "log2 of a block size's width in 8x8 mode-info units"),
    ("vp9/common/vp9_common_data.c", "num_8x8_blocks_wide_lookup", "NUM_8X8_WIDE", "u8", [13],
     "a block size's width in 8x8 units, at least one"),
    ("vp9/common/vp9_common_data.c", "num_8x8_blocks_high_lookup", "NUM_8X8_HIGH", "u8", [13],
     "a block size's height in 8x8 units, at least one"),
    ("vp9/common/vp9_common_data.c", "size_group_lookup", "SIZE_GROUP", "u8", [13],
     "which of the four y-mode probability sets a block size uses"),
    ("vp9/common/vp9_common_data.c", "num_pels_log2_lookup", "NUM_PELS_LOG2", "u8", [13],
     "log2 of a block size's area in pixels"),
    ("vp9/common/vp9_common_data.c", "subsize_lookup", "SUBSIZE", "u8", [4, 13],
     "the block size a partition type splits a block size into (13 = invalid)"),
    ("vp9/common/vp9_common_data.c", "max_txsize_lookup", "MAX_TXSIZE", "u8", [13],
     "the largest transform a block size can use"),
    ("vp9/common/vp9_common_data.c", "txsize_to_bsize", "TXSIZE_TO_BSIZE", "u8", [4],
     "the square block size of a transform size"),
    ("vp9/common/vp9_common_data.c", "tx_mode_to_biggest_tx_size", "TX_MODE_TO_BIGGEST_TX_SIZE",
     "u8", [5], "the largest transform a frame's transform mode allows"),
    ("vp9/common/vp9_common_data.c", "ss_size_lookup", "SS_SIZE", "u8", [13, 2, 2],
     "a block size subsampled by [x][y] (13 = invalid)"),
    ("vp9/common/vp9_common_data.c", "uv_txsize_lookup", "UV_TXSIZE", "u8", [13, 4, 2, 2],
     "a chroma plane's transform size, by block size, luma transform size and [x][y] subsampling"),
    ("vp9/common/vp9_common_data.c", "partition_context_lookup", "PARTITION_CONTEXT_LOOKUP",
     "u8", [13, 2], "the partition context a block size leaves: [above, left]"),

    ("vp9/common/vp9_entropy.c", "vp9_coefband_trans_8x8plus", "COEFBAND_TRANS_8X8PLUS", "u8",
     [1024], "the coefficient band of each scan position, 8x8 transforms and larger"),
    ("vp9/common/vp9_entropy.c", "vp9_coefband_trans_4x4", "COEFBAND_TRANS_4X4", "u8", [16],
     "the coefficient band of each scan position of a 4x4 transform"),
    ("vp9/common/vp9_entropy.c", "vp9_pt_energy_class", "PT_ENERGY_CLASS", "u8", [12],
     "a token's energy class: what it contributes to its neighbours' contexts"),
    ("vp9/common/vp9_entropy.c", "vp9_cat6_prob_high12", "CAT6_PROB_HIGH12", "u8", [18],
     "the probabilities of category 6's extra bits at 12 bits; 10 and 8 bits use its tail"),
    ("vp9/common/vp9_entropy.c", "vp9_pareto8_full", "PARETO8_FULL", "u8", [255, 8],
     "the model's probabilities for the nodes after the pivot, by the pivot's probability"),
    ("vp9/common/vp9_entropy.c", "default_coef_probs_4x4", "DEFAULT_COEF_PROBS_4X4", "u8",
     [2, 2, 6, 6, 3], "the default coefficient probabilities, 4x4 transforms"),
    ("vp9/common/vp9_entropy.c", "default_coef_probs_8x8", "DEFAULT_COEF_PROBS_8X8", "u8",
     [2, 2, 6, 6, 3], "the default coefficient probabilities, 8x8 transforms"),
    ("vp9/common/vp9_entropy.c", "default_coef_probs_16x16", "DEFAULT_COEF_PROBS_16X16", "u8",
     [2, 2, 6, 6, 3], "the default coefficient probabilities, 16x16 transforms"),
    ("vp9/common/vp9_entropy.c", "default_coef_probs_32x32", "DEFAULT_COEF_PROBS_32X32", "u8",
     [2, 2, 6, 6, 3], "the default coefficient probabilities, 32x32 transforms"),

    ("vp9/common/vp9_entropymode.c", "vp9_kf_y_mode_prob", "KF_Y_MODE_PROB", "u8", [10, 10, 9],
     "key-frame luma mode probabilities, by the above and left blocks' modes"),
    ("vp9/common/vp9_entropymode.c", "vp9_kf_uv_mode_prob", "KF_UV_MODE_PROB", "u8", [10, 9],
     "key-frame chroma mode probabilities, by the luma mode"),
    ("vp9/common/vp9_entropymode.c", "default_if_y_probs", "DEFAULT_IF_Y_PROBS", "u8", [4, 9],
     "inter-frame luma intra mode probabilities, by size group"),
    ("vp9/common/vp9_entropymode.c", "default_if_uv_probs", "DEFAULT_IF_UV_PROBS", "u8", [10, 9],
     "inter-frame chroma mode probabilities, by the luma mode"),
    ("vp9/common/vp9_entropymode.c", "vp9_kf_partition_probs", "KF_PARTITION_PROBS", "u8",
     [16, 3], "key-frame partition probabilities, by partition context"),
    ("vp9/common/vp9_entropymode.c", "default_partition_probs", "DEFAULT_PARTITION_PROBS", "u8",
     [16, 3], "inter-frame partition probabilities, by partition context"),
    ("vp9/common/vp9_entropymode.c", "default_inter_mode_probs", "DEFAULT_INTER_MODE_PROBS",
     "u8", [7, 3], "inter mode probabilities, by mode context"),
    ("vp9/common/vp9_entropymode.c", "default_intra_inter_p", "DEFAULT_INTRA_INTER_P", "u8", [4],
     "the probabilities that a block is intra, by context"),
    ("vp9/common/vp9_entropymode.c", "default_comp_inter_p", "DEFAULT_COMP_INTER_P", "u8", [5],
     "the probabilities that a block has one reference, by context"),
    ("vp9/common/vp9_entropymode.c", "default_comp_ref_p", "DEFAULT_COMP_REF_P", "u8", [5],
     "compound reference probabilities, by context"),
    ("vp9/common/vp9_entropymode.c", "default_single_ref_p", "DEFAULT_SINGLE_REF_P", "u8",
     [5, 2], "single reference probabilities, by context"),
    ("vp9/common/vp9_entropymode.c", "default_skip_probs", "DEFAULT_SKIP_PROBS", "u8", [3],
     "the probabilities that a block has no coefficients, by context"),
    ("vp9/common/vp9_entropymode.c", "default_switchable_interp_prob",
     "DEFAULT_SWITCHABLE_INTERP_PROB", "u8", [4, 2],
     "interpolation filter probabilities, by context"),

    ("vp9/common/vp9_entropymv.c", "log_in_base_2", "LOG_IN_BASE_2", "u8", [1025],
     "floor(log2(n)) for the motion vector class of n"),

    ("vp9/common/vp9_quant_common.c", "dc_qlookup", "DC_QLOOKUP", "i16", [256],
     "the DC quantiser step for each index, 8 bits"),
    ("vp9/common/vp9_quant_common.c", "dc_qlookup_10", "DC_QLOOKUP_10", "i16", [256],
     "the DC quantiser step for each index, 10 bits"),
    ("vp9/common/vp9_quant_common.c", "dc_qlookup_12", "DC_QLOOKUP_12", "i16", [256],
     "the DC quantiser step for each index, 12 bits"),
    ("vp9/common/vp9_quant_common.c", "ac_qlookup", "AC_QLOOKUP", "i16", [256],
     "the AC quantiser step for each index, 8 bits"),
    ("vp9/common/vp9_quant_common.c", "ac_qlookup_10", "AC_QLOOKUP_10", "i16", [256],
     "the AC quantiser step for each index, 10 bits"),
    ("vp9/common/vp9_quant_common.c", "ac_qlookup_12", "AC_QLOOKUP_12", "i16", [256],
     "the AC quantiser step for each index, 12 bits"),

    ("vp9/common/vp9_filter.c", "bilinear_filters", "BILINEAR_FILTERS", "i16", [16, 8],
     "the bilinear interpolation kernels, by sixteenth-pixel position"),
    ("vp9/common/vp9_filter.c", "sub_pel_filters_8", "SUB_PEL_FILTERS_8", "i16", [16, 8],
     "the regular 8-tap interpolation kernels"),
    ("vp9/common/vp9_filter.c", "sub_pel_filters_8lp", "SUB_PEL_FILTERS_8LP", "i16", [16, 8],
     "the smooth 8-tap interpolation kernels"),
    ("vp9/common/vp9_filter.c", "sub_pel_filters_8s", "SUB_PEL_FILTERS_8S", "i16", [16, 8],
     "the sharp 8-tap interpolation kernels"),

    ("vp9/decoder/vp9_dsubexp.c", "inv_map_table", "INV_MAP_TABLE", "u8", [255],
     "how a probability update's coded index maps to a distance from the old probability"),
    ("vp9/common/vp9_mvref_common.h", "mv_ref_blocks", "MV_REF_BLOCKS", "i8", [13, 8, 2],
     "the neighbours searched for candidate motion vectors, as [row, col] offsets in 8x8 units, by block size"),
    ("vp9/common/vp9_mvref_common.h", "mode_2_counter", "MODE_2_COUNTER", "u8", [14],
     "what a neighbour's mode adds to the inter-mode context count"),
    ("vp9/common/vp9_mvref_common.h", "counter_to_context", "COUNTER_TO_CONTEXT", "u8", [19],
     "the inter-mode context for a neighbour count (9 marks a count no stream can produce)"),
    ("vp9/common/vp9_mvref_common.h", "idx_n_column_to_subblock", "IDX_N_COLUMN_TO_SUBBLOCK",
     "u8", [4, 2], "which sub-block of a neighbour a sub-8x8 block takes its candidate from"),
    ("vp9/common/vp9_scan.c", "default_scan_4x4", "DEFAULT_SCAN_4X4", "i16", [16], ""),
    ("vp9/common/vp9_scan.c", "col_scan_4x4", "COL_SCAN_4X4", "i16", [16], ""),
    ("vp9/common/vp9_scan.c", "row_scan_4x4", "ROW_SCAN_4X4", "i16", [16], ""),
    ("vp9/common/vp9_scan.c", "default_scan_8x8", "DEFAULT_SCAN_8X8", "i16", [64], ""),
    ("vp9/common/vp9_scan.c", "col_scan_8x8", "COL_SCAN_8X8", "i16", [64], ""),
    ("vp9/common/vp9_scan.c", "row_scan_8x8", "ROW_SCAN_8X8", "i16", [64], ""),
    ("vp9/common/vp9_scan.c", "default_scan_16x16", "DEFAULT_SCAN_16X16", "i16", [256], ""),
    ("vp9/common/vp9_scan.c", "col_scan_16x16", "COL_SCAN_16X16", "i16", [256], ""),
    ("vp9/common/vp9_scan.c", "row_scan_16x16", "ROW_SCAN_16X16", "i16", [256], ""),
    ("vp9/common/vp9_scan.c", "default_scan_32x32", "DEFAULT_SCAN_32X32", "i16", [1024], ""),
    ("vp9/common/vp9_scan.c", "default_scan_4x4_neighbors", "DEFAULT_SCAN_4X4_NEIGHBORS",
     "i16", [34], ""),
    ("vp9/common/vp9_scan.c", "col_scan_4x4_neighbors", "COL_SCAN_4X4_NEIGHBORS", "i16", [34],
     ""),
    ("vp9/common/vp9_scan.c", "row_scan_4x4_neighbors", "ROW_SCAN_4X4_NEIGHBORS", "i16", [34],
     ""),
    ("vp9/common/vp9_scan.c", "default_scan_8x8_neighbors", "DEFAULT_SCAN_8X8_NEIGHBORS",
     "i16", [130], ""),
    ("vp9/common/vp9_scan.c", "col_scan_8x8_neighbors", "COL_SCAN_8X8_NEIGHBORS", "i16", [130],
     ""),
    ("vp9/common/vp9_scan.c", "row_scan_8x8_neighbors", "ROW_SCAN_8X8_NEIGHBORS", "i16", [130],
     ""),
    ("vp9/common/vp9_scan.c", "default_scan_16x16_neighbors", "DEFAULT_SCAN_16X16_NEIGHBORS",
     "i16", [514], ""),
    ("vp9/common/vp9_scan.c", "col_scan_16x16_neighbors", "COL_SCAN_16X16_NEIGHBORS", "i16",
     [514], ""),
    ("vp9/common/vp9_scan.c", "row_scan_16x16_neighbors", "ROW_SCAN_16X16_NEIGHBORS", "i16",
     [514], ""),
    ("vp9/common/vp9_scan.c", "default_scan_32x32_neighbors", "DEFAULT_SCAN_32X32_NEIGHBORS",
     "i16", [2050], ""),
]


def initializer(source: str, name: str) -> str:
    """The text between the braces of `name`'s initializer."""
    m = re.search(r"\b" + re.escape(name) + r"\s*\[[^=;]*=\s*\{", source)
    if not m:
        raise SystemExit(f"{name}: not found")
    depth = 0
    start = m.end() - 1
    for i in range(start, len(source)):
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
            if depth == 0:
                return source[start:i + 1]
    raise SystemExit(f"{name}: unbalanced braces")


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    return re.sub(r"//[^\n]*", " ", text)


TOKEN = re.compile(r"\s*(\{|\}|,|-?\s*0[xX][0-9a-fA-F]+[uUlL]*|-?\s*\d+[uUlL]*|-?\s*[A-Za-z_]\w*)")


def parse(text: str):
    """The initializer as nested lists of ints."""
    pos = 0
    tokens = []
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m:
            if text[pos:].strip() == "":
                break
            raise SystemExit(f"cannot read {text[pos:pos + 40]!r}")
        tokens.append(m.group(1).replace(" ", ""))
        pos = m.end()

    def value(tok: str) -> int:
        neg = tok.startswith("-")
        body = tok.lstrip("-").rstrip("uUlL")
        if body.lower().startswith("0x"):
            v = int(body, 16)
        elif body[0].isdigit():
            v = int(body)
        elif body in SYMBOLS:
            v = SYMBOLS[body]
        else:
            raise SystemExit(f"unknown symbol {body}")
        return -v if neg else v

    def group(i: int):
        assert tokens[i] == "{"
        items = []
        i += 1
        while tokens[i] != "}":
            if tokens[i] == "{":
                sub, i = group(i)
                items.append(sub)
            elif tokens[i] == ",":
                i += 1
                continue
            else:
                items.append(value(tokens[i]))
                i += 1
        return items, i + 1

    tree, _ = group(0)
    return tree


def fill(items, shape, name):
    """C's initializer rules: nested braces, brace elision, zero padding."""

    def take(cursor, shape):
        # `cursor` is [list, index]: the items at one brace level.
        lst, _ = cursor
        if not shape:
            if cursor[1] >= len(lst):
                return 0
            item = lst[cursor[1]]
            cursor[1] += 1
            while isinstance(item, list):  # a scalar in braces
                item = item[0] if item else 0
            return item
        out = []
        for _ in range(shape[0]):
            if cursor[1] < len(lst) and isinstance(lst[cursor[1]], list) and len(shape) > 1:
                sub = [lst[cursor[1]], 0]
                cursor[1] += 1
                out.append(take_all(sub, shape[1:]))
            elif cursor[1] < len(lst) and isinstance(lst[cursor[1]], list):
                sub = lst[cursor[1]]
                cursor[1] += 1
                out.append(sub[0] if sub else 0)
            else:
                out.append(take(cursor, shape[1:]))
        return out

    def take_all(cursor, shape):
        result = take(cursor, shape)
        if cursor[1] < len(cursor[0]):
            raise SystemExit(f"{name}: more initializers than {shape} holds")
        return result

    return take_all([items, 0], shape)


def rust_array(values, shape, ty, indent=0) -> str:
    if len(shape) == 1:
        nums = [str(v) for v in values]
        lines = []
        line = ""
        for n in nums:
            piece = n + ", "
            if len(line) + len(piece) > 92 - indent:
                lines.append(line.rstrip())
                line = ""
            line += piece
        if line:
            lines.append(line.rstrip())
        pad = " " * (indent + 4)
        return "[\n" + "\n".join(pad + l for l in lines) + "\n" + " " * indent + "]"
    inner = ",\n".join(" " * (indent + 4) + rust_array(v, shape[1:], ty, indent + 4)
                       for v in values)
    return "[\n" + inner + ",\n" + " " * indent + "]"


def rust_type(ty, shape):
    t = ty
    for n in reversed(shape):
        t = f"[{t}; {n}]"
    return t


def check_range(name, values, ty):
    flat = values
    while flat and isinstance(flat[0], list):
        flat = [x for sub in flat for x in sub]
    lo, hi = {"u8": (0, 255), "i8": (-128, 127), "i16": (-32768, 32767), "u16": (0, 65535),
              "u64": (0, 2**64 - 1)}[ty]
    for v in flat:
        if not lo <= v <= hi:
            raise SystemExit(f"{name}: {v} does not fit {ty}")


def main() -> int:
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    sources = {}
    out = [
        "//! libvpx's constant tables, written by `tools/gen_tables.py` from libvpx",
        "//! v1.17.0's source (copyright the WebM project authors): do not edit;",
        "//! regenerate. Used under libvpx's BSD licence and patent grant",
        "//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).",
        "//!",
        "//! Each table is read by its C name, filled by C's initializer rules and",
        "//! checked against the shape declared here, so a table that does not fit",
        "//! fails the generator instead of being cut short.",
        "",
    ]
    for path, cname, rname, ty, shape, doc in TABLES:
        if path not in sources:
            with open(f"{root}/{path}", encoding="utf-8") as f:
                sources[path] = strip_comments(f.read())
        items = parse(initializer(sources[path], cname))
        values = fill(items, shape, cname)
        check_range(cname, values, ty)
        if doc:
            out.append(f"/// {doc[0].upper() + doc[1:]}: libvpx's `{cname}`.")
        else:
            out.append(f"/// libvpx's `{cname}`.")
        out.append(f"pub const {rname}: {rust_type(ty, shape)} = {rust_array(values, shape, ty)};")
        out.append("")
    # Through rustfmt, so the file is formatted the way every Rust file in the
    # tree is (the pre-push gate formats each file on its own, and allows no
    # exceptions), and so that regenerating and diffing still compares like
    # with like. Bytes, not text: on Windows a text-mode stdout turns every
    # newline into CRLF.
    formatted = subprocess.run(
        ["rustfmt", "--edition", "2024", "--emit", "stdout"],
        input="\n".join(out).encode("utf-8"),
        capture_output=True,
        check=True,
    ).stdout
    sys.stdout.buffer.write(formatted.replace(b"\r\n", b"\n"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
