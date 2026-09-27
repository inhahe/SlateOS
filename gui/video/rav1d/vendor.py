#!/usr/bin/env python3
"""Re-vendor rav1d from its crates.io package: steps 1 and the copying of
VENDORED.md ("What was changed").

    python vendor.py <rav1d-X.Y.Z.crate> <empty destination directory>

Checks the package against the SHA-256 recorded below (update it, and
VENDORED.md, when moving to a new release), unpacks it, copies `lib.rs`,
`include/**/*.rs` and `src/*.rs` -- all of the Rust, none of the assembly --
with the licence and readme files, and formats every `.rs` file with
`rustfmt --edition 2024`, as this repository's pre-push gate requires. The
manifest, `VENDORED.md`, `safe.rs` and the other local changes are then carried
across by hand from the previous vendored copy, and the result diffed.
"""

import hashlib
import io
import pathlib
import subprocess
import sys
import tarfile

PACKAGE_SHA256 = "1932f060d5e7bd49dc9f8b272c1dc5e9ce0ffe141c28be900265d3989b36c9ed"
KEEP_FILES = ["lib.rs", "COPYING", "README.md", "THANKS.md", "NEWS.dav1d"]


def main() -> None:
    crate = pathlib.Path(sys.argv[1])
    dest = pathlib.Path(sys.argv[2])
    data = crate.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if digest != PACKAGE_SHA256:
        sys.exit(f"{crate}: sha256 {digest}, expected {PACKAGE_SHA256}")
    if dest.exists() and any(dest.iterdir()):
        sys.exit(f"{dest} is not empty; refusing to overwrite")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        members = {m.name.split("/", 1)[1]: m for m in tar.getmembers() if "/" in m.name and m.isfile()}
        wanted = [
            name
            for name in members
            if name in KEEP_FILES
            or (name.startswith("include/") and name.endswith(".rs"))
            or (name.startswith("src/") and name.count("/") == 1 and name.endswith(".rs"))
        ]
        for name in sorted(wanted):
            target = dest / name
            target.parent.mkdir(parents=True, exist_ok=True)
            source = tar.extractfile(members[name])
            if source is None:
                sys.exit(f"{name}: not a regular file")
            # Byte for byte: no newline translation on any platform.
            target.write_bytes(source.read())
    # Formatting lib.rs formats the module tree it declares; the rest are then
    # formatted one by one, as the gate checks them.
    for path in [dest / "lib.rs", *sorted(dest.rglob("*.rs"))]:
        subprocess.run(["rustfmt", "--edition", "2024", str(path)], check=True)
    print(f"vendored {len(wanted)} files into {dest}")


if __name__ == "__main__":
    main()
