"""Turn libopus's static tables into Rust.

Reads the C sources and writes `src/celt/tables.rs`: the 48 kHz mode's
tables from `celt/static_modes_fixed.h` and the band layout from
`celt/modes.c`, each array as its C declaration states its type and length.
"""

import re
import sys
from pathlib import Path

OPUS = Path(sys.argv[1])
OUT = Path(sys.argv[2])

C_TO_RUST = {
    "opus_val16": "i16",
    "opus_int16": "i16",
    "unsigned char": "u8",
    "opus_int32": "i32",
    "opus_uint32": "u32",
    "opus_uint16": "u16",
    "opus_int8": "i8",
    "signed char": "i8",
}


def arrays(text):
    """Every `static const TYPE NAME[N] = { ... };` in `text`."""
    pat = re.compile(r"static\s+const\s+([A-Za-z_0-9 ]+?)\s+(\w+)\s*\[\s*(\w*)\s*\]\s*=\s*\{(.*?)\};", re.S)
    for m in pat.finditer(text):
        ctype, name, length, body = m.group(1).strip(), m.group(2), m.group(3), m.group(4)
        body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
        body = re.sub(r"//[^\n]*", "", body)
        yield ctype, name, length, body


def numbers(body):
    return [int(v, 0) for v in re.findall(r"-?(?:0x[0-9A-Fa-f]+|\d+)", body)]


def emit(out, ctype, name, body, doc):
    if ctype == "kiss_twiddle_cpx":
        vals = numbers(body)
        pairs = [f"({vals[i]}, {vals[i + 1]})" for i in range(0, len(vals), 2)]
        out.append(f"/// {doc}")
        out.append(f"pub(crate) static {name.upper()}: [(i16, i16); {len(pairs)}] = [")
        for i in range(0, len(pairs), 6):
            out.append("    " + ", ".join(pairs[i:i + 6]) + ",")
        out.append("];")
        out.append("")
        return
    rtype = C_TO_RUST[ctype]
    vals = numbers(body)
    out.append(f"/// {doc}")
    out.append(f"pub(crate) static {name.upper()}: [{rtype}; {len(vals)}] = [")
    for i in range(0, len(vals), 12):
        out.append("    " + ", ".join(str(v) for v in vals[i:i + 12]) + ",")
    out.append("];")
    out.append("")


DOCS = {
    "window120": "The overlap window, 120 samples (`window120`).",
    "logN400": "Each band's log2 of its width, in eighths of a bit (`logN400`).",
    "cache_index50": "Where each band's and size's pulse cache begins (`cache_index50`).",
    "cache_bits50": "The pulse caches: bits for each pulse count (`cache_bits50`).",
    "cache_caps50": "The most bits each band can take, by size and channels (`cache_caps50`).",
    "fft_twiddles48000_960": "The FFT's twiddle factors, Q15 (`fft_twiddles48000_960`).",
    "fft_bitrev480": "The 480-point FFT's output order (`fft_bitrev480`).",
    "fft_bitrev240": "The 240-point FFT's output order (`fft_bitrev240`).",
    "fft_bitrev120": "The 120-point FFT's output order (`fft_bitrev120`).",
    "fft_bitrev60": "The 60-point FFT's output order (`fft_bitrev60`).",
    "mdct_twiddles960": "The MDCT's twiddle factors, Q15 (`mdct_twiddles960`).",
    "eband5ms": "The band edges, in units of 200 Hz at a 5 ms frame (`eband5ms`).",
    "band_allocation": "The allocation table, in 1/32 bit a sample, by quality and band (`band_allocation`).",
}


def main():
    out = [
        "//! The 48 kHz mode's tables: libopus 1.5.2's `celt/static_modes_fixed.h`",
        "//! (the fixed-point build's) and the band layout from `celt/modes.c`,",
        "//! copyright Xiph.Org and the contributors named in its `COPYING`, used",
        "//! under libopus's BSD licence (`licenses/libopus-COPYING`). Written by",
        "//! `tools/convert_tables.py`, which reads them from libopus's sources.",
        "",
    ]
    wanted = list(DOCS)
    found = {}
    for path in ("celt/static_modes_fixed.h", "celt/modes.c"):
        text = (OPUS / path).read_text(encoding="utf-8")
        for ctype, name, length, body in arrays(text):
            if name in DOCS and name not in found:
                found[name] = (ctype, body)
    for name in wanted:
        if name not in found:
            raise SystemExit(f"{name} not found")
        ctype, body = found[name]
        emit(out, ctype, name, body, DOCS[name])
    OUT.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print(f"wrote {len(found)} tables")


main()
