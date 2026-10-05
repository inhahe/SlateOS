"""Turn libopus's PVQ codeword-count table into Rust.

Reads `celt/cwrs.c`, keeps the non-custom-mode build's `CELT_PVQ_U_DATA`
(the `#if defined(CUSTOM_MODES)` parts dropped, their `#else` parts kept) and
writes `src/celt/pvq_table.rs`.
"""

import re
import sys
from pathlib import Path

src = (Path(sys.argv[1]) / "celt" / "cwrs.c").read_text(encoding="utf-8")
start = src.index("static const opus_uint32 CELT_PVQ_U_DATA[1272]={")
body_start = src.index("{", start) + 1
end = src.index("};", body_start)
body = src[body_start:end]

kept = []
state = []  # stack of "keep" flags
for line in body.splitlines():
    s = line.strip()
    if s.startswith("#if"):
        state.append(not ("CUSTOM_MODES" in s))
        continue
    if s.startswith("#else"):
        state[-1] = not state[-1]
        continue
    if s.startswith("#endif"):
        # The declaration itself sits in an #else whose #endif follows it.
        if state:
            state.pop()
        continue
    if all(state):
        kept.append(line)

text = re.sub(r"/\*.*?\*/", "", "\n".join(kept), flags=re.S)
values = [int(v) for v in re.findall(r"\d+", text)]
if len(values) != 1272:
    raise SystemExit(f"{len(values)} values, not 1272")

out = [
    "//! The PVQ codeword counts `U(N, K)` (libopus 1.5.2's `CELT_PVQ_U_DATA`,",
    "//! `celt/cwrs.c`, without Opus Custom's rows), copyright Xiph.Org and the",
    "//! contributors named in its `COPYING`, used under libopus's BSD licence",
    "//! (`licenses/libopus-COPYING`). Written by `tools/convert_pvq.py`.",
    "",
    "/// `U(N, K)` for the band sizes a standard Opus mode can split into, row by",
    "/// row: row `i` begins at `PVQ_U_ROW[i]`.",
    "pub(crate) static PVQ_U_DATA: [u32; 1272] = [",
]
for i in range(0, len(values), 8):
    out.append("    " + ", ".join(str(v) for v in values[i:i + 8]) + ",")
out.append("];")
out.append("")
out.append("/// Where each row of `PVQ_U_DATA` begins (`CELT_PVQ_U_ROW`).")
out.append("pub(crate) const PVQ_U_ROW: [usize; 15] = [")
out.append("    0, 176, 351, 525, 698, 870, 1041, 1131, 1178, 1207, 1226, 1240, 1248, 1254, 1257,")
out.append("];")
out.append("")
Path(sys.argv[2]).write_text("\n".join(out), encoding="utf-8", newline="\n")
print("wrote", len(values))
