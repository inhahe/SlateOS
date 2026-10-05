#!/usr/bin/env python3
"""Times XZ Utils 5.2.5's `xz`, single-threaded, on the inputs of `rate.rs`,
for the comparison its doc comment records. Run from Windows, after
`tests/data/generate.py` has built `~/xz525` in WSL:

    python xz/benches/compare.py

Prints the same table as `cargo bench -p xz`, best of two runs, the input
read from WSL's own /tmp so that only the coding is timed.
"""

from __future__ import annotations

import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tests" / "data"))

from generate import gen, wsl, wsl_path  # noqa: E402  (path set above)


def seconds(script: str) -> float:
    """The shortest of two `time` readings of `script` in WSL's bash."""
    out = wsl(
        "TIMEFORMAT=%R\n"
        "for i in 1 2; do { time " + script + " ; } 2>&1; done\n"
    ).decode("utf-8")
    return min(float(line) for line in out.split() if line.replace(".", "", 1).isdigit())


def main() -> None:
    print("| input | preset | compress MiB/s | ratio | decompress MiB/s |")
    print("|---|---|---|---|---|")
    for name, data in [("text, 8 MiB", gen("text", 8 << 20, 1)), ("random, 2 MiB", gen("random", 2 << 20, 2))]:
        tmp = HERE / "input.tmp"
        tmp.write_bytes(data)
        try:
            wsl(f'cp "{wsl_path(tmp)}" /tmp/xzbench.in\n')
        finally:
            tmp.unlink()
        mib = len(data) / (1024 * 1024)
        for level in (0, 1, 6, 9):
            t_c = seconds(f"~/xz525/build/xz -T1 -{level} -c /tmp/xzbench.in > /tmp/xzbench.xz")
            size = int(wsl("stat -c %s /tmp/xzbench.xz\n").decode().strip())
            t_d = seconds("~/xz525/build/xz -d -c /tmp/xzbench.xz > /dev/null")
            print(f"| {name} | -{level} | {mib / t_c:.1f} | {size / len(data):.3f} | {mib / t_d:.1f} |")
    wsl("rm -f /tmp/xzbench.in /tmp/xzbench.xz\n")


if __name__ == "__main__":
    main()
