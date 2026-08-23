#!/usr/bin/env python3
"""Wire the `quoting` crate into a crate that `quote-names.py --fix` just edited.

`--fix` rewrites the call sites and then tells you to do two things by hand:
add the dependency and add the `use`. Doing those two things 700-odd times is
the part of the burn-down that is pure clerical work, and the part where a
slip is invisible -- a missing `use` is a compile error you will notice, but a
dependency comment pasted from the wrong crate is a lie in the tree that
nobody notices at all.

So this does exactly those two edits and nothing else:

  python scripts/quote-names-wire.py <crate-dir> --why "<one paragraph>"

* The `[dependencies]` line gains `quoting = { path = "../quoting" }`,
  preceded by `--why` rewrapped as a comment. **The rationale is required and
  has no default**, on purpose: the whole value of the comment is that it says
  what *this* crate's untrusted words are, and a default would be a blank that
  every crate silently shares.
* The entry point gains `use quoting::{...};`, with the import set derived
  from which functions the file actually calls after `--fix` -- so a crate
  that ended up using only `quoteaf_os` does not import a `quotef_os` it never
  calls and fail `-D warnings`.

It is idempotent: a crate already wired is left alone and reported as such,
so a batch can be re-run after fixing one member of it.
"""

from __future__ import annotations

import argparse
import re
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The functions `--fix` emits. Order matters: it is the order they go into the
# `use` list, which rustfmt would otherwise sort for us on the next run.
FUNCS = ("quoteaf_os", "quotef_os", "quoteaf", "quotef")


def entry_points(crate: Path) -> list[Path]:
    """Every file that might need the `use`: the bin/lib roots and, for a
    multicall crate, each file under `src/bin`."""
    out = []
    for rel in ("src/main.rs", "src/lib.rs"):
        p = crate / rel
        if p.is_file():
            out.append(p)
    binv = crate / "src" / "bin"
    if binv.is_dir():
        out += sorted(p for p in binv.rglob("*.rs"))
    return out


def wire_cargo(crate: Path, why: str) -> str:
    toml = crate / "Cargo.toml"
    src = toml.read_text(encoding="utf-8")
    if "quoting = {" in src:
        return "already"
    if "[dependencies]\n" not in src:
        # A crate with no dependency table at all: append one rather than
        # guess where it should go relative to `[lints]`/`[dev-dependencies]`.
        if not src.endswith("\n"):
            src += "\n"
        src += "\n[dependencies]\n"
    body = "\n".join(
        textwrap.wrap(
            " ".join(why.split()),
            width=76,
            initial_indent="# ",
            subsequent_indent="# ",
            break_long_words=False,
            break_on_hyphens=False,
        )
    )
    src = src.replace(
        "[dependencies]\n",
        f'[dependencies]\n{body}\nquoting = {{ path = "../quoting" }}\n',
        1,
    )
    if not src.endswith("\n"):
        src += "\n"
    toml.write_text(src, encoding="utf-8", newline="")
    return "wired"


def wire_use(path: Path) -> str:
    src = path.read_text(encoding="utf-8")
    used = [f for f in FUNCS if re.search(rf"\b{f}\s*\(", src)]
    if not used:
        return "no-calls"
    if re.search(r"^use quoting::", src, re.M):
        return "already"
    imp = used[0] if len(used) == 1 else "{" + ", ".join(sorted(used)) + "}"
    line = f"use quoting::{imp};\n"
    # Insert before the first top-level `use`, which rustfmt then sorts into
    # place. Anchoring on the first `use` rather than on a named one keeps
    # this working for a crate whose imports are in any order.
    m = re.search(r"^use ", src, re.M)
    if not m:
        return "no-use-block"
    src = src[: m.start()] + line + src[m.start() :]
    path.write_text(src, encoding="utf-8", newline="")
    return "wired"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("crate", type=Path)
    ap.add_argument("--why", required=True, help="why this crate's words are untrusted")
    a = ap.parse_args()

    crate = a.crate if a.crate.is_absolute() else ROOT / a.crate
    if not (crate / "Cargo.toml").is_file():
        print(f"no Cargo.toml under {crate}", file=sys.stderr)
        return 2

    print(f"{crate.name}: Cargo.toml {wire_cargo(crate, a.why)}")
    for p in entry_points(crate):
        r = wire_use(p)
        if r != "no-calls":
            print(f"{crate.name}: {p.relative_to(crate).as_posix()} {r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
