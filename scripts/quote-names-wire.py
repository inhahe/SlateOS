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
import os
import re
import sys
import textwrap
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import selftestflag  # noqa: E402

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


def quoting_path(crate: Path) -> str:
    """The `path = ` value that reaches the `quoting` crate from `crate`.

    Not a constant: `userspace/du` wants `../quoting` but `init/servicebus`
    wants `../../userspace/quoting`, and a hardcoded `../quoting` in the
    second is a manifest that does not resolve -- a loud failure, but only
    after the rest of the wiring has already been written."""
    target = (ROOT / "userspace" / "quoting").resolve()
    rel = os.path.relpath(target, crate.resolve())
    return Path(rel).as_posix()


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
        f'[dependencies]\n{body}\nquoting = {{ path = "{quoting_path(crate)}" }}\n',
        1,
    )
    if not src.endswith("\n"):
        src += "\n"
    toml.write_text(src, encoding="utf-8", newline="")
    return "wired"


def _preceded_by_attribute(src: str, pos: int) -> bool:
    """Is the item at `pos` carrying an attribute on the line(s) above it?

    Walks back over blank lines and comments, because an attribute may sit
    above either. A doc comment counts as an attribute for this purpose -- it
    binds to the item after it exactly as `#[..]` does, and inserting between
    the two would re-document the wrong import.
    """
    for line in reversed(src[:pos].splitlines()):
        stripped = line.strip()
        if not stripped:
            continue
        if stripped.startswith("#["):
            return True
        # A doc comment binds to the item after it exactly as an attribute
        # does, so inserting between the two would re-document the wrong
        # import. An inner one (`//!`) belongs to the enclosing module and is
        # a safe boundary.
        if stripped.startswith("///"):
            return True
        # A plain comment binds to nothing; keep looking past it for an
        # attribute above.
        if stripped.startswith("//"):
            continue
        return False
    return False


def wire_use(path: Path) -> str:
    src = path.read_text(encoding="utf-8")
    used = [f for f in FUNCS if re.search(rf"\b{f}\s*\(", src)]
    if not used:
        return "no-calls"
    if re.search(r"^use quoting::", src, re.M):
        return "already"
    imp = used[0] if len(used) == 1 else "{" + ", ".join(sorted(used)) + "}"
    line = f"use quoting::{imp};\n"
    # Insert before a top-level `use` that does NOT have an attribute attached
    # to it, which rustfmt then sorts into place.
    #
    # The attribute check is the whole of this. Anchoring on the first `use`
    # alone put the new import between an existing `#[cfg(not(test))]` and the
    # `use` it was written for, which does TWO wrong things at once and only
    # announces one of them:
    #
    #     #[cfg(not(test))]        #[cfg(not(test))]
    #     use std::env;      ->    use quoting::quoteaf_os;   <- now gated
    #                              use std::env;              <- now ungated
    #
    # `userspace/chpasswd` is the crate that did it. The gated import failed
    # to compile for the test target and that was noticed; `use std::env`
    # silently losing its gate was not, and nothing would have reported it.
    # An attribute binds to the item after it, so an insertion point is only
    # safe if nothing is bound to the item it displaces.
    m = next(
        (
            m
            for m in re.finditer(r"^use ", src, re.M)
            if not _preceded_by_attribute(src, m.start())
        ),
        None,
    )
    if not m:
        return "no-use-block"
    src = src[: m.start()] + line + src[m.start() :]
    path.write_text(src, encoding="utf-8", newline="")
    return "wired"


def wire(crate: Path, why: str) -> int:
    if not (crate / "Cargo.toml").is_file():
        print(f"no Cargo.toml under {crate}", file=sys.stderr)
        return 2
    print(f"{crate.name}: Cargo.toml {wire_cargo(crate, why)}")
    for p in entry_points(crate):
        r = wire_use(p)
        if r != "no-calls":
            print(f"{crate.name}: {p.relative_to(crate).as_posix()} {r}")
    return 0


def selftest() -> int:
    """Where this tool writes, and the one place it must not.

    An attribute binds to the item after it, so inserting a `use` immediately
    below one gives the new import an attribute meant for something else AND
    takes it away from the item that had it. Only the first of those fails to
    compile; the second is silent. `userspace/chpasswd` is the crate it
    happened to, on 2026-09-11, in a batch of thirty.
    """
    cases = [
        ("#[cfg(not(test))]\nuse std::env;\n", True, "the chpasswd shape"),
        ("use std::env;\n", False, "a bare use"),
        ("//! module doc\n\nuse std::env;\n", False, "an inner doc is a boundary"),
        ("/// documents the import\nuse std::env;\n", True, "a doc comment binds too"),
        ("// an ordinary remark\nuse std::env;\n", False, "a plain comment binds to nothing"),
        ("// remark\n#[cfg(unix)]\nuse std::env;\n", True, "an attribute above a remark"),
        ("fn f() {}\n\nuse std::env;\n", False, "an item above is a boundary"),
    ]
    failures = []
    for src, want, label in cases:
        got = _preceded_by_attribute(src, src.index("use std::env;"))
        if got != want:
            failures.append(f"{label}: want {want}, got {got}")

    # And the insertion end to end: the import must land ABOVE the first
    # unattributed use and leave the attributed one exactly as it was.
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        f = Path(td) / "main.rs"
        f.write_text(
            "#[cfg(not(test))]\nuse std::env;\nuse std::fs;\n\nfn g() { let _ = quoteaf_os(\"x\"); }\n",
            encoding="utf-8",
            newline="",
        )
        wire_use(f)
        got = f.read_text(encoding="utf-8")
    want = "#[cfg(not(test))]\nuse std::env;\nuse quoting::quoteaf_os;\nuse std::fs;\n"
    if want not in got:
        failures.append(f"insertion point: wanted {want!r} in {got!r}")

    for f2 in failures:
        print(f"selftest FAIL {f2}")
    print(f"selftest: {len(cases) + 1 - len(failures)}/{len(cases) + 1} cases pass")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("crate", type=Path, nargs="?")
    ap.add_argument("--why", help="why this crate's words are untrusted")
    ap.add_argument(
        "--batch",
        type=Path,
        help="a TSV of `<crate>\\t<why>`, as `quote-names-why.py --batch` "
        "writes. Taking the rationales from a file rather than from argv is "
        "not only convenience: several of these messages contain an em dash, "
        "and a Windows console hands a non-ASCII argument to Python in the "
        "OEM code page, so passing one on the command line writes mojibake "
        "into the manifest -- silently, because a comment is never compiled.",
    )
    if selftestflag.wants_selftest(sys.argv[1:]):
        return selftest()

    a = ap.parse_args()

    if a.batch:
        if a.crate or a.why:
            ap.error("--batch takes the crates and the rationales from the file")
        rc = 0
        lines = a.batch.read_text(encoding="utf-8").splitlines()
        for n, line in enumerate(lines, start=1):
            if not line.strip():
                continue
            name, sep, why = line.partition("\t")
            if not sep or not why.strip():
                print(f"{a.batch}:{n}: no rationale after a tab", file=sys.stderr)
                rc = 2
                continue
            name = name.strip()
            crate = ROOT / (name if "/" in name else f"userspace/{name}")
            rc = wire(crate, why.strip()) or rc
        return rc

    if not a.crate or not a.why:
        ap.error("a crate and --why are required unless --batch is given")
    return wire(a.crate if a.crate.is_absolute() else ROOT / a.crate, a.why)


if __name__ == "__main__":
    raise SystemExit(main())
