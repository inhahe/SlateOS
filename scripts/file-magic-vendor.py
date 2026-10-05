#!/usr/bin/env python3
"""Vendor file 5.45's magic database into `userspace/file/magic/`.

`file` carries file 5.45's own magic database, the one upstream compiles into
`magic.mgc`: `magic/Header`, `magic/Localstuff` and every file of
`magic/Magdir`, which upstream's `magic/Makefile.am` copies into one directory
and compiles with `file -C -m magic`. This copies the same files into
`userspace/file/magic/` -- and nothing else, so that the directory is a magic
directory `file -m` can read as it stands. `userspace/file/build.rs` compiles
that directory as `file -C` would, and the program carries the result; nothing
here needs regenerating by hand.

    curl -sSfLO https://astron.com/pub/file/file-5.45.tar.gz
    python scripts/file-magic-vendor.py file-5.45.tar.gz

The tarball is pinned by SHA-256: a different one is refused rather than
silently producing a different database. Run it again only to move to a new
release, and then change the pin, the version in `file`'s `--version` and the
reference `scripts/file-diff.sh` builds, in the same commit. The release's
licence is `userspace/file/licenses/file-COPYING`, where the image's notices
are gathered from.
"""

import hashlib
import io
import sys
import tarfile
from pathlib import Path

PINNED_SHA256 = "fc97f51029bb0e2c9f4e3bffefdaf678f0e039ee872b9de5c002a6d09c784d82"
TOP = "file-5.45"

ROOT = Path(__file__).resolve().parent.parent
DEST = ROOT / "userspace" / "file" / "magic"


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: file-magic-vendor.py file-5.45.tar.gz", file=sys.stderr)
        return 2
    data = Path(sys.argv[1]).read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if digest != PINNED_SHA256:
        print(f"refusing: {sys.argv[1]} has SHA-256 {digest}, not the pinned {PINNED_SHA256}",
              file=sys.stderr)
        return 1

    fragments = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        for member in tar.getmembers():
            if not member.isfile():
                continue
            name = member.name
            body = tar.extractfile(member).read()
            if name in (f"{TOP}/magic/Header", f"{TOP}/magic/Localstuff"):
                fragments[name.rsplit("/", 1)[1]] = body
            elif name.startswith(f"{TOP}/magic/Magdir/"):
                fragments[name.rsplit("/", 1)[1]] = body
    if "Header" not in fragments or len(fragments) < 300:
        print("refusing: the tarball does not hold the expected magic files", file=sys.stderr)
        return 1

    # Replace what was there: a fragment upstream removed must not linger.
    if DEST.exists():
        for old in DEST.iterdir():
            old.unlink()
    DEST.mkdir(parents=True, exist_ok=True)
    for name, body in fragments.items():
        (DEST / name).write_bytes(body)
    print(f"vendored {len(fragments)} fragments into {DEST}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
