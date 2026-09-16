#!/usr/bin/env python3
"""Report library exports that no other crate names.

WHY
---

Rust's `dead_code` lint stops at the crate boundary. A `pub fn` in a library
is, as far as the compiler is concerned, used -- somebody outside might call
it -- so a capability can be built, documented, tested, and called by nothing
at all, and every signal a developer normally trusts stays green. The crate
compiles. Its tests pass. `cargo clippy` is silent.

On 2026-09-16 this lane found five of them by accident, in one day:

  * `guitk::text::available_families`, whose doc comment described it as "what
    a font picker lists". There was no font picker.
  * `guitk::text::set_font_family` / `set_mono_family` -- the settings existed,
    were round-tripped by a test written to catch a forgotten field, and were
    applied by nobody, so the configured UI font was ignored system-wide.
  * `apps/lockscreen`'s `PasswordValidator`, off every production path but
    still linking a key derivation into the shipped binary.
  * `ziparchive::parse_at` / `extract_entry_at` / `entry_data_at` -- a ranged
    reader built so an archive need not be held in memory, while the archive
    manager went on reading whole files.
  * `ziparchive::ZipWriter` / `WriteStream` / `copy_entry`, whose doc names the
    exact pairing nobody had written.

Each was found by reading something adjacent. None was found by a tool, and
the tree had no way to notice.

WHAT IT REPORTS
---------------

An item exported by a *library* crate and named in no other crate: free
functions, structs, enums and traits. "Named" is a word-boundary match on the
identifier anywhere in another crate's Rust, which is deliberately generous --
the aim is confidence that a *reported* item really has no caller, not a
complete list of every unused one.

WHAT IT CANNOT SEE, and these are not small
-------------------------------------------

* **Methods.** `ZipWriter::copy_entry` is invisible here: resolving `x.foo()`
  to a type needs a compiler. The *type* is checked, so an entirely unused
  type is caught, but an unused method on a used type is not.
* **Items reached through a re-export** under a different name.
* **Trait methods called through the trait**, and impls generally.
* **Macro-generated calls.**

It also cannot tell "nobody has written the caller yet" from "this is a
deliberate part of a published API". That judgement is the reader's, which is
why this reports and does not refuse.

WHY IT DOES NOT FAIL A BUILD
----------------------------

A baselined gate would be the wrong shape twice over. The honest answer to
most entries is "not yet", so a refusing gate would be silenced immediately;
and a gate that only catches *additions* cannot see the survivals, which are
the whole problem -- `set_font_family` sat unused for as long as it existed.
Use `--strict` to make it exit 1 when the list is non-empty, if a caller wants
that.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

# `pub` items worth asking about. Methods are excluded deliberately -- see the
# module docstring.
# Bare `pub` only. `pub(crate)`, `pub(super)` and `pub(in path)` all restrict
# visibility to below the crate boundary, so none of them is an export and an
# absent external caller says nothing about them. The first version of this
# pattern treated the parenthesised form as optional and counted them all --
# caught by this file's own self-test before the number was quoted anywhere.
ITEM_RE = re.compile(
    r"^pub\s+(?:async\s+)?(?:const\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*"
    r"(fn|struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)

# Identifiers so common that a word-boundary match proves nothing. A report
# that cries wolf on `new` teaches its reader to skim.
TOO_COMMON = {
    "new",
    "default",
    "from",
    "len",
    "get",
    "set",
    "is_empty",
    "next",
    "parse",
    "read",
    "write",
    "open",
    "close",
    "run",
    "main",
    "build",
    "name",
    "value",
    "id",
    "count",
    "clear",
    "push",
    "pop",
    "insert",
    "remove",
    "contains",
    "iter",
    "fmt",
    "clone",
    "drop",
    "init",
    "state",
    "config",
    "error",
    "result",
}

# A run that finds fewer library crates than this has not searched the tree it
# thinks it has, and a clean report from it would be a clean report about
# nothing.
FLOOR_CRATES = 20


def tracked(root: Path, pattern: str) -> list[Path]:
    out = subprocess.run(
        ["git", "-C", str(root), "ls-files", pattern],
        capture_output=True,
        text=True,
        check=True,
    )
    return [root / line for line in out.stdout.splitlines() if line]


def crate_of(path: Path, root: Path) -> Path | None:
    for parent in path.parents:
        if (parent / "Cargo.toml").is_file():
            return parent
        if parent == root:
            break
    return None


def strip_test_modules(text: str) -> str:
    """Remove `#[cfg(test)]` blocks, so test-only exports are not reported.

    Brace-matched from the `mod` that follows the attribute. A regex that cut
    to the next `}` would stop at the first nested block and leave the rest of
    the module looking like ordinary code.
    """
    out = []
    i = 0
    while True:
        at = text.find("#[cfg(test)]", i)
        if at == -1:
            out.append(text[i:])
            return "".join(out)
        out.append(text[i:at])
        brace = text.find("{", at)
        if brace == -1:
            return "".join(out)
        depth = 0
        j = brace
        while j < len(text):
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        i = j + 1


# Bare `pub mod` only, for ITEM_RE's reason: a `pub(crate) mod` is not a way
# out of the crate.
PUB_MOD_RE = re.compile(r"^pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", re.MULTILINE)


def reachable_sources(crate: Path) -> list[Path]:
    """`lib.rs` and every file reachable from it through `pub mod`.

    Without this the report is half noise, and the noise is the same shape
    every time: an item marked `pub` inside a *private* module. It is `pub`
    because its own crate uses it across module boundaries, it is unreachable
    from outside, and no caller elsewhere is a fact about the module system
    rather than about the item. Counting those made the first run of this
    script report 2044 of 4114 exports, which is not a list anybody reads.
    """
    lib = crate / "src" / "lib.rs"
    if not lib.is_file():
        return []
    seen: list[Path] = [lib]
    queue = [lib]
    while queue:
        cur = queue.pop()
        try:
            text = strip_test_modules(cur.read_text(encoding="utf-8", errors="replace"))
        except OSError:
            continue
        base = cur.parent if cur.name in ("lib.rs", "mod.rs") else cur.with_suffix("")
        for name in PUB_MOD_RE.findall(text):
            for cand in (base / f"{name}.rs", base / name / "mod.rs"):
                if cand.is_file() and cand not in seen:
                    seen.append(cand)
                    queue.append(cand)
    return seen


def exports(root: Path) -> tuple[dict[str, list[tuple[str, str]]], int]:
    """Public items per library crate: `{crate: [(kind, name), ...]}`."""
    found: dict[str, list[tuple[str, str]]] = {}
    libs = 0
    for lib in tracked(root, "*/src/lib.rs") + tracked(root, "*/*/src/lib.rs"):
        crate = crate_of(lib, root)
        if crate is None:
            continue
        libs += 1
        rel = crate.relative_to(root).as_posix()
        items: list[tuple[str, str]] = []
        for src in reachable_sources(crate):
            try:
                text = strip_test_modules(src.read_text(encoding="utf-8", errors="replace"))
            except OSError:
                continue
            for kind, name in ITEM_RE.findall(text):
                if name in TOO_COMMON or name.startswith("_"):
                    continue
                items.append((kind, name))
        if items:
            found[rel] = sorted(set(items))
    return found, libs


SELFTEST_SOURCE = """
pub fn wanted(x: u8) -> u8 { x }
pub(crate) fn not_exported() {}
pub struct Kept;
pub enum Shape { A }
pub trait Sink {}
pub mod visible;
mod hidden;
fn private() {}

#[cfg(test)]
mod tests {
    pub fn test_only_export() {}
    fn nested() { if true { } }
}
"""


def selftest() -> int:
    """The cases this has to get right, including the ones that were wrong."""
    cases: list[tuple[str, bool]] = []

    def case(name: str, ok: bool) -> None:
        cases.append((name, ok))
        print(f"{'ok  ' if ok else 'FAIL'}  {name}")

    stripped = strip_test_modules(SELFTEST_SOURCE)
    case("a cfg(test) module is removed", "test_only_export" not in stripped)
    case(
        "...and its nested braces do not end the cut early",
        "nested" not in stripped and "pub fn wanted" in stripped,
    )

    items = dict((name, kind) for kind, name in ITEM_RE.findall(stripped))
    case("a pub fn is an export", items.get("wanted") == "fn")
    case("a pub struct is an export", items.get("Kept") == "struct")
    case("a pub enum is an export", items.get("Shape") == "enum")
    case("a pub trait is an export", items.get("Sink") == "trait")
    case("pub(crate) is not an export", "not_exported" not in items)
    case("a private fn is not an export", "private" not in items)
    case(
        "a test-only export is not reported",
        "test_only_export" not in items,
    )

    mods = PUB_MOD_RE.findall(SELFTEST_SOURCE)
    case("pub mod is followed", mods == ["visible"])
    case("a private mod is not", "hidden" not in mods)

    case("the noise list is applied", "new" in TOO_COMMON and "parse" in TOO_COMMON)

    failed = sum(1 for _, ok in cases if not ok)
    print()
    print(f"selftest: {len(cases) - failed}/{len(cases)} cases pass")
    return 1 if failed else 0


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    if any(flag in sys.argv for flag in ("--selftest", "--self-test")):
        return selftest()
    if selftest():
        print("check-unused-exports: SELFTEST FAILED -- not scanning", file=sys.stderr)
        return 2
    print()

    by_crate, libs = exports(root)

    if libs < FLOOR_CRATES:
        print(
            f"check-unused-exports: DECLINING -- found only {libs} library crate(s), "
            f"below the floor of {FLOOR_CRATES}.",
            file=sys.stderr,
        )
        return 2

    # One pass over every tracked Rust file, recording which crate it is in, so
    # the search below is a dictionary lookup rather than a grep per item.
    mentions: dict[str, set[str]] = {}
    for src in tracked(root, "*.rs"):
        crate = crate_of(src, root)
        owner = crate.relative_to(root).as_posix() if crate else ""
        try:
            text = src.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for word in set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", text)):
            mentions.setdefault(word, set()).add(owner)

    unused: list[tuple[str, str, str]] = []
    for crate, items in sorted(by_crate.items()):
        for kind, name in items:
            others = mentions.get(name, set()) - {crate}
            if not others:
                unused.append((crate, kind, name))

    total = sum(len(v) for v in by_crate.values())
    if unused:
        print(f"check-unused-exports: {len(unused)} of {total} export(s) "
              f"in {libs} library crate(s) are named by no other crate.\n")
        last = None
        for crate, kind, name in unused:
            if crate != last:
                print(f"  {crate}")
                last = crate
            print(f"      {kind} {name}")
        print(
            "\nNot necessarily a defect: an export with no caller yet is ordinary "
            "in a tree this size. What it is good for is the case where the doc "
            "comment names a caller that does not exist -- that one is a promise "
            "nothing keeps."
        )
    else:
        print(f"check-unused-exports: every one of {total} export(s) is named elsewhere.")

    return 1 if (unused and "--strict" in sys.argv) else 0


if __name__ == "__main__":
    sys.exit(main())
