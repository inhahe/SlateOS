"""Writes tests/data/references.txt: what Tremor makes of every stream in
tests/data, as tools/reference.c reports it.

    python3 tools/references.py [reference binary]

For each stream: its headers, its clean decode, and its decode damaged by
each of DAMAGE_SEEDS. The binary defaults to ~/vorbisref/build/reference
(tools/build_reference.sh). tests/streams.rs and tests/damage.rs hold the
port to every line.
"""

import pathlib
import subprocess
import sys

DATA = pathlib.Path(__file__).resolve().parent.parent / "tests" / "data"
REFERENCE = sys.argv[1] if len(sys.argv) > 1 else str(pathlib.Path.home() / "vorbisref/build/reference")
DAMAGE_SEEDS = range(1, 9)
SETUP_SEEDS = range(1, 25)


def reference(*args):
    out = subprocess.run([REFERENCE, *args], capture_output=True, text=True, encoding="utf-8")
    if out.returncode < 0:
        # Tremor crashed: hostile headers can make it divide by zero or
        # read past an array. The port must merely not.
        return f"tremor crashed (signal {-out.returncode})"
    if out.returncode not in (0, 1):
        raise SystemExit(f"{args}: exit {out.returncode}: {out.stderr}")
    return " | ".join(line.strip() for line in out.stdout.strip().splitlines())


lines = []
for path in sorted(DATA.glob("*.ogg")):
    name = path.name
    lines.append(f"{name} headers => {reference('headers', str(path))}")
    lines.append(f"{name} decode => {reference('decode', str(path))}")
    for seed in DAMAGE_SEEDS:
        lines.append(f"{name} damage {seed} => {reference('damage', str(path), str(seed))}")
    for seed in SETUP_SEEDS:
        lines.append(f"{name} setupdamage {seed} => {reference('setupdamage', str(path), str(seed))}")
(DATA / "references.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
print(f"{len(lines)} references")
