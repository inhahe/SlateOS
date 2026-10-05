"""Turn libopus's SILK decoder tables into Rust.

Reads the `silk/tables_*.c`, `silk/pitch_est_tables.c`,
`silk/resampler_rom.c` and `silk/table_LSF_cos.c` of a libopus source tree
and writes every integer array they define (1 to 3 dimensions) to
`src/silk/tables.rs`, named without libopus's `silk_` prefix and in upper
case. The pointer tables and codebook structs built from them are written by
hand in `src/silk/codebook.rs`.

    python tools/convert_silk_tables.py <libopus source> src/silk/tables.rs
"""

import re
import sys
from pathlib import Path

FILES = [
    "tables_LTP.c",
    "tables_NLSF_CB_NB_MB.c",
    "tables_NLSF_CB_WB.c",
    "tables_gain.c",
    "tables_other.c",
    "tables_pitch_lag.c",
    "tables_pulses_per_block.c",
    "pitch_est_tables.c",
    "resampler_rom.c",
    "table_LSF_cos.c",
]
TYPES = {
    "opus_uint8": "u8",
    "opus_int8": "i8",
    "opus_int16": "i16",
    "opus_uint16": "u16",
    "opus_int32": "i32",
    "opus_uint32": "u32",
}
RANGES = {
    "u8": (0, 255),
    "i8": (-128, 127),
    "i16": (-32768, 32767),
    "u16": (0, 65535),
    "i32": (-(2**31), 2**31 - 1),
    "u32": (0, 2**32 - 1),
}
DECL = re.compile(
    r"(?:silk_DWORD_ALIGN\s+)?(?:static\s+)?const\s+(opus_\w+)\s+silk_(\w+)\s*((?:\[[^\]]*\]\s*)+)=\s*\{",
    re.S,
)
IDENT = re.compile(r"\b[A-Za-z_]\w*\b")
ARITHMETIC = re.compile(r"[0-9\s+\-*/()<>]+")

# The integer `#define`s of `silk/define.h`, which some tables use.
DEFINES = {}


def parse_braced(text, start):
    """The nested lists of the brace that opens at `start`, and where it ends."""
    assert text[start] == "{"
    stack = [[]]
    item = ""
    i = start + 1
    while True:
        c = text[i]
        if c == "{":
            stack.append([])
            item = ""
        elif c in ",}":
            if item.strip():
                stack[-1].append(evaluate(item))
            item = ""
            if c == "}":
                done = stack.pop()
                if not stack:
                    return done, i
                stack[-1].append(done)
        else:
            item += c
        i += 1


def load_defines(root):
    """The integer `#define`s of the headers the tables' sizes and values
    use, resolved until no more resolve (a define may use a later one)."""
    pending = {}
    for header in ("define.h", "pitch_est_defines.h", "resampler_rom.h", "SigProc_FIX.h"):
        text = (root / header).read_text(encoding="utf-8")
        text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
        for m in re.finditer(r"^#define\s+(\w+)\s+(.+)$", text, flags=re.M):
            pending[m.group(1)] = m.group(2).strip()
    progress = True
    while progress:
        progress = False
        for name, value in list(pending.items()):
            try:
                DEFINES[name] = evaluate(value)
            except (ValueError, SyntaxError):
                continue
            del pending[name]
            progress = True


def evaluate(expr):
    """An integer expression of digits, operators and known `#define`s."""
    expr = IDENT.sub(lambda m: str(DEFINES.get(m.group(0), m.group(0))), expr.strip())
    if not ARITHMETIC.fullmatch(expr):
        raise ValueError(f"cannot evaluate {expr!r}")
    # Digits and operators only, checked above; C's integer division.
    return int(eval(expr.replace("/", "//")))


def shape(values):
    dims = []
    v = values
    while isinstance(v, list):
        dims.append(len(v))
        v = v[0] if v else None
    return dims


def flatten(values):
    if isinstance(values, list):
        for v in values:
            yield from flatten(v)
    else:
        yield values


def rust_type(base, dims):
    t = base
    for d in reversed(dims):
        t = f"[{t}; {d}]"
    return t


def render(values, indent):
    pad = " " * indent
    if values and not isinstance(values[0], list):
        lines = []
        for i in range(0, len(values), 12):
            lines.append(pad + ", ".join(str(v) for v in values[i : i + 12]) + ",")
        return "\n".join(lines)
    out = []
    for row in values:
        out.append(pad + "[")
        out.append(render(row, indent + 4))
        out.append(pad + "],")
    return "\n".join(out)


def main():
    root = Path(sys.argv[1]) / "silk"
    load_defines(root)
    out = [
        "//! SILK's decoder tables: libopus 1.5.2's `silk/tables_*.c`,",
        "//! `silk/pitch_est_tables.c`, `silk/resampler_rom.c` and",
        "//! `silk/table_LSF_cos.c`, copyright Skype Limited, Xiph.Org and the",
        "//! contributors named in its `COPYING`, used under libopus's BSD licence",
        "//! (`licenses/libopus-COPYING`). Written by `tools/convert_silk_tables.py`;",
        "//! each keeps libopus's name, without its `silk_` prefix.",
        "",
        "#![allow(dead_code, reason = \"every table libopus's files define, some only its encoder's\")]",
        "",
    ]
    count = 0
    for name in FILES:
        text = (root / name).read_text(encoding="utf-8")
        text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
        text = re.sub(r"//[^\n]*", "", text)
        for m in DECL.finditer(text):
            ctype, cname, declared = m.groups()
            base = TYPES[ctype]
            values, _end = parse_braced(text, m.end() - 1)
            dims = shape(values)
            # C zero-fills an initialiser shorter than its declaration; none
            # of these is, and this keeps it so.
            declared_dims = [evaluate(d) for d in re.findall(r"\[([^\]]*)\]", declared)]
            if declared_dims != dims:
                raise SystemExit(f"{cname}: declared {declared_dims}, initialised {dims}")
            lo, hi = RANGES[base]
            for v in flatten(values):
                if not lo <= v <= hi:
                    raise SystemExit(f"{cname}: {v} out of {base}")
            out.append(f"/// `silk_{cname}` ({name}).")
            out.append(f"pub(crate) static {cname.upper()}: {rust_type(base, dims)} = [")
            out.append(render(values, 4))
            out.append("];")
            out.append("")
            count += 1
    Path(sys.argv[2]).write_text("\n".join(out), encoding="utf-8", newline="\n")
    print("wrote", count, "tables")


main()
