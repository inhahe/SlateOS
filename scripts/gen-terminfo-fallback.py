#!/usr/bin/env python3
"""Write userspace/terminfo/src/fallback_data.rs from the reference's compiled terminfo entries.

`userspace/terminfo` carries, built in, the entries ncurses would be given
with `--with-fallbacks` -- consulted only when the database has no entry for
the name -- so that the terminal SlateOS's own terminal claims to be is known
even on an image with no terminfo database (design-decisions §1066).

The entries are the reference's own compiled files: Ubuntu 24.04's
ncurses-base 6.4+20240113, as installed in WSL. Run it there:

    wsl -- python3 scripts/gen-terminfo-fallback.py

It overwrites the data file and prints what it wrote; nothing else reads it.
"""
import os
import sys

# (Rust constant name, compiled file) -- in the order the fallbacks are tried.
ENTRIES = [
    ("XTERM_256COLOR", "/usr/share/terminfo/x/xterm-256color"),
]

HERE = os.path.dirname(os.path.abspath(__file__))
DST = os.path.join(HERE, "..", "userspace", "terminfo", "src", "fallback_data.rs")


def constant(name, path):
    data = open(path, "rb").read()
    rows = []
    for i in range(0, len(data), 16):
        rows.append("    " + ", ".join("0x%02x" % b for b in data[i:i + 16]) + ",")
    return (f"/// `{os.path.basename(path)}`: {len(data)} bytes from `{path}`.\n"
            f"pub(crate) const {name}: &[u8] = &[\n" + "\n".join(rows) + "\n];\n"), len(data)


def main():
    parts = []
    for name, path in ENTRIES:
        text, size = constant(name, path)
        parts.append(text)
        print(f"{name}: {size} bytes from {path}")
    header = (
        "//! The compiled entries built into this library, as ncurses carries those\n"
        "//! named to `--with-fallbacks`: see [`super::fallback`].\n"
        "//!\n"
        "//! Generated from the reference's database -- Ubuntu 24.04's ncurses-base\n"
        "//! 6.4+20240113 -- by `scripts/gen-terminfo-fallback.py`. Not edited by\n"
        "//! hand: rerun the script.\n\n"
    )
    names = ", ".join(n for n, _ in ENTRIES)
    table = f"/// Every fallback, in the order they are tried.\npub(crate) const ALL: &[&[u8]] = &[{names}];\n"
    with open(DST, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "\n".join(parts) + "\n" + table)
    print("wrote", os.path.normpath(DST))
    return 0


if __name__ == "__main__":
    sys.exit(main())
