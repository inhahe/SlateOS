"""Write src/slice/tests/fixtures.rs: real slice headers, and what FFmpeg reads in them.

For each of a set of FFmpeg's FATE H.264 conformance streams this traces the
first frames with FFmpeg's `trace_headers` bitstream filter, picks the slices
that between them carry every kind of slice header syntax the stream has --
fields, B slices, reference list modifications, weights, memory management
operations, deblocking settings -- and writes out, in the stream's order,
every parameter set up to the last of them and the slices themselves,
escapes and all, cut a little past the header's end. With each slice goes
every syntax element FFmpeg traced for it and the bit its header ends at.
src/slice/tests.rs replays the units and checks each header against them.

    python gui/video/h264/tools/slice_fixtures.py <fate-suite>/h264-conformance

`ffmpeg` and `rustfmt` must be on PATH.
"""
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "src" / "slice" / "tests" / "fixtures.rs"

# How many frames to look through for the syntax each stream has.
FRAMES = 300
# The most slices taken from one stream.
MOST = 10
# Bytes kept past the byte a header ends in, so no read of the header comes
# near the cut.
MARGIN = 8

# (constant name, file, what it exercises)
STREAMS = [
    ("BA1_SONY", "BA1_Sony_D.jsv", "Baseline I and P slices"),
    ("BA1_FT", "BA1_FT_C.264", "several slices a picture, deblocking offsets"),
    ("BA3_SVA", "BA3_SVA_C.264", "B slices, overridden reference counts"),
    ("CABREF3", "CABREF3_Sand_D.264", "CABAC fields, cabac_init_idc 2"),
    ("CACQP3", "CACQP3_Sony_D.jsv", "deblocking within slices only"),
    ("CAMA1", "CAMA1_TOSHIBA_B.264", "MBAFF, a bottom field's POC delta"),
    ("CVWP2", "CVWP2_TOSHIBA_E.264", "explicit weights for B slices"),
    ("CAWP5", "CAWP5_TOSHIBA_E.264", "explicit weights, list modifications"),
    ("HCMP1", "HCMP1_HHI_A.264", "list 1 modifications, memory management"),
    ("MR5", "MR5_TANDBERG_C.264", "POC type 1, long-term references, resets"),
    ("MR9", "MR9_BT_B.h264", "fields with long-term references and operations"),
    ("MMCO4", "FRext/FRExt_MMCO4_Sony_B.264", "an IDR kept as a long-term reference"),
    ("SP1", "sp1_bt_a.h264", "SP slices"),
    ("PPH10", "FRext/PPH10I1_Panasonic_A.264", "10 bits: QP past 51"),
    ("HI422", "FRext/Hi422FR1_SONY_A.jsv", "4:2:2"),
    ("CANL1", "CANL1_TOSHIBA_G.264", "deblocking off"),
    ("MONO", "FRext/HPCAMOLQ_BRCM_B.264", "monochrome"),
]


def alls(f, name, values):
    return any(k == name and v in values for k, v in f)


# A feature: a predicate over one slice header's [(name, value)].
FEATURES = {
    "idr": lambda f: alls(f, "nal_unit_type", (5,)),
    "non_idr": lambda f: alls(f, "nal_unit_type", (1,)),
    "field": lambda f: alls(f, "field_pic_flag", (1,)),
    "bottom": lambda f: alls(f, "bottom_field_flag", (1,)),
    "P": lambda f: any(k == "slice_type" and v % 5 == 0 for k, v in f),
    "B": lambda f: any(k == "slice_type" and v % 5 == 1 for k, v in f),
    "SP": lambda f: any(k == "slice_type" and v % 5 == 3 for k, v in f),
    "fixed": lambda f: any(k == "slice_type" and v > 4 for k, v in f),
    "override": lambda f: alls(f, "num_ref_idx_active_override_flag", (1,)),
    "rplm_l0": lambda f: alls(f, "ref_pic_list_modification_flag_l0", (1,)),
    "rplm_l1": lambda f: alls(f, "ref_pic_list_modification_flag_l1", (1,)),
    "rplm_sub": lambda f: alls(f, "modification_of_pic_nums_idc", (0,)),
    "rplm_add": lambda f: alls(f, "modification_of_pic_nums_idc", (1,)),
    "rplm_long": lambda f: alls(f, "modification_of_pic_nums_idc", (2,)),
    "weights": lambda f: any(k == "luma_log2_weight_denom" for k, _ in f),
    "luma_weights": lambda f: any(k.startswith("luma_weight_l0_flag") and v for k, v in f),
    "chroma_weights": lambda f: any(k.startswith("chroma_weight_l0_flag") and v for k, v in f),
    "weights_l1": lambda f: any(
        k.startswith(("luma_weight_l1_flag", "chroma_weight_l1_flag")) and v for k, v in f),
    "mmco": lambda f: alls(f, "adaptive_ref_pic_marking_mode_flag", (1,)),
    "mmco_1": lambda f: alls(f, "memory_management_control_operation", (1,)),
    "mmco_2": lambda f: alls(f, "memory_management_control_operation", (2,)),
    "mmco_3": lambda f: alls(f, "memory_management_control_operation", (3,)),
    "mmco_4": lambda f: alls(f, "memory_management_control_operation", (4,)),
    "mmco_reset": lambda f: alls(f, "memory_management_control_operation", (5,)),
    "mmco_6": lambda f: alls(f, "memory_management_control_operation", (6,)),
    "idr_long": lambda f: alls(f, "long_term_reference_flag", (1,)),
    "cabac_init": lambda f: alls(f, "cabac_init_idc", (1, 2)),
    "deblock_off": lambda f: alls(f, "disable_deblocking_filter_idc", (1,)),
    "deblock_2": lambda f: alls(f, "disable_deblocking_filter_idc", (2,)),
    "deblock_offsets": lambda f: any(
        k in ("slice_alpha_c0_offset_div2", "slice_beta_offset_div2") and v != 0 for k, v in f),
    "delta_bottom": lambda f: any(k == "delta_pic_order_cnt_bottom" and v != 0 for k, v in f),
    "delta_poc1": lambda f: any(k == "delta_pic_order_cnt[1]" and v != 0 for k, v in f),
    "qp_delta": lambda f: any(k == "slice_qp_delta" and v != 0 for k, v in f),
    "later_slice": lambda f: any(k == "first_mb_in_slice" and v != 0 for k, v in f),
}


def units(data: bytes):
    """Annex B units as they are in the file, start codes and trailing zero
    bytes off, escapes kept."""
    starts = []
    i = 0
    while (j := data.find(b"\x00\x00\x01", i)) >= 0:
        starts.append(j + 3)
        i = j + 3
    for k, s in enumerate(starts):
        e = starts[k + 1] - 3 if k + 1 < len(starts) else len(data)
        unit = data[s:e]
        while unit.endswith(b"\x00"):
            unit = unit[:-1]
        if unit:
            yield unit


def traced_slices(path: Path) -> list:
    """Each traced slice header: ([(name, value)], the bit it ends at)."""
    out = subprocess.run(
        ["ffmpeg", "-hide_banner", "-i", str(path), "-c", "copy", "-bsf:v", "trace_headers",
         "-frames:v", str(FRAMES), "-f", "null", "-"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    ).stderr
    result, cur = [], None
    for line in out.splitlines():
        if "] Slice Header" in line:
            cur = {"fields": [], "end": 0}
            result.append(cur)
            continue
        if cur is None:
            continue
        parts = line.split()
        if len(parts) >= 7 and parts[-2] == "=" and parts[3].isdigit():
            name, value = parts[4], int(parts[-1])
            if name == "cabac_alignment_one_bit":
                continue
            cur["fields"].append((name, value))
            cur["end"] = int(parts[3]) + len(parts[-3])
        elif "] " in line:
            cur = None
    return result


def escaped_prefix(unit: bytes, rbsp_bytes: int) -> bytes:
    """The shortest prefix of `unit` that unescapes to `rbsp_bytes` bytes, or
    all of it."""
    zeros = 0
    got = 0
    for i, b in enumerate(unit):
        if zeros >= 2 and b == 3:
            zeros = 0
            continue
        got += 1
        zeros = zeros + 1 if b == 0 else 0
        if got == rbsp_bytes:
            return unit[: i + 1]
    return unit


def choose(slices: list) -> list:
    """The indices of the slices to keep: the first, then the first with each
    feature not yet covered, at most MOST."""
    chosen = [0]
    covered = {n for n, p in FEATURES.items() if p(slices[0]["fields"])}
    for name, pred in FEATURES.items():
        if name in covered or len(chosen) >= MOST:
            continue
        for k, s in enumerate(slices):
            if pred(s["fields"]):
                chosen.append(k)
                covered |= {n for n, p in FEATURES.items() if p(s["fields"])}
                break
    return sorted(set(chosen))


def rust_bytes(b: bytes) -> str:
    return "[" + ", ".join(f"0x{x:02X}" for x in b) + "]"


def rust_fields(fields) -> str:
    return "[" + ", ".join(f'("{n}", {v})' for n, v in fields) + "]"


def stream(path: Path) -> list:
    """The units to replay: parameter sets and the chosen slices, in order."""
    data = path.read_bytes()
    slices = traced_slices(path)
    if not slices:
        raise SystemExit(f"{path}: no slices traced")
    keep = set(choose(slices))
    last = max(keep)
    out = []
    n = 0
    for u in units(data):
        kind = u[0] & 0x1F
        if kind in (7, 8):
            out.append((u, None))
        elif kind in (1, 5):
            if n in keep:
                s = slices[n]
                cut = escaped_prefix(u, (s["end"] + 7) // 8 + MARGIN)
                out.append((cut, s))
            n += 1
            if n > last:
                break
    return out


def main() -> None:
    root = Path(sys.argv[1])
    body = [
        "// Generated by tools/slice_fixtures.py from FFmpeg's FATE H.264 conformance",
        "// streams, and FFmpeg's trace_headers reading of them. Do not edit.",
        "",
        "/// A unit to replay: a parameter set, or a slice cut a little past its",
        "/// header with what FFmpeg traced of the header and the bit it ends at.",
        "pub(super) struct Unit {",
        "    pub(super) bytes: &'static [u8],",
        "    pub(super) trace: &'static [(&'static str, i64)],",
        "    pub(super) end: u32,",
        "}",
        "",
        "/// One stream's units.",
        "pub(super) struct Fixture {",
        "    pub(super) name: &'static str,",
        "    pub(super) units: &'static [Unit],",
        "}",
        "",
    ]
    names = []
    for const, file, what in STREAMS:
        path = root / file
        body += [
            f"/// {file}: {what}.",
            f"pub(super) const {const}: Fixture = Fixture {{",
            f'    name: "{file}",',
            "    units: &[",
        ]
        for u, s in stream(path):
            trace = rust_fields(s["fields"]) if s else "[]"
            end = s["end"] if s else 0
            body.append(f"        Unit {{ bytes: &{rust_bytes(u)}, trace: &{trace}, end: {end} }},")
        body += ["    ],", "};", ""]
        names.append(const)
    body += [
        "/// Every fixture.",
        f"pub(super) const ALL: [Fixture; {len(names)}] = [{', '.join(names)}];",
        "",
    ]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(body), encoding="utf-8", newline="\n")
    subprocess.run(["rustfmt", "--edition", "2024", str(OUT)], check=True)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
