#!/usr/bin/env python3
"""The IETF CELLAR working group's FLAC conformance files, with libFLAC's
answers, for `tests/ietf.rs` (an ignored test: the files are 440 MB, CC0,
fetched rather than carried).

    python tools/ietf.py DIR

clones https://github.com/ietf-wg-cellar/flac-test-files into DIR (if it is
not there), and writes next to each `.flac` its NAME.txt: libFLAC 1.5.0's
reading of it through `tools/reference.c` (built in WSL as its header says),
with seeks to a third, a half and the last sample where STREAMINFO gives the
length. Then:

    FLAC_IETF=DIR cargo test -p flac --test ietf -- --ignored

holds the crate to every answer, line for line.
"""

import os
import shlex
import subprocess
import sys

REFERENCE = "~/flacref/build/reference"
REPO = "https://github.com/ietf-wg-cellar/flac-test-files.git"


def wsl_path(p):
    p = os.path.abspath(p).replace(os.sep, "/")
    return "/mnt/" + p[0].lower() + p[2:]


def wsl(command):
    r = subprocess.run(["wsl", "-d", "Ubuntu", "--", "bash", "-c", command],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if r.returncode != 0:
        sys.exit(f"{command}: {r.stderr.decode('utf-8', 'replace')}")
    return r.stdout.decode("ascii", "replace")


def total_samples(path):
    with open(path, "rb") as f:
        data = f.read(64)
    if data[:4] != b"fLaC" or data[4] & 0x7F != 0:
        return 0
    return int.from_bytes(data[18:26], "big") & 0xF_FFFF_FFFF


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    root = sys.argv[1]
    if not os.path.isdir(root):
        subprocess.run(["git", "clone", "--depth", "1", REPO, root], check=True)
    count = 0
    for sub in ("subset", "uncommon", "faulty"):
        folder = os.path.join(root, sub)
        for name in sorted(os.listdir(folder)):
            if not name.endswith(".flac"):
                continue
            path = os.path.join(folder, name)
            quoted = shlex.quote(wsl_path(path))
            lines = wsl(f"{REFERENCE} {quoted}").splitlines()
            total = total_samples(path)
            if total > 0 and sub != "faulty":
                seeks = [total // 3, total // 2 + 1, total - 1]
                again = wsl(f"{REFERENCE} {quoted} {' '.join(map(str, seeks))}").splitlines()
                first = next(i for i, l in enumerate(again) if l.startswith("seek "))
                lines += [l for l in again[first:] if not l.startswith("end ")]
            with open(path[:-5] + ".txt", "w", encoding="utf-8", newline="\n") as f:
                f.write("\n".join(lines) + "\n")
            count += 1
            print(f"{sub}/{name}: {sum(l.startswith('frame') for l in lines)} frames, "
                  f"{sum(l.startswith('error') for l in lines)} errors")
    print(f"{count} answers written")


main()
